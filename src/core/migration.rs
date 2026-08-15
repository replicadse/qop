use comfy_table::{Cell, CellAlignment, ContentArrangement, Table, modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, Write};
use {
    anyhow::{Context, Result},
    chrono::{Local, NaiveDateTime, TimeZone, Utc},
    std::{collections::HashSet, path::Path},
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MigrationMeta {
    pub comment: Option<String>,
    pub locked: Option<bool>,
}

impl Default for MigrationMeta {
    fn default() -> Self {
        Self {
            comment: None,
            locked: None,
        }
    }
}

impl MigrationMeta {
    /// Create a new MigrationMeta with a default comment including user and timestamp
    pub fn new_with_default_comment() -> Self {
        let username = whoami::username();
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let comment = format!("Created by {} at {}", username, timestamp);
        Self {
            comment: Some(comment),
            locked: None,
        }
    }

    /// Check if this migration is locked
    pub fn is_locked(&self) -> bool {
        self.locked.unwrap_or(false)
    }
}

/// Normalize migration ID to remove "id=" prefix if present
pub fn normalize_migration_id(id: &str) -> String {
    if id.starts_with("id=") {
        id.strip_prefix("id=").unwrap().to_string()
    } else {
        id.to_string()
    }
}

/// Get local migrations from directory by scanning for "id=" prefixed directories
pub fn get_local_migrations(path: &Path) -> Result<HashSet<String>> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    Ok(std::fs::read_dir(migration_dir)
        .with_context(|| format!("Failed to read migration directory: {}", migration_dir.display()))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if entry.file_type().ok()?.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                // Only accept directories that start with "id=" prefix
                if name.starts_with("id=") {
                    Some(name.strip_prefix("id=").unwrap().to_string())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect())
}

/// Create a new migration directory with timestamp-based ID
pub fn create_migration_directory(path: &Path, comment: Option<&str>, locked: bool) -> Result<std::path::PathBuf> {
    create_migration_directory_with_extension(path, comment, locked, "sql")
}

/// Create a new migration directory with backend-specific migration files.
pub fn create_migration_directory_with_extension(
    path: &Path,
    comment: Option<&str>,
    locked: bool,
    extension: &str,
) -> Result<std::path::PathBuf> {
    let id = Utc::now().timestamp_millis().to_string();
    let migration_path = path.parent().unwrap();
    let migration_id_path = migration_path.join(format!("id={}", id));
    std::fs::create_dir_all(&migration_id_path)
        .with_context(|| format!("Failed to create directory: {}", migration_id_path.display()))?;

    let up_path = migration_id_path.join(migration_file_name("up", extension)?);
    let down_path = migration_id_path.join(migration_file_name("down", extension)?);
    let meta_path = migration_id_path.join("meta.toml");

    let placeholder = migration_placeholder(extension);
    std::fs::write(&up_path, placeholder)
        .with_context(|| format!("Failed to write up migration: {}", up_path.display()))?;
    std::fs::write(&down_path, placeholder)
        .with_context(|| format!("Failed to write down migration: {}", down_path.display()))?;

    // Create meta.toml with provided comment or default comment including user and timestamp
    let meta = if let Some(comment) = comment {
        MigrationMeta {
            comment: Some(comment.to_string()),
            locked: if locked { Some(true) } else { None },
        }
    } else {
        let mut meta = MigrationMeta::new_with_default_comment();
        if locked {
            meta.locked = Some(true);
        }
        meta
    };
    let meta_content = toml::to_string(&meta).with_context(|| {
        format!(
            "Failed to serialize meta.toml for migration: {}",
            migration_id_path.display()
        )
    })?;
    std::fs::write(&meta_path, &meta_content)
        .with_context(|| format!("Failed to write meta.toml: {}", meta_path.display()))?;

    Ok(migration_id_path)
}

/// Read migration metadata from meta.toml file
pub fn read_migration_meta(migration_dir: &Path, migration_id: &str) -> Result<MigrationMeta> {
    // Migration folders always use "id=" prefix
    let migration_path = migration_dir.join(format!("id={}", migration_id));
    let meta_path = migration_path.join("meta.toml");

    // If meta.toml doesn't exist, return default (for backwards compatibility)
    if !meta_path.exists() {
        return Ok(MigrationMeta::default());
    }

    let meta_content = std::fs::read_to_string(&meta_path)
        .with_context(|| format!("Failed to read meta.toml: {}", meta_path.display()))?;

    let meta: MigrationMeta =
        toml::from_str(&meta_content).with_context(|| format!("Failed to parse meta.toml: {}", meta_path.display()))?;

    Ok(meta)
}

/// Read migration SQL files for a given migration ID
pub fn read_migration_files(migration_dir: &Path, migration_id: &str) -> Result<(String, String)> {
    read_migration_files_with_extension(migration_dir, migration_id, "sql")
}

/// Read backend-specific migration files for a given migration ID.
pub fn read_migration_files_with_extension(
    migration_dir: &Path,
    migration_id: &str,
    extension: &str,
) -> Result<(String, String)> {
    // Migration folders always use "id=" prefix
    let migration_path = migration_dir.join(format!("id={}", migration_id));
    let up_sql_path = migration_path.join(migration_file_name("up", extension)?);
    let down_sql_path = migration_path.join(migration_file_name("down", extension)?);

    let up_sql = std::fs::read_to_string(&up_sql_path)
        .with_context(|| format!("Failed to read up migration: {}", up_sql_path.display()))?;
    let down_sql = std::fs::read_to_string(&down_sql_path)
        .with_context(|| format!("Failed to read down migration: {}", down_sql_path.display()))?;

    Ok((up_sql, down_sql))
}

/// Read migration SQL files and metadata for a given migration ID
pub fn read_migration_with_meta(migration_dir: &Path, migration_id: &str) -> Result<(String, String, MigrationMeta)> {
    read_migration_with_meta_with_extension(migration_dir, migration_id, "sql")
}

/// Read backend-specific migration files and metadata for a given migration ID.
pub fn read_migration_with_meta_with_extension(
    migration_dir: &Path,
    migration_id: &str,
    extension: &str,
) -> Result<(String, String, MigrationMeta)> {
    let (up_sql, down_sql) = read_migration_files_with_extension(migration_dir, migration_id, extension)?;
    let meta = read_migration_meta(migration_dir, migration_id)?;
    Ok((up_sql, down_sql, meta))
}

fn migration_file_name(direction: &str, extension: &str) -> Result<String> {
    validate_migration_file_extension(extension)?;
    Ok(format!("{}.{}", direction, extension))
}

fn validate_migration_file_extension(extension: &str) -> Result<()> {
    if extension.is_empty()
        || !extension
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        anyhow::bail!("Invalid migration file extension: {}", extension);
    }
    Ok(())
}

fn migration_placeholder(extension: &str) -> &'static str {
    match extension {
        | "surql" => "-- SurrealQL goes here",
        | _ => "-- SQL goes here",
    }
}

/// Check if migration should be warned about for non-linear history
pub fn check_non_linear_history(applied_migrations: &HashSet<String>, migrations_to_apply: &[String]) -> Vec<String> {
    if applied_migrations.is_empty() || migrations_to_apply.is_empty() {
        return Vec::new();
    }

    let max_applied_migration = applied_migrations.iter().max().cloned().unwrap_or_default();

    migrations_to_apply
        .iter()
        .filter(|id| id.as_str() < max_applied_migration.as_str())
        .cloned()
        .collect()
}

/// Display non-linear history warning and get user confirmation
pub fn handle_non_linear_warning(out_of_order_migrations: &[String], max_applied: &str) -> Result<bool> {
    if out_of_order_migrations.is_empty() {
        return Ok(true);
    }
    println!("⚠️  Non-linear history detected!");
    println!("The following migrations would create a non-linear history:");
    for migration in out_of_order_migrations {
        println!("  - {}", migration);
    }
    println!("Latest applied migration: {}", max_applied);
    println!("");
    println!("This could cause issues with database schema consistency.");
    println!("Alternatively, you can run history fix to rename out-of-order migrations.");
    print!("Do you want to continue? [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();
    Ok(matches!(input.as_str(), "y" | "yes"))
}

/// Print migration application results
pub fn print_migration_results(applied_count: usize, action: &str) {
    if applied_count > 0 {
        println!("\n🎉 Successfully {} {} migration(s)!", action, applied_count);
    }
}

/// Prompt the user for confirmation with an optional diff callback.
pub fn prompt_for_confirmation_with_diff<F>(message: &str, yes: bool, diff_fn: F) -> Result<bool>
where
    F: Fn() -> Result<()>,
{
    if yes {
        return Ok(true);
    }
    loop {
        print!("{} [y/N/d]: ", message);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        match input.as_str() {
            | "y" | "yes" => return Ok(true),
            | "n" | "no" | "" => return Ok(false),
            | "d" | "diff" => {
                println!("\n📋 Migration Details:");
                diff_fn()?;
                println!("");
            },
            | _ => println!("Please enter 'y' (yes), 'n' (no), or 'd' (diff)"),
        }
    }
}

/// Prints a formatted SQL migration diff block to stdout for easy identification
pub fn display_sql_migration(migration_id: &str, sql: &str, direction: &str) -> Result<()> {
    let header_line = "────────────────────────────────────────────────────────";
    println!("");
    println!("▶ Migration: {} [{}]", migration_id, direction);
    println!("{}", header_line);
    print!("{}", sql);
    if !sql.ends_with('\n') {
        println!("");
    }
    println!("{}", header_line);
    println!("");
    Ok(())
}

/// Render a migration table given local and remote data in a unified way
pub fn render_migration_table(
    local_ids: &std::collections::HashSet<String>,
    remote_history: &[(String, NaiveDateTime, Option<String>, bool)],
    migration_dir: &std::path::Path,
) -> Result<()> {
    let mut all: BTreeMap<String, (Option<NaiveDateTime>, bool, Option<String>, bool)> = BTreeMap::new();

    for id in local_ids {
        let entry = all.entry(id.clone()).or_default();
        entry.1 = true;
        // Get locked status from local meta.toml
        if let Ok(meta) = read_migration_meta(migration_dir, id) {
            entry.3 = meta.is_locked();
        }
    }
    for (id, ts, comment, locked) in remote_history.iter() {
        let entry = all.entry(id.clone()).or_default();
        entry.0 = Some(*ts);
        entry.2 = comment.clone();
        // Use remote locked status if migration is applied
        if entry.0.is_some() {
            entry.3 = *locked;
        }
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            Cell::new("Migration ID"),
            Cell::new("Remote"),
            Cell::new("Local"),
            Cell::new("Comment"),
            Cell::new("Locked"),
        ]);

    for (id, (applied_at, is_local, comment, locked)) in all {
        let remote_str = if let Some(ts) = applied_at {
            let utc_dt = Local.from_utc_datetime(&ts);
            utc_dt.format("%Y-%m-%d %H:%M:%S %Z").to_string()
        } else {
            "❌".to_string()
        };
        let local_str = if is_local { "✅" } else { "❌" };
        let comment_str = comment.unwrap_or_else(|| "-".to_string());
        let locked_str = if locked { "🔒" } else { "" };

        table.add_row(vec![
            Cell::new(id),
            Cell::new(remote_str).set_alignment(CellAlignment::Center),
            Cell::new(local_str).set_alignment(CellAlignment::Center),
            Cell::new(comment_str),
            Cell::new(locked_str).set_alignment(CellAlignment::Center),
        ]);
    }

    println!("{table}");
    Ok(())
}
