use {
    crate::config::{Config, DataSource, WithVersion},
    crate::subsystem::sqlite::config::SubsystemSqlite,
    anyhow::{Context, Result},
    chrono::{NaiveDateTime, Utc},
    sqlx::sqlite::SqlitePoolOptions,
    sqlx::{Pool, QueryBuilder, Row, Sqlite, sqlite::SqliteRow},
    std::{
        collections::{HashMap, HashSet},
        path::Path,
    },
};

use crate::core::migration::create_migration_directory;
use std::io::{self, Write};

// Database utility functions
pub(crate) fn get_effective_timeout(config: &SubsystemSqlite, provided_timeout: Option<u64>) -> Option<u64> {
    provided_timeout.or(config.timeout)
}

pub(crate) fn quote_ident(ident: &str) -> String {
    let mut s = String::with_capacity(ident.len() + 2);
    s.push('"');
    for ch in ident.chars() {
        if ch == '"' {
            s.push('"');
        }
        s.push(ch);
    }
    s.push('"');
    s
}

pub(crate) fn build_table_query<'a>(base_sql: &'a str, table: &str) -> QueryBuilder<'a, Sqlite> {
    let mut query = QueryBuilder::new(base_sql);
    query.push(quote_ident(table));
    query
}

pub(crate) async fn set_timeout_if_needed<'e, E>(executor: E, timeout_seconds: Option<u64>) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    if let Some(seconds) = timeout_seconds {
        let ms: i64 = (seconds as i64) * 1000;
        sqlx::query("PRAGMA busy_timeout = ?")
            .bind(ms)
            .execute(executor)
            .await?;
    }
    Ok(())
}

use crate::core::migration::prompt_for_confirmation_with_diff;

fn display_sql_migration(migration_id: &str, sql: &str, direction: &str) {
    let _ = crate::core::migration::display_sql_migration(migration_id, sql, direction);
}

fn create_bulk_migrations_diff_fn<'a>(
    migrations: &'a [String],
    migration_dir: &'a Path,
) -> impl Fn() -> Result<()> + 'a {
    move || -> Result<()> {
        for migration_id in migrations {
            let (up_sql, _down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;

            display_sql_migration(migration_id, &up_sql, "UP");
        }
        Ok(())
    }
}

fn create_bulk_reverts_diff_fn<'a>(
    migrations: &'a [SqliteRow],
    migration_dir: &'a Path,
    remote: bool,
) -> impl Fn() -> Result<()> + 'a {
    move || -> Result<()> {
        for row in migrations {
            let id: String = row.get("id");
            let down_sql: String = if remote {
                row.get("down")
            } else {
                let (_up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, &id)?;
                down_sql
            };

            display_sql_migration(&id, &down_sql, "DOWN");
        }
        Ok(())
    }
}

fn create_single_migration_diff_fn<'a>(
    migration_id: &'a str,
    sql: &'a str,
    direction: &'a str,
) -> impl Fn() -> Result<()> + 'a {
    move || -> Result<()> {
        display_sql_migration(migration_id, sql, direction);
        Ok(())
    }
}

pub(crate) async fn get_applied_migrations(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
) -> Result<HashSet<String>> {
    let mut query = build_table_query("SELECT id FROM ", table);
    query.push(" ORDER BY id ASC");
    Ok(query
        .build()
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| row.get("id"))
        .collect())
}

pub(crate) async fn get_last_migration_id(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
) -> Result<Option<String>> {
    let mut query = build_table_query("SELECT id FROM ", table);
    query.push(" ORDER BY id DESC LIMIT 1");
    Ok(query.build().fetch_optional(&mut **tx).await?.map(|row| row.get("id")))
}

pub(crate) async fn insert_migration_record<'e, E>(
    executor: E,
    table: &str,
    id: &str,
    up_sql: &str,
    down_sql: &str,
    comment: Option<&str>,
    pre_migration_id: Option<&str>,
    locked: bool,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let mut query = build_table_query("INSERT INTO ", table);
    query.push(" (id, version, up, down, comment, pre, locked) VALUES (?, ?, ?, ?, ?, ?, ?)");
    query
        .build()
        .bind(id)
        .bind(env!("CARGO_PKG_VERSION"))
        .bind(up_sql)
        .bind(down_sql)
        .bind(comment)
        .bind(pre_migration_id)
        .bind(locked)
        .execute(executor)
        .await?;
    Ok(())
}

pub(crate) async fn delete_migration_record<'e, E>(executor: E, table: &str, id: &str) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let mut query = build_table_query("DELETE FROM ", table);
    query.push(" WHERE id = ?");
    query.build().bind(id).execute(executor).await?;
    Ok(())
}

pub(crate) async fn is_migration_locked<'e, E>(executor: E, table: &str, id: &str) -> Result<bool>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let mut query = build_table_query("SELECT locked FROM ", table);
    query.push(" WHERE id = ?");
    let locked: Option<bool> = query
        .build()
        .bind(id)
        .fetch_optional(executor)
        .await?
        .map(|row| row.get("locked"));
    Ok(locked.unwrap_or(false))
}

pub(crate) async fn get_migration_history(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
) -> Result<HashMap<String, (NaiveDateTime, Option<String>, bool)>> {
    let mut query = build_table_query("SELECT id, created_at, comment, locked FROM ", table);
    query.push(" ORDER BY id ASC");
    Ok(query
        .build()
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.get("id"),
                (row.get("created_at"), row.get("comment"), row.get("locked")),
            )
        })
        .collect())
}

pub(crate) async fn get_recent_migrations_for_revert(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
) -> Result<Vec<SqliteRow>> {
    let mut query = build_table_query("SELECT id, down FROM ", table);
    query.push(" ORDER BY id DESC");
    Ok(query.build().fetch_all(&mut **tx).await?)
}

pub(crate) async fn get_all_migration_data(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
) -> Result<Vec<SqliteRow>> {
    let mut query = build_table_query("SELECT id, up, down FROM ", table);
    query.push(" ORDER BY id ASC");
    Ok(query.build().fetch_all(&mut **tx).await?)
}

pub(crate) async fn get_migration_down_sql(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    table: &str,
    migration_id: &str,
) -> Result<String> {
    let mut query = build_table_query("SELECT down FROM ", table);
    query.push(" WHERE id = ?");
    let row = query.build().bind(migration_id).fetch_one(&mut **tx).await?;
    Ok(row.get("down"))
}

pub(crate) async fn get_table_version(tx: &mut sqlx::Transaction<'_, Sqlite>, table: &str) -> Result<Option<String>> {
    let mut query = QueryBuilder::new("SELECT version FROM ");
    query.push(table);
    query.push(" ORDER BY id DESC LIMIT 1");
    Ok(query
        .build()
        .fetch_optional(&mut **tx)
        .await?
        .map(|row| row.get("version")))
}

pub(crate) async fn execute_sql_statements(
    tx: &mut sqlx::Transaction<'_, Sqlite>,
    sql: &str,
    migration_id: &str,
) -> Result<()> {
    match sqlx::raw_sql(sql).execute(&mut **tx).await {
        | Ok(_) => {
            // Statement executed successfully
        },
        | Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to execute statements in migration {}: {}",
                migration_id,
                e,
            ));
        },
    }
    Ok(())
}

pub(crate) async fn build_pool_from_config(
    path: &Path,
    sqlite_config: &SubsystemSqlite,
    check_cli_version: bool,
) -> Result<Pool<Sqlite>> {
    let uri = match &sqlite_config.connection {
        | DataSource::Static(connection) => connection.to_owned(),
        | DataSource::FromEnv(var) => std::env::var(var).with_context(|| {
            format!(
                "Missing environment variable '{}' referenced by [subsystem.sqlite].connection in {}",
                var,
                path.display()
            )
        })?,
    };

    let pool = SqlitePoolOptions::new().max_connections(1).connect(&uri).await?;
    if check_cli_version {
        let mut tx = pool.begin().await?;
        let table_exists = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
            .bind(&sqlite_config.tables.migrations)
            .fetch_optional(&mut *tx)
            .await?
            .is_some();
        if table_exists {
            if let Some(version) = get_table_version(&mut tx, &sqlite_config.tables.migrations).await? {
                let cli_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
                if !(cli_version.major == 0 && cli_version.minor == 0 && cli_version.patch == 0) {
                    let last_migration_version = semver::Version::parse(&version)?;
                    if last_migration_version > cli_version {
                        anyhow::bail!(
                            "Latest migration table version is older than the CLI version. Please run 'qop subsystem sqlite history fix' to rename out-of-order migrations."
                        );
                    }
                }
            }
        }
        tx.commit().await?;
    }
    Ok(pool)
}

pub(crate) fn get_local_migrations(path: &Path) -> Result<HashSet<String>> {
    crate::core::migration::get_local_migrations(path)
}

// Log operations
pub(crate) async fn insert_log_entry<'c, E>(
    executor: E,
    log_table: &str,
    migration_id: &str,
    operation: &str,
    sql_command: &str,
) -> Result<()>
where
    E: sqlx::Executor<'c, Database = Sqlite>,
{
    let log_id = uuid::Uuid::now_v7().to_string();
    let mut query = build_table_query("INSERT INTO ", log_table);
    query.push(" (id, migration_id, operation, sql_command) VALUES (?, ?, ?, ?)");
    query
        .build()
        .bind(log_id)
        .bind(migration_id)
        .bind(operation)
        .bind(sql_command)
        .execute(executor)
        .await?;
    Ok(())
}

// High-level command functions
pub async fn init_with_pool(migrations_table: &str, log_table: &str, pool: &Pool<Sqlite>) -> Result<()> {
    let mut tx = pool.begin().await?;
    {
        // Create migrations table
        let mut query = build_table_query("CREATE TABLE IF NOT EXISTS ", migrations_table);
        query.push(" (id TEXT PRIMARY KEY, version TEXT NOT NULL, up TEXT NOT NULL, down TEXT NOT NULL, created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP, pre TEXT, comment TEXT, locked BOOLEAN NOT NULL DEFAULT 0)");
        query.build().execute(&mut *tx).await?;

        // Create log table
        let mut log_query = build_table_query("CREATE TABLE IF NOT EXISTS ", log_table);
        log_query.push(" (id TEXT PRIMARY KEY, migration_id TEXT NOT NULL, operation TEXT NOT NULL, sql_command TEXT NOT NULL, executed_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP)");
        log_query.build().execute(&mut *tx).await?;
    };
    tx.commit().await?;
    println!("Initialized migration tables.");
    Ok(())
}

pub async fn new_migration(path: &Path) -> Result<()> {
    let migration_id_path = create_migration_directory(path, None, false)?;
    println!("Created new migration: {}", migration_id_path.display());
    Ok(())
}

pub async fn up(
    path: &Path,
    timeout: Option<u64>,
    count: Option<usize>,
    _diff: bool,
    dry: bool,
    yes: bool,
) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Sqlite(c) => c,
        | _ => anyhow::bail!("expected sqlite config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;
    let effective_timeout = get_effective_timeout(&config, timeout);

    let mut tx = pool.begin().await?;

    set_timeout_if_needed(&mut *tx, effective_timeout).await?;

    let applied_migrations = get_applied_migrations(&mut tx, &config.tables.migrations).await?;
    let mut last_migration_id = get_last_migration_id(&mut tx, &config.tables.migrations).await?;

    // Commit the initial query transaction
    tx.commit().await?;

    let mut migrations_to_apply: Vec<String> = local_migrations.difference(&applied_migrations).cloned().collect();

    migrations_to_apply.sort();

    let migrations_to_apply = if let Some(count) = count {
        migrations_to_apply.into_iter().take(count).collect()
    } else {
        migrations_to_apply
    };

    // Check for non-linear history
    let out_of_order_migrations =
        crate::core::migration::check_non_linear_history(&applied_migrations, &migrations_to_apply);
    if !out_of_order_migrations.is_empty() {
        let max_applied = applied_migrations.iter().max().cloned().unwrap_or_default();
        if !crate::core::migration::handle_non_linear_warning(&out_of_order_migrations, &max_applied)? {
            println!("Operation cancelled.");
            return Ok(());
        }
    }

    if migrations_to_apply.is_empty() {
        println!("All migrations are up to date.");
    } else {
        // Prompt for confirmation when not in silent mode
        println!("\n📋 About to apply {} migration(s):", migrations_to_apply.len());
        for migration_id in &migrations_to_apply {
            println!("  - {}", migration_id);
        }

        let diff_fn = create_bulk_migrations_diff_fn(&migrations_to_apply, migration_dir);

        if !prompt_for_confirmation_with_diff(
            "❓ Do you want to proceed with applying these migrations?",
            yes,
            diff_fn,
        )? {
            println!("❌ Migration cancelled.");
            return Ok(());
        }

        // Apply each migration in its own transaction
        for migration_id in &migrations_to_apply {
            println!("⏳ Applying migration: {}", migration_id);
            let id = migration_id.as_str();

            let (up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;

            // Start a new transaction for this migration
            let mut migration_tx = pool.begin().await?;

            // Set timeout for this transaction if specified
            set_timeout_if_needed(&mut *migration_tx, effective_timeout).await?;

            // Execute the migration SQL
            execute_sql_statements(&mut migration_tx, &up_sql, id).await?;

            // Record the migration in the tracking table
            insert_migration_record(
                &mut *migration_tx,
                &config.tables.migrations,
                id,
                &up_sql,
                &down_sql,
                None, // comment not available in this legacy function
                last_migration_id.as_deref(),
                false, // locked not available in this legacy function
            )
            .await?;

            // Commit or rollback based on dry-run mode
            if dry {
                migration_tx.rollback().await?;
                println!("🔄 Migration {} executed and rolled back (dry-run mode).", migration_id);
            } else {
                migration_tx.commit().await?;
                println!("✅ Migration {} applied successfully.", migration_id);
                last_migration_id = Some(id.to_string());
            }
        }

        if dry {
            crate::core::migration::print_migration_results(migrations_to_apply.len(), "tested in dry-run mode");
        } else {
            crate::core::migration::print_migration_results(migrations_to_apply.len(), "applied");
        }
    }

    Ok(())
}

pub async fn down(
    path: &Path,
    timeout: Option<u64>,
    count: Option<usize>,
    remote: bool,
    _diff: bool,
    dry: bool,
    yes: bool,
) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Sqlite(c) => c,
        | _ => anyhow::bail!("expected sqlite config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let effective_timeout = get_effective_timeout(&config, timeout);

    let mut tx = pool.begin().await?;

    set_timeout_if_needed(&mut *tx, effective_timeout).await?;

    let last_migrations = get_recent_migrations_for_revert(&mut tx, &config.tables.migrations).await?;

    let migrations_to_revert: Vec<SqliteRow> = if let Some(count) = count {
        last_migrations.into_iter().take(count).collect()
    } else {
        last_migrations.into_iter().take(1).collect()
    };

    // Commit the initial query transaction
    tx.commit().await?;

    if migrations_to_revert.is_empty() {
        println!("No migrations to revert.");
    } else {
        // Prompt for confirmation when not in silent mode
        println!("\n📋 About to revert {} migration(s):", migrations_to_revert.len());
        for row in &migrations_to_revert {
            let id: String = row.get("id");
            println!("  - {}", id);
        }

        let diff_fn = create_bulk_reverts_diff_fn(&migrations_to_revert, migration_dir, remote);

        if !prompt_for_confirmation_with_diff(
            "❓ Do you want to proceed with reverting these migrations?",
            yes,
            diff_fn,
        )? {
            println!("❌ Revert cancelled.");
            return Ok(());
        }

        // Revert each migration in its own transaction
        for row in migrations_to_revert {
            let id: String = row.get("id");
            let down_sql: String = if remote {
                row.get("down")
            } else {
                let (_up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, &id)?;
                down_sql
            };
            println!("Reverting migration: {}", id);

            // Start a new transaction for this migration revert
            let mut revert_tx = pool.begin().await?;

            // Set timeout for this transaction if specified
            set_timeout_if_needed(&mut *revert_tx, effective_timeout).await?;

            // Execute the down migration SQL
            execute_sql_statements(&mut revert_tx, &down_sql, &id).await?;

            // Remove the migration from the tracking table
            delete_migration_record(&mut *revert_tx, &config.tables.migrations, &id).await?;

            // Commit or rollback based on dry-run mode
            if dry {
                revert_tx.rollback().await?;
                println!("🔄 Migration {} reverted and rolled back (dry-run mode).", id);
            } else {
                revert_tx.commit().await?;
                println!("✅ Migration {} reverted.", id);
            }
        }
    }

    Ok(())
}

pub async fn list(path: &Path, migrations_table: &str, pool: &Pool<Sqlite>) -> Result<()> {
    let local_migrations = get_local_migrations(path)?;

    let mut tx = pool.begin().await?;

    // Gracefully handle absence of the remote table
    let table_exists = sqlx::query("SELECT name FROM sqlite_master WHERE type='table' AND name=?")
        .bind(migrations_table)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();

    let applied_map = if table_exists {
        get_migration_history(&mut tx, migrations_table).await?
    } else {
        std::collections::HashMap::new()
    };

    let mut remote: Vec<(String, chrono::NaiveDateTime, Option<String>, bool)> = applied_map
        .into_iter()
        .map(|(id, (ts, comment, locked))| (id, ts, comment, locked))
        .collect();
    remote.sort_by(|a, b| a.0.cmp(&b.0));

    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    crate::core::migration::render_migration_table(&local_migrations, &remote, migration_dir)?;

    tx.commit().await?;

    Ok(())
}

// Placeholder implementations for remaining functions
pub async fn apply_up(path: &Path, id: &str, timeout: Option<u64>, dry: bool, yes: bool) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Sqlite(c) => c,
        | _ => anyhow::bail!("expected sqlite config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;

    // Normalize the migration ID to remove "id=" prefix if present
    let target_migration_id = crate::core::migration::normalize_migration_id(&id);

    let mut tx = pool.begin().await?;

    // Get current applied migrations
    let applied_migrations = get_applied_migrations(&mut tx, &config.tables.migrations).await?;

    tx.commit().await?;

    // Check if migration exists locally
    if !local_migrations.contains(&target_migration_id) {
        return Err(anyhow::anyhow!(
            "Migration {} does not exist locally",
            target_migration_id
        ));
    }

    // Check if migration is already applied
    if applied_migrations.contains(&target_migration_id) {
        println!("Migration {} is already applied.", target_migration_id);
        return Ok(());
    }

    // Check for non-linear history
    let mut needs_confirmation = false;
    if !applied_migrations.is_empty() {
        let max_applied_migration = applied_migrations.iter().max().cloned().unwrap_or_default();

        if target_migration_id.as_str() < max_applied_migration.as_str() {
            println!("⚠️  Non-linear history detected!");
            println!(
                "Applying migration {} would create a non-linear history.",
                target_migration_id
            );
            println!("Latest applied migration: {}", max_applied_migration);
            println!();
            println!("This could cause issues with database schema consistency.");
            needs_confirmation = true;
        }
    }

    if needs_confirmation {
        print!("Do you want to continue? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("Operation cancelled.");
            return Ok(());
        }
    }

    // Confirm migration application
    let (up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, &target_migration_id)?;

    let diff_fn = create_single_migration_diff_fn(&target_migration_id, &up_sql, "UP");

    if !prompt_for_confirmation_with_diff(
        &format!("❓ Do you want to apply migration '{}'?", target_migration_id),
        yes,
        diff_fn,
    )? {
        println!("❌ Operation cancelled.");
        return Ok(());
    }

    // Apply the migration

    // Get the latest migration for the pre field
    let mut tx = pool.begin().await?;
    let last_migration_id = get_last_migration_id(&mut tx, &config.tables.migrations).await?;
    tx.commit().await?;

    // Execute the migration
    let mut migration_tx = pool.begin().await?;

    set_timeout_if_needed(&mut *migration_tx, effective_timeout).await?;

    if dry {
        println!("Testing migration: {}", target_migration_id);
    } else {
        println!("Applying migration: {}", target_migration_id);
    }

    execute_sql_statements(&mut migration_tx, &up_sql, &target_migration_id).await?;

    insert_migration_record(
        &mut *migration_tx,
        &config.tables.migrations,
        &target_migration_id,
        &up_sql,
        &down_sql,
        None, // comment not available in this legacy function
        last_migration_id.as_deref(),
        false, // locked not available in this legacy function
    )
    .await?;

    if dry {
        migration_tx.rollback().await?;
        println!(
            "🔄 Migration {} executed and rolled back (dry-run mode).",
            target_migration_id
        );
    } else {
        migration_tx.commit().await?;
        println!("✅ Migration {} applied successfully.", target_migration_id);
    }

    Ok(())
}

pub async fn apply_down(path: &Path, id: &str, timeout: Option<u64>, remote: bool, dry: bool, yes: bool) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Sqlite(c) => c,
        | _ => anyhow::bail!("expected sqlite config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;

    // Normalize the migration ID to remove "id=" prefix if present
    let target_migration_id = crate::core::migration::normalize_migration_id(&id);

    let mut tx = pool.begin().await?;

    // Get current applied migrations
    let applied_migrations = get_applied_migrations(&mut tx, &config.tables.migrations).await?;

    tx.commit().await?;

    // Check if migration is applied
    if !applied_migrations.contains(&target_migration_id) {
        return Err(anyhow::anyhow!(
            "Migration {} is not currently applied",
            target_migration_id
        ));
    }

    // Check for non-linear history (reverting a migration that's not the latest)
    let mut needs_confirmation = false;
    if !applied_migrations.is_empty() {
        let max_applied_migration = applied_migrations.iter().max().cloned().unwrap_or_default();

        if target_migration_id != max_applied_migration {
            println!("⚠️  Non-linear history detected!");
            println!(
                "Reverting migration {} would create a non-linear history.",
                target_migration_id
            );
            println!("Latest applied migration: {}", max_applied_migration);
            println!();
            println!("This could cause issues with database schema consistency.");
            needs_confirmation = true;
        }
    }

    if needs_confirmation {
        print!("Do you want to continue? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();

        if input != "y" && input != "yes" {
            println!("Operation cancelled.");
            return Ok(());
        }
    }

    // Get the down SQL from database or local file based on remote flag
    let down_sql: String = if remote {
        // Get from database
        let mut tx = pool.begin().await?;
        let sql = get_migration_down_sql(&mut tx, &config.tables.migrations, &target_migration_id).await?;
        tx.commit().await?;
        sql
    } else {
        // Get from local file via helper to ensure `id=` directory convention
        let (_up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, &target_migration_id)?;
        down_sql
    };

    // Confirm migration revert
    let diff_fn = create_single_migration_diff_fn(&target_migration_id, &down_sql, "DOWN");

    if !prompt_for_confirmation_with_diff(
        &format!("❓ Do you want to revert migration '{}'?", target_migration_id),
        yes,
        diff_fn,
    )? {
        println!("❌ Operation cancelled.");
        return Ok(());
    }

    // Execute the down migration
    let mut revert_tx = pool.begin().await?;

    set_timeout_if_needed(&mut *revert_tx, effective_timeout).await?;

    if dry {
        println!("Testing revert migration: {}", target_migration_id);
    } else {
        println!("Reverting migration: {}", target_migration_id);
    }

    execute_sql_statements(&mut revert_tx, &down_sql, &target_migration_id).await?;

    delete_migration_record(&mut *revert_tx, &config.tables.migrations, &target_migration_id).await?;

    if dry {
        revert_tx.rollback().await?;
        println!(
            "🔄 Migration {} reverted and rolled back (dry-run mode).",
            target_migration_id
        );
    } else {
        revert_tx.commit().await?;
        println!("✅ Migration {} reverted successfully.", target_migration_id);
    }

    Ok(())
}

pub async fn history_fix(path: &Path, migrations_table: &str, pool: &Pool<Sqlite>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;

    let mut tx = pool.begin().await?;

    let applied_migrations = get_applied_migrations(&mut tx, migrations_table).await?;

    let max_applied_migration = applied_migrations.iter().max().cloned().unwrap_or_default();

    let max_applied_ts = applied_migrations
        .iter()
        .filter_map(|id| id.parse::<i64>().ok())
        .max()
        .unwrap_or(0);

    let mut next_ts = std::cmp::max(max_applied_ts, Utc::now().timestamp_millis());

    let out_of_order_migrations: Vec<String> = local_migrations
        .difference(&applied_migrations)
        .filter(|id| id.as_str() < max_applied_migration.as_str())
        .cloned()
        .collect();

    if out_of_order_migrations.is_empty() {
        println!("No out-of-order migrations to fix.");
    } else {
        for old_id in out_of_order_migrations {
            next_ts += 1;
            let new_id = format!("id={}", next_ts);
            let old_path = migration_dir.join(format!("id={}", old_id));
            let new_path = migration_dir.join(&new_id);

            std::fs::rename(&old_path, &new_path).with_context(|| {
                format!(
                    "Failed to shuffle migration from {} to {}",
                    old_path.display(),
                    new_path.display()
                )
            })?;

            println!("Shuffled migration {} to {}", old_id, new_id);
        }
    }

    tx.commit().await?;

    Ok(())
}

pub async fn history_sync(path: &Path, migrations_table: &str, pool: &Pool<Sqlite>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;

    let mut tx = pool.begin().await?;

    // Get all migrations from the database
    let all_migrations = get_all_migration_data(&mut tx, migrations_table).await?;

    if all_migrations.is_empty() {
        println!("No migrations to sync.");
    } else {
        for row in all_migrations {
            let id: String = row.get("id");
            let up_sql: String = row.get("up");
            let down_sql: String = row.get("down");

            // Ensure local directory follows the "id=<id>" convention
            let migration_id_path = migration_dir.join(format!("id={}", id));
            std::fs::create_dir_all(&migration_id_path)
                .with_context(|| format!("Failed to create directory: {}", migration_id_path.display()))?;

            let up_path = migration_id_path.join("up.sql");
            let down_path = migration_id_path.join("down.sql");

            std::fs::write(&up_path, up_sql)
                .with_context(|| format!("Failed to write up migration: {}", up_path.display()))?;
            std::fs::write(&down_path, down_sql)
                .with_context(|| format!("Failed to write down migration: {}", down_path.display()))?;

            println!("Synced migration: {}", id);
        }
    }

    tx.commit().await?;

    Ok(())
}

pub async fn diff(path: &Path, migrations_table: &str, pool: &Pool<Sqlite>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;

    let mut tx = pool.begin().await?;

    let applied_migrations = get_applied_migrations(&mut tx, migrations_table).await?;

    tx.commit().await?;

    let mut pending_migrations: Vec<String> = local_migrations.difference(&applied_migrations).cloned().collect();

    pending_migrations.sort();

    if pending_migrations.is_empty() {
        println!("All migrations are up to date.");
    } else {
        for migration_id in &pending_migrations {
            let (up_sql, _down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;
            // Render with same formatting as interactive 'd'
            crate::core::migration::display_sql_migration(migration_id, &up_sql, "UP")?;
        }
    }

    Ok(())
}
