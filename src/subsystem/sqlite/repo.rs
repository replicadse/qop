use {
    crate::core::repo::MigrationRepository,
    crate::subsystem::sqlite::migration as sq,
    crate::subsystem::sqlite::migration,
    anyhow::Result,
    chrono::NaiveDateTime,
    sqlx::Row,
    sqlx::sqlite::SqliteRow,
    sqlx::{Pool, Sqlite},
    std::collections::HashSet,
};

pub struct SqliteRepo {
    pub config: crate::subsystem::sqlite::config::SubsystemSqlite,
    pub pool: Pool<Sqlite>,
    pub path: std::path::PathBuf,
}

impl SqliteRepo {
    pub async fn from_config(
        path: &std::path::Path,
        config: crate::subsystem::sqlite::config::SubsystemSqlite,
        check_cli_version: bool,
    ) -> Result<Self> {
        let pool = sq::build_pool_from_config(path, &config, check_cli_version).await?;
        Ok(Self {
            config,
            pool,
            path: path.to_path_buf(),
        })
    }
}

#[async_trait::async_trait(?Send)]
impl MigrationRepository for SqliteRepo {
    async fn init_store(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        {
            // Create migrations table
            let mut query = sq::build_table_query("CREATE TABLE IF NOT EXISTS ", &self.config.tables.migrations);
            query.push(" (id TEXT PRIMARY KEY, version TEXT NOT NULL, up TEXT NOT NULL, down TEXT NOT NULL, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, pre TEXT, comment TEXT, locked BOOLEAN NOT NULL DEFAULT 0)");
            query.build().execute(&mut *tx).await?;

            // Create log table
            let mut log_query = sq::build_table_query("CREATE TABLE IF NOT EXISTS ", &self.config.tables.log);
            log_query.push(" (id TEXT PRIMARY KEY, migration_id TEXT NOT NULL, operation TEXT NOT NULL, sql_command TEXT NOT NULL, executed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP)");
            log_query.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        println!("Initialized migration tables.");
        Ok(())
    }

    async fn fetch_applied_ids(&self) -> Result<HashSet<String>> {
        let mut tx = self.pool.begin().await?;
        let ids = sq::get_applied_migrations(&mut tx, &self.config.tables.migrations).await?;
        tx.commit().await?;
        Ok(ids)
    }

    async fn fetch_last_id(&self) -> Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let id = sq::get_last_migration_id(&mut tx, &self.config.tables.migrations).await?;
        tx.commit().await?;
        Ok(id)
    }

    async fn apply_migration(
        &self,
        id: &str,
        up_sql: &str,
        down_sql: &str,
        comment: Option<&str>,
        pre: Option<&str>,
        timeout: Option<u64>,
        dry_run: bool,
        locked: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sq::set_timeout_if_needed(&mut *tx, timeout).await?;

        // Execute migration
        sq::execute_sql_statements(&mut tx, up_sql, id).await?;
        sq::insert_migration_record(
            &mut *tx,
            &self.config.tables.migrations,
            id,
            up_sql,
            down_sql,
            comment,
            pre,
            locked,
        )
        .await?;

        // Log successful migration
        sq::insert_log_entry(&mut *tx, &self.config.tables.log, id, "up", up_sql).await?;

        if dry_run {
            tx.rollback().await?;
        } else {
            tx.commit().await?;
        }
        Ok(())
    }

    async fn revert_migration(
        &self,
        id: &str,
        down_sql: &str,
        timeout: Option<u64>,
        dry_run: bool,
        unlock: bool,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sq::set_timeout_if_needed(&mut *tx, timeout).await?;

        // Check if migration is locked
        let is_locked = sq::is_migration_locked(&mut *tx, &self.config.tables.migrations, id).await?;
        if is_locked && !unlock {
            anyhow::bail!(
                "Migration {} is locked and cannot be reverted without --unlock flag",
                id
            );
        }

        // Execute revert migration
        sq::execute_sql_statements(&mut tx, down_sql, id).await?;
        sq::delete_migration_record(&mut *tx, &self.config.tables.migrations, id).await?;

        // Log successful revert
        sq::insert_log_entry(&mut *tx, &self.config.tables.log, id, "down", down_sql).await?;

        if dry_run {
            tx.rollback().await?;
        } else {
            tx.commit().await?;
        }
        Ok(())
    }

    async fn fetch_history(&self) -> Result<Vec<(String, NaiveDateTime, Option<String>, bool)>> {
        let mut tx = self.pool.begin().await?;
        let map = sq::get_migration_history(&mut tx, &self.config.tables.migrations).await?;
        tx.commit().await?;
        let mut v: Vec<(String, NaiveDateTime, Option<String>, bool)> = map
            .into_iter()
            .map(|(id, (ts, comment, locked))| (id, ts, comment, locked))
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(v)
    }

    async fn fetch_recent_for_revert_remote(&self) -> Result<Vec<(String, String)>> {
        let mut tx = self.pool.begin().await?;
        let rows: Vec<SqliteRow> =
            sq::get_recent_migrations_for_revert(&mut tx, &self.config.tables.migrations).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|row| (row.get("id"), row.get("down"))).collect())
    }

    async fn fetch_down_sql(&self, id: &str) -> Result<Option<String>> {
        // fetch by reading file in local mode; SQLite path stores down text in table too but no single get function provided
        let mut tx = self.pool.begin().await?;
        let mut q = sqlx::QueryBuilder::new("SELECT down FROM ");
        q.push(migration::quote_ident(&self.config.tables.migrations));
        q.push(" WHERE id = ?");
        let row = q.build().bind(id).fetch_optional(&mut *tx).await?;
        tx.commit().await?;
        Ok(row.map(|r| r.get("down")))
    }

    async fn fetch_all_migrations(&self) -> Result<Vec<(String, String, String, Option<String>)>> {
        let mut tx = self.pool.begin().await?;
        let mut q = sqlx::QueryBuilder::new("SELECT id, up, down, comment FROM ");
        q.push(migration::quote_ident(&self.config.tables.migrations));
        q.push(" ORDER BY id ASC");
        let rows = q.build().fetch_all(&mut *tx).await?;
        tx.commit().await?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get("id"), row.get("up"), row.get("down"), row.get("comment")))
            .collect())
    }

    fn get_path(&self) -> &std::path::Path {
        &self.path
    }
}
