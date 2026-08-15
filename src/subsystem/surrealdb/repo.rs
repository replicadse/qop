use {
    crate::core::repo::MigrationRepository, crate::subsystem::surrealdb::migration as sd, anyhow::Result,
    chrono::NaiveDateTime, std::collections::HashSet,
};

pub struct SurrealdbRepo {
    pub config: crate::subsystem::surrealdb::config::SubsystemSurrealdb,
    pub client: sd::SurrealdbClient,
    pub path: std::path::PathBuf,
}

impl SurrealdbRepo {
    pub async fn from_config(
        path: &std::path::Path,
        config: crate::subsystem::surrealdb::config::SubsystemSurrealdb,
        check_cli_version: bool,
    ) -> Result<Self> {
        let client = sd::build_client_from_config(path, &config, check_cli_version).await?;
        Ok(Self {
            config,
            client,
            path: path.to_path_buf(),
        })
    }
}

#[async_trait::async_trait(?Send)]
impl MigrationRepository for SurrealdbRepo {
    async fn init_store(&self) -> Result<()> {
        sd::init_store(&self.client, &self.config).await
    }

    async fn fetch_applied_ids(&self) -> Result<HashSet<String>> {
        sd::get_applied_migrations(&self.client, &self.config.tables.migrations, self.config.timeout).await
    }

    async fn fetch_last_id(&self) -> Result<Option<String>> {
        sd::get_last_migration_id(&self.client, &self.config.tables.migrations, self.config.timeout).await
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
        sd::apply_migration(
            &self.client,
            &self.config,
            id,
            up_sql,
            down_sql,
            comment,
            pre,
            timeout,
            dry_run,
            locked,
        )
        .await
    }

    async fn revert_migration(
        &self,
        id: &str,
        down_sql: &str,
        timeout: Option<u64>,
        dry_run: bool,
        unlock: bool,
    ) -> Result<()> {
        sd::revert_migration(&self.client, &self.config, id, down_sql, timeout, dry_run, unlock).await
    }

    async fn fetch_history(&self) -> Result<Vec<(String, NaiveDateTime, Option<String>, bool)>> {
        let mut history =
            sd::get_migration_history(&self.client, &self.config.tables.migrations, self.config.timeout).await?;
        history.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(history)
    }

    async fn fetch_recent_for_revert_remote(&self) -> Result<Vec<(String, String)>> {
        sd::get_recent_migrations_for_revert(&self.client, &self.config.tables.migrations, self.config.timeout).await
    }

    async fn fetch_down_sql(&self, id: &str) -> Result<Option<String>> {
        sd::get_migration_down_sql(&self.client, &self.config.tables.migrations, id, self.config.timeout).await
    }

    async fn fetch_all_migrations(&self) -> Result<Vec<(String, String, String, Option<String>)>> {
        sd::get_all_migration_data(&self.client, &self.config.tables.migrations, self.config.timeout).await
    }

    fn get_path(&self) -> &std::path::Path {
        &self.path
    }

    fn migration_file_extension(&self) -> &'static str {
        "surql"
    }
}
