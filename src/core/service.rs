use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;
use {super::repo::MigrationRepository, crate::core::migration as util, anyhow::Result, std::path::Path};

#[derive(Debug, Clone, Copy)]
pub enum OutputFormat {
    Human,
    Json,
}

pub struct MigrationService<R: MigrationRepository> {
    repo: R,
}

impl<R: MigrationRepository> MigrationService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn init(&self) -> Result<()> {
        self.repo.init_store().await
    }

    pub async fn new_migration(&self, path: &Path, comment: Option<&str>, locked: bool) -> Result<()> {
        let migration_id_path = util::create_migration_directory_with_extension(
            path,
            comment,
            locked,
            self.repo.migration_file_extension(),
        )?;
        println!("Created new migration: {}", migration_id_path.display());
        Ok(())
    }

    pub async fn apply_up(
        &self,
        path: &Path,
        id: &str,
        timeout: Option<u64>,
        yes: bool,
        dry_run: bool,
        locked: bool,
    ) -> Result<()> {
        let migration_dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
        let target_id = util::normalize_migration_id(id);
        let (up_sql, down_sql, meta) = util::read_migration_with_meta_with_extension(
            migration_dir,
            &target_id,
            self.repo.migration_file_extension(),
        )?;

        let diff_fn = || -> Result<()> { util::display_sql_migration(&target_id, &up_sql, "UP") };
        if !util::prompt_for_confirmation_with_diff(
            &format!("❓ Do you want to apply migration '{}'?", &target_id),
            yes,
            diff_fn,
        )? {
            println!("❌ Migration cancelled.");
            return Ok(());
        }

        let pre = self.repo.fetch_last_id().await?;
        self.repo
            .apply_migration(
                &target_id,
                &up_sql,
                &down_sql,
                meta.comment.as_deref(),
                pre.as_deref(),
                timeout,
                dry_run,
                locked,
            )
            .await?;
        util::print_migration_results(1, "applied");
        Ok(())
    }

    pub async fn apply_down(
        &self,
        path: &Path,
        id: &str,
        timeout: Option<u64>,
        remote: bool,
        yes: bool,
        dry_run: bool,
        unlock: bool,
    ) -> Result<()> {
        let migration_dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
        let target_id = util::normalize_migration_id(id);
        let down_sql = if remote {
            self.repo.fetch_down_sql(&target_id).await?.unwrap_or_default()
        } else {
            let (_up_sql, down_sql) = util::read_migration_files_with_extension(
                migration_dir,
                &target_id,
                self.repo.migration_file_extension(),
            )?;
            down_sql
        };

        let diff_fn = || -> Result<()> { util::display_sql_migration(&target_id, &down_sql, "DOWN") };
        if !util::prompt_for_confirmation_with_diff(
            &format!("❓ Do you want to revert migration '{}'?", &target_id),
            yes,
            diff_fn,
        )? {
            println!("❌ Revert cancelled.");
            return Ok(());
        }

        self.repo
            .revert_migration(&target_id, &down_sql, timeout, dry_run, unlock)
            .await?;
        util::print_migration_results(1, "reverted");
        Ok(())
    }

    pub async fn list(&self, output: OutputFormat) -> Result<()> {
        let history = self.repo.fetch_history().await?;
        let local = util::get_local_migrations(self.repo.get_path())?;
        match output {
            | OutputFormat::Human => {
                if history.is_empty() && local.is_empty() {
                    println!("No migrations found.");
                    return Ok(());
                }
                let migration_dir = self
                    .repo
                    .get_path()
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", self.repo.get_path().display()))?;
                util::render_migration_table(&local, &history, migration_dir)?;
                Ok(())
            },
            | OutputFormat::Json => {
                #[derive(serde::Serialize)]
                struct RowOut {
                    id: String,
                    remote: Option<DateTime<Utc>>,
                    local: bool,
                    comment: Option<String>,
                    locked: bool,
                }
                let mut all: BTreeMap<String, (Option<chrono::NaiveDateTime>, bool, Option<String>, bool)> =
                    BTreeMap::new();
                let migration_dir = self
                    .repo
                    .get_path()
                    .parent()
                    .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", self.repo.get_path().display()))?;

                for id in &local {
                    let entry = all.entry(id.clone()).or_default();
                    entry.1 = true;
                    // Get locked status from local meta.toml
                    if let Ok(meta) = util::read_migration_meta(migration_dir, id) {
                        entry.3 = meta.is_locked();
                    }
                }
                for (id, ts, comment, locked) in &history {
                    let entry = all.entry(id.clone()).or_default();
                    entry.0 = Some(*ts);
                    entry.2 = comment.clone();
                    // Use remote locked status if migration is applied
                    if entry.0.is_some() {
                        entry.3 = *locked;
                    }
                }
                let mut rows: Vec<RowOut> = Vec::new();
                for (id, (applied_at, is_local, comment, locked)) in all {
                    rows.push(RowOut {
                        id,
                        remote: applied_at.map(|naive| Utc.from_utc_datetime(&naive)),
                        local: is_local,
                        comment,
                        locked,
                    });
                }
                println!("{}", serde_json::to_string_pretty(&rows)?);
                Ok(())
            },
        }
    }

    pub async fn up(
        &self,
        path: &Path,
        timeout: Option<u64>,
        count: Option<usize>,
        yes: bool,
        dry_run: bool,
    ) -> Result<()> {
        let local = util::get_local_migrations(path)?;
        let applied = self.repo.fetch_applied_ids().await?;

        let mut to_apply: Vec<String> = local.difference(&applied).cloned().collect();
        to_apply.sort();
        if let Some(c) = count {
            to_apply.truncate(c);
        }

        if to_apply.is_empty() {
            println!("All migrations are up to date.");
            return Ok(());
        }

        // Non-linear warning
        let out_of_order = util::check_non_linear_history(&applied, &to_apply);
        if !out_of_order.is_empty() {
            let max_applied = applied.iter().max().cloned().unwrap_or_default();
            if !util::handle_non_linear_warning(&out_of_order, &max_applied)? {
                println!("Operation cancelled.");
                return Ok(());
            }
        }

        // Confirm
        println!("\n📋 About to apply {} migration(s):", to_apply.len());
        for id in &to_apply {
            println!("  - {}", id);
        }
        let migration_dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
        let to_apply_for_diff = to_apply.clone();
        let migration_file_extension = self.repo.migration_file_extension();
        let diff_fn = move || -> Result<()> {
            for id in &to_apply_for_diff {
                let (up_sql, _down) =
                    util::read_migration_files_with_extension(migration_dir, id, migration_file_extension)?;
                util::display_sql_migration(id, &up_sql, "UP")?;
            }
            Ok(())
        };
        if !util::prompt_for_confirmation_with_diff(
            "❓ Do you want to proceed with applying these migrations?",
            yes,
            diff_fn,
        )? {
            println!("❌ Migration cancelled.");
            return Ok(());
        }

        let mut previous: Option<String> = self.repo.fetch_last_id().await?;
        let mut applied_count = 0usize;
        for id in to_apply {
            let (up_sql, down_sql, meta) = util::read_migration_with_meta_with_extension(
                migration_dir,
                &id,
                self.repo.migration_file_extension(),
            )?;
            self.repo
                .apply_migration(
                    &id,
                    &up_sql,
                    &down_sql,
                    meta.comment.as_deref(),
                    previous.as_deref(),
                    timeout,
                    dry_run,
                    meta.is_locked(),
                )
                .await?;
            previous = Some(id.clone());
            applied_count += 1;
        }

        util::print_migration_results(applied_count, "applied");
        Ok(())
    }

    pub async fn down(
        &self,
        path: &Path,
        timeout: Option<u64>,
        count: usize,
        remote: bool,
        yes: bool,
        dry_run: bool,
        unlock: bool,
    ) -> Result<()> {
        let applied = self.repo.fetch_applied_ids().await?;
        if applied.is_empty() {
            println!("No migrations applied.");
            return Ok(());
        }
        let mut applied_sorted: Vec<String> = applied.into_iter().collect();
        applied_sorted.sort();
        applied_sorted.reverse();
        let targets: Vec<String> = applied_sorted.into_iter().take(count).collect();

        if targets.is_empty() {
            println!("Nothing to revert.");
            return Ok(());
        }

        let migration_dir = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
        let diff_fn = {
            let targets = targets.clone();
            let migration_file_extension = self.repo.migration_file_extension();
            move || -> Result<()> {
                for id in &targets {
                    let down_sql = if remote {
                        String::from("-- remote down sql omitted in preview")
                    } else {
                        let (_up_sql, down_sql) =
                            util::read_migration_files_with_extension(migration_dir, id, migration_file_extension)?;
                        down_sql
                    };
                    util::display_sql_migration(id, &down_sql, "DOWN")?;
                }
                Ok(())
            }
        };
        if !util::prompt_for_confirmation_with_diff(
            "❓ Do you want to proceed with reverting these migrations?",
            yes,
            diff_fn,
        )? {
            println!("❌ Revert cancelled.");
            return Ok(());
        }

        let mut reverted = 0usize;
        for id in targets {
            let down_sql = if remote {
                self.repo.fetch_down_sql(&id).await?.unwrap_or_default()
            } else {
                let (_up_sql, down_sql) = util::read_migration_files_with_extension(
                    migration_dir,
                    &id,
                    self.repo.migration_file_extension(),
                )?;
                down_sql
            };
            self.repo
                .revert_migration(&id, &down_sql, timeout, dry_run, unlock)
                .await?;
            reverted += 1;
        }

        util::print_migration_results(reverted, "reverted");
        Ok(())
    }
}
