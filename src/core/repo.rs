use anyhow::Result;
use chrono::NaiveDateTime;
use std::{collections::HashSet, path::Path};

#[async_trait::async_trait(?Send)]
pub trait MigrationRepository {
    async fn init_store(&self) -> Result<()>;
    async fn fetch_applied_ids(&self) -> Result<HashSet<String>>;
    async fn fetch_last_id(&self) -> Result<Option<String>>;
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
    ) -> Result<()>;
    async fn revert_migration(
        &self,
        id: &str,
        down_sql: &str,
        timeout: Option<u64>,
        dry_run: bool,
        unlock: bool,
    ) -> Result<()>;
    async fn fetch_history(&self) -> Result<Vec<(String, NaiveDateTime, Option<String>, bool)>>;
    async fn fetch_recent_for_revert_remote(&self) -> Result<Vec<(String, String)>>; // id, down
    async fn fetch_down_sql(&self, id: &str) -> Result<Option<String>>;
    async fn fetch_all_migrations(&self) -> Result<Vec<(String, String, String, Option<String>)>>; // id, up, down, comment
    fn get_path(&self) -> &Path;
    fn migration_file_extension(&self) -> &'static str {
        "sql"
    }
}
