use {
    crate::core::repo::MigrationRepository,
    crate::subsystem::postgres::migration as pg,
    anyhow::Result,
    chrono::NaiveDateTime,
    sqlx::{Pool, Postgres, Row},
    std::collections::HashSet,
};

pub struct PostgresRepo {
    pub config: crate::subsystem::postgres::config::SubsystemPostgres,
    pub pool: Pool<Postgres>,
    pub path: std::path::PathBuf,
}

impl PostgresRepo {
    pub async fn from_config(
        path: &std::path::Path,
        config: crate::subsystem::postgres::config::SubsystemPostgres,
        check_cli_version: bool,
    ) -> Result<Self> {
        let pool = pg::build_pool_from_config(path, &config, check_cli_version).await?;
        Ok(Self {
            config,
            pool,
            path: path.to_path_buf(),
        })
    }
}

#[async_trait::async_trait(?Send)]
impl MigrationRepository for PostgresRepo {
    async fn init_store(&self) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        {
            // Create migrations table
            let mut query = pg::build_table_query(
                "CREATE TABLE IF NOT EXISTS ",
                &self.config.schema,
                &self.config.tables.migrations,
            );
            query.push(" (id VARCHAR PRIMARY KEY, version VARCHAR NOT NULL, up VARCHAR NOT NULL, down VARCHAR NOT NULL, created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, pre VARCHAR, comment VARCHAR, locked BOOLEAN NOT NULL DEFAULT FALSE)");
            query.build().execute(&mut *tx).await?;

            // Create log table
            let mut log_query = pg::build_table_query(
                "CREATE TABLE IF NOT EXISTS ",
                &self.config.schema,
                &self.config.tables.log,
            );
            log_query.push(" (id VARCHAR PRIMARY KEY, migration_id VARCHAR NOT NULL, operation VARCHAR NOT NULL, sql_command TEXT NOT NULL, executed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)");
            log_query.build().execute(&mut *tx).await?;
        }
        tx.commit().await?;
        println!("Initialized migration tables.");
        Ok(())
    }

    async fn fetch_applied_ids(&self) -> Result<HashSet<String>> {
        let mut tx = self.pool.begin().await?;
        let ids = pg::get_applied_migrations(&mut tx, &self.config.schema, &self.config.tables.migrations).await?;
        tx.commit().await?;
        Ok(ids)
    }

    async fn fetch_last_id(&self) -> Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let id = pg::get_last_migration_id(&mut tx, &self.config.schema, &self.config.tables.migrations).await?;
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
        pg::set_timeout_if_needed(&mut *tx, timeout).await?;

        // Execute migration
        pg::execute_sql_statements(&mut tx, up_sql, id).await?;
        pg::insert_migration_record(
            &mut *tx,
            &self.config.schema,
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
        pg::insert_log_entry(&mut *tx, &self.config.schema, &self.config.tables.log, id, "up", up_sql).await?;

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
        pg::set_timeout_if_needed(&mut *tx, timeout).await?;

        // Check if migration is locked
        let is_locked =
            pg::is_migration_locked(&mut *tx, &self.config.schema, &self.config.tables.migrations, id).await?;
        if is_locked && !unlock {
            anyhow::bail!(
                "Migration {} is locked and cannot be reverted without --unlock flag",
                id
            );
        }

        // Execute revert migration
        pg::execute_sql_statements(&mut tx, down_sql, id).await?;
        pg::delete_migration_record(&mut *tx, &self.config.schema, &self.config.tables.migrations, id).await?;

        // Log successful revert
        pg::insert_log_entry(
            &mut *tx,
            &self.config.schema,
            &self.config.tables.log,
            id,
            "down",
            down_sql,
        )
        .await?;

        if dry_run {
            tx.rollback().await?;
        } else {
            tx.commit().await?;
        }
        Ok(())
    }

    async fn fetch_history(&self) -> Result<Vec<(String, NaiveDateTime, Option<String>, bool)>> {
        let mut tx = self.pool.begin().await?;
        let map = pg::get_migration_history(&mut tx, &self.config.schema, &self.config.tables.migrations).await?;
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
        let rows =
            pg::get_recent_migrations_for_revert(&mut tx, &self.config.schema, &self.config.tables.migrations).await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(|row| (row.get("id"), row.get("down"))).collect())
    }

    async fn fetch_down_sql(&self, id: &str) -> Result<Option<String>> {
        let mut tx = self.pool.begin().await?;
        let sql = pg::get_migration_down_sql(&mut tx, &self.config.schema, &self.config.tables.migrations, id)
            .await
            .ok();
        tx.commit().await?;
        Ok(sql)
    }

    async fn fetch_all_migrations(&self) -> Result<Vec<(String, String, String, Option<String>)>> {
        let mut tx = self.pool.begin().await?;
        let rows = pg::get_all_migration_data(&mut tx, &self.config.schema, &self.config.tables.migrations).await?;
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
