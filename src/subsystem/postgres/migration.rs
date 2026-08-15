use std::io::{self, Write};
use {
    crate::config::{Config, DataSource, WithVersion},
    crate::subsystem::postgres::config::SubsystemPostgres,
    anyhow::{Context, Result},
    chrono::{NaiveDateTime, Utc},
    sqlx::postgres::PgPoolOptions,
    sqlx::{Pool, Postgres, QueryBuilder, Row, postgres::PgRow},
    std::{
        collections::{HashMap, HashSet},
        path::Path,
    },
};

// Database utility functions
pub(crate) fn get_effective_timeout(config: &SubsystemPostgres, provided_timeout: Option<u64>) -> Option<u64> {
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

pub(crate) fn build_table_query<'a>(base_sql: &'a str, schema: &str, table: &str) -> QueryBuilder<'a, Postgres> {
    let mut query = QueryBuilder::new(base_sql);
    query.push(quote_ident(schema));
    query.push(".");
    query.push(quote_ident(table));
    query
}

pub(crate) async fn set_timeout_if_needed<'e, E>(executor: E, timeout_seconds: Option<u64>) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    if let Some(seconds) = timeout_seconds {
        let ms: i64 = (seconds as i64) * 1000;
        sqlx::query("SET LOCAL statement_timeout = $1")
            .bind(ms)
            .execute(executor)
            .await?;
    }
    Ok(())
}

use crate::core::migration::prompt_for_confirmation_with_diff;

fn display_migration_diff_from_sql(_migration_id: &str, sql: &str, _direction: &str) -> Result<()> {
    crate::core::migration::display_sql_migration(_migration_id, sql, _direction)
}

fn create_bulk_migrations_diff_fn<'a>(
    migrations: &'a [String],
    migration_dir: &'a Path,
    direction: &'a str,
) -> impl Fn() -> Result<()> + 'a {
    move || -> Result<()> {
        for migration_id in migrations {
            let (up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;
            let sql = if direction == "UP" { up_sql } else { down_sql };

            display_migration_diff_from_sql(migration_id, &sql, direction)?;
        }
        Ok(())
    }
}

fn create_bulk_reverts_diff_fn<'a>(
    migrations: &'a [sqlx::postgres::PgRow],
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

            display_migration_diff_from_sql(&id, &down_sql, "DOWN")?;
        }
        Ok(())
    }
}

fn create_single_migration_diff_fn<'a>(
    migration_id: &'a str,
    sql: &'a str,
    direction: &'a str,
) -> impl Fn() -> Result<()> + 'a {
    move || -> Result<()> { display_migration_diff_from_sql(migration_id, sql, direction) }
}

pub(crate) async fn get_applied_migrations(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
) -> Result<HashSet<String>> {
    let mut query = build_table_query("SELECT id FROM ", schema, table);
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
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
) -> Result<Option<String>> {
    let mut query = build_table_query("SELECT id FROM ", schema, table);
    query.push(" ORDER BY id DESC LIMIT 1");
    Ok(query.build().fetch_optional(&mut **tx).await?.map(|row| row.get("id")))
}

pub(crate) async fn insert_migration_record<'e, E>(
    executor: E,
    schema: &str,
    table: &str,
    id: &str,
    up_sql: &str,
    down_sql: &str,
    comment: Option<&str>,
    pre_migration_id: Option<&str>,
    locked: bool,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let mut query = build_table_query("INSERT INTO ", schema, table);
    query.push(" (id, version, up, down, comment, pre, locked) VALUES ($1, $2, $3, $4, $5, $6, $7)");
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

pub(crate) async fn delete_migration_record<'e, E>(executor: E, schema: &str, table: &str, id: &str) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let mut query = build_table_query("DELETE FROM ", schema, table);
    query.push(" WHERE id = $1");
    query.build().bind(id).execute(executor).await?;
    Ok(())
}

pub(crate) async fn is_migration_locked<'e, E>(executor: E, schema: &str, table: &str, id: &str) -> Result<bool>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let mut query = build_table_query("SELECT locked FROM ", schema, table);
    query.push(" WHERE id = $1");
    let locked: Option<bool> = query
        .build()
        .bind(id)
        .fetch_optional(executor)
        .await?
        .map(|row| row.get("locked"));
    Ok(locked.unwrap_or(false))
}

pub(crate) async fn get_migration_history(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
) -> Result<HashMap<String, (NaiveDateTime, Option<String>, bool)>> {
    let mut query = build_table_query("SELECT id, created_at, comment, locked FROM ", schema, table);
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

pub(crate) async fn get_all_migration_data(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
) -> Result<Vec<PgRow>> {
    let mut query = build_table_query("SELECT id, up, down FROM ", schema, table);
    query.push(" ORDER BY id ASC");
    Ok(query.build().fetch_all(&mut **tx).await?)
}

pub(crate) use crate::core::migration::normalize_migration_id;

pub(crate) async fn get_recent_migrations_for_revert(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
) -> Result<Vec<PgRow>> {
    let mut query = build_table_query("SELECT id, down FROM ", schema, table);
    query.push(" ORDER BY id DESC");
    Ok(query.build().fetch_all(&mut **tx).await?)
}

pub(crate) async fn get_migration_down_sql(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    schema: &str,
    table: &str,
    migration_id: &str,
) -> Result<String> {
    let mut query = build_table_query("SELECT down FROM ", schema, table);
    query.push(" WHERE id = $1");
    let row = query.build().bind(migration_id).fetch_one(&mut **tx).await?;
    Ok(row.get("down"))
}

pub(crate) async fn get_table_version(tx: &mut sqlx::Transaction<'_, Postgres>, table: &str) -> Result<Option<String>> {
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
    tx: &mut sqlx::Transaction<'_, Postgres>,
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
    subsystem_config: &SubsystemPostgres,
    check_cli_version: bool,
) -> Result<Pool<Postgres>> {
    let uri = match &subsystem_config.connection {
        | DataSource::Static(connection) => connection.to_owned(),
        | DataSource::FromEnv(var) => std::env::var(var).with_context(|| {
            format!(
                "Missing environment variable '{}' referenced by [subsystem.postgres].connection in {}",
                var,
                path.display()
            )
        })?,
    };

    let pool = PgPoolOptions::new().max_connections(10).connect(&uri).await?;
    if check_cli_version {
        let mut tx = pool.begin().await?;
        let last_migration_version = get_table_version(&mut tx, &subsystem_config.tables.migrations).await?;
        if let Some(version) = last_migration_version {
            let cli_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
            if !(cli_version.major == 0 && cli_version.minor == 0 && cli_version.patch == 0) {
                let last_migration_version = semver::Version::parse(&version)?;
                if last_migration_version > cli_version {
                    anyhow::bail!(
                        "Latest migration table version is older than the CLI version. Please run 'qop subsystem postgres history fix' to rename out-of-order migrations."
                    );
                }
            }
        }
        tx.commit().await?;
    }
    Ok(pool)
}

pub(crate) use crate::core::migration::get_local_migrations;

// Log operations
pub(crate) async fn insert_log_entry<'c, E>(
    executor: E,
    schema: &str,
    log_table: &str,
    migration_id: &str,
    operation: &str,
    sql_command: &str,
) -> Result<()>
where
    E: sqlx::Executor<'c, Database = Postgres>,
{
    let log_id = uuid::Uuid::now_v7().to_string();
    let mut query = build_table_query("INSERT INTO ", schema, log_table);
    query.push(" (id, migration_id, operation, sql_command) VALUES ($1, $2, $3, $4)");
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
pub async fn init_with_pool(
    schema: &str,
    migrations_table: &str,
    log_table: &str,
    pool: &Pool<Postgres>,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    {
        // Create migrations table
        let mut query = build_table_query("CREATE TABLE IF NOT EXISTS ", schema, migrations_table);
        query.push(" (id VARCHAR PRIMARY KEY, version VARCHAR NOT NULL, up VARCHAR NOT NULL, down VARCHAR NOT NULL, created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, pre VARCHAR, comment VARCHAR, locked BOOLEAN NOT NULL DEFAULT FALSE)");
        query.build().execute(&mut *tx).await?;

        // Create log table
        let mut log_query = build_table_query("CREATE TABLE IF NOT EXISTS ", schema, log_table);
        log_query.push(" (id VARCHAR PRIMARY KEY, migration_id VARCHAR NOT NULL, operation VARCHAR NOT NULL, sql_command TEXT NOT NULL, executed_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)");
        log_query.build().execute(&mut *tx).await?;
    };
    tx.commit().await?;
    println!("Initialized migration tables.");
    Ok(())
}

pub async fn up(
    path: &Path,
    timeout: Option<u64>,
    count: Option<usize>,
    diff: bool,
    dry: bool,
    yes: bool,
) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Postgres(c) => c,
        | _ => anyhow::bail!("expected postgres config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let schema = &config.schema;
    let migrations_table = &config.tables.migrations;

    let mut tx = pool.begin().await?;

    set_timeout_if_needed(&mut *tx, effective_timeout).await?;

    let applied_migrations = get_applied_migrations(&mut tx, &schema, &migrations_table).await?;
    let mut last_migration_id = get_last_migration_id(&mut tx, &schema, &migrations_table).await?;

    // Commit the initial query transaction
    tx.commit().await?;

    let mut migrations_to_apply: Vec<String> = local_migrations.difference(&applied_migrations).cloned().collect();

    migrations_to_apply.sort();

    let migrations_to_apply = if let Some(count) = count {
        migrations_to_apply.into_iter().take(count).collect()
    } else {
        migrations_to_apply
    };

    // Linear history enforcement: Check for out-of-order migrations
    if !applied_migrations.is_empty() && !migrations_to_apply.is_empty() {
        let max_applied_migration = applied_migrations.iter().max().cloned().unwrap_or_default();

        let out_of_order_migrations: Vec<&String> = migrations_to_apply
            .iter()
            .filter(|id| id.as_str() < max_applied_migration.as_str())
            .collect();

        if !out_of_order_migrations.is_empty() {
            println!("⚠️  Non-linear history detected!");
            println!("The following migrations would create a non-linear history:");
            for migration in &out_of_order_migrations {
                println!("  - {}", migration);
            }
            println!("Latest applied migration: {}", max_applied_migration);
            println!();
            println!("This could cause issues with database schema consistency.");
            println!("Alternatively, you can run 'qop migration history fix' to rename out-of-order migrations.");

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
    }

    if migrations_to_apply.is_empty() {
        println!("All migrations are up to date.");
    } else {
        // Show diff preview if --diff flag is specified
        if diff {
            for migration_id in &migrations_to_apply {
                let (up_sql, _down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;
                print!("{}", up_sql);
            }

            // Ask for confirmation when showing diff
            print!("\n❓ Do you want to apply these migrations? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();

            if input != "y" && input != "yes" {
                println!("❌ Migration cancelled.");
                return Ok(());
            }

            if dry {
                println!("\n🧪 Running migrations in dry-run mode...");
            } else {
                println!("\n🚀 Applying migrations...");
            }
        } else if dry {
            println!("\n🧪 Running migrations in dry-run mode...");
        } else {
            // Prompt for confirmation when not using diff and not in silent mode
            println!("\n📋 About to apply {} migration(s):", migrations_to_apply.len());
            for migration_id in &migrations_to_apply {
                println!("  - {}", migration_id);
            }

            let diff_fn = create_bulk_migrations_diff_fn(&migrations_to_apply, migration_dir, "UP");

            if !prompt_for_confirmation_with_diff(
                "❓ Do you want to proceed with applying these migrations?",
                yes,
                diff_fn,
            )? {
                println!("❌ Migration cancelled.");
                return Ok(());
            }

            println!("\n🚀 Applying migrations...");
        }

        // Apply each migration in its own transaction
        for migration_id in &migrations_to_apply {
            if dry {
                println!("⏳ Testing migration: {}", migration_id);
            } else {
                println!("⏳ Applying migration: {}", migration_id);
            }
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
                &schema,
                &migrations_table,
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
            }
            if !dry {
                last_migration_id = Some(id.to_string());
            }
        }

        if dry {
            println!(
                "\n🎉 Successfully executed {} migration(s) in dry-run mode! (No changes were committed)",
                migrations_to_apply.len()
            );
        } else {
            println!("\n🎉 Successfully applied {} migration(s)!", migrations_to_apply.len());
        }
    }

    Ok(())
}

pub async fn down(
    path: &Path,
    timeout: Option<u64>,
    count: Option<usize>,
    remote: bool,
    diff: bool,
    dry: bool,
    yes: bool,
) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Postgres(c) => c,
        | _ => anyhow::bail!("expected postgres config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let schema = &config.schema;
    let migrations_table = &config.tables.migrations;

    let mut tx = pool.begin().await?;

    set_timeout_if_needed(&mut *tx, effective_timeout).await?;

    let last_migrations = get_recent_migrations_for_revert(&mut tx, &schema, &migrations_table).await?;

    let migrations_to_revert: Vec<PgRow> = if let Some(count) = count {
        last_migrations.into_iter().take(count).collect()
    } else {
        last_migrations.into_iter().take(1).collect()
    };

    // Commit the initial query transaction
    tx.commit().await?;

    if migrations_to_revert.is_empty() {
        println!("No migrations to revert.");
    } else {
        // Show diff preview if --diff flag is specified
        if diff {
            for row in &migrations_to_revert {
                let id: String = row.get("id");
                let down_sql: String = if remote {
                    row.get("down")
                } else {
                    let down_sql_path = migration_dir.join(&id).join("down.sql");
                    std::fs::read_to_string(&down_sql_path)
                        .with_context(|| format!("Failed to read down migration: {}", down_sql_path.display()))?
                };

                print!("{}", down_sql);
            }

            // Ask for confirmation when showing diff
            print!("\n❓ Do you want to revert these migrations? [y/N]: ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let input = input.trim().to_lowercase();

            if input != "y" && input != "yes" {
                println!("❌ Revert cancelled.");
                return Ok(());
            }

            println!("\n🔄 Reverting migrations...");
        } else {
            // Prompt for confirmation when not using diff and not in silent mode
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

            println!("\n🔄 Reverting migrations...");
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
            delete_migration_record(&mut *revert_tx, &schema, &migrations_table, &id).await?;

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

// Note: This function is deprecated - use the core migration creation service instead
// which goes through util::create_migration_directory()
pub async fn new_migration(path: &Path) -> Result<()> {
    crate::core::migration::create_migration_directory(path, None, false)?;
    Ok(())
}

pub async fn apply_up(path: &Path, id: &str, timeout: Option<u64>, dry: bool, yes: bool) -> Result<()> {
    let config_content = std::fs::read_to_string(path)?;
    let with_version: WithVersion = toml::from_str(&config_content)?;
    with_version.validate(env!("CARGO_PKG_VERSION"))?;
    let cfg: Config = toml::from_str(&config_content)?;
    #[allow(unreachable_patterns)]
    let config = match cfg.subsystem {
        | crate::config::Subsystem::Postgres(c) => c,
        | _ => anyhow::bail!("expected postgres config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;
    let schema = &config.schema;
    let migrations_table = &config.tables.migrations;

    // Normalize the migration ID to remove "id=" prefix if present
    let target_migration_id = normalize_migration_id(&id);

    let mut tx = pool.begin().await?;

    // Get current applied migrations
    let applied_migrations = get_applied_migrations(&mut tx, &schema, &migrations_table).await?;

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

    // Apply the migration (read via helper to ensure `id=` directory convention)
    let (up_sql, down_sql) = crate::core::migration::read_migration_files(migration_dir, &target_migration_id)?;
    // Confirm migration application
    let diff_fn = create_single_migration_diff_fn(&target_migration_id, &up_sql, "UP");

    if !prompt_for_confirmation_with_diff(
        &format!("❓ Do you want to apply migration '{}'?", target_migration_id),
        yes,
        diff_fn,
    )? {
        println!("❌ Operation cancelled.");
        return Ok(());
    }

    // Both up and down SQL already read above

    // Get the latest migration for the pre field
    let mut tx = pool.begin().await?;
    let last_migration_id = get_last_migration_id(&mut tx, &schema, &migrations_table).await?;
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
        &schema,
        &migrations_table,
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
        | crate::config::Subsystem::Postgres(c) => c,
        | _ => anyhow::bail!("expected postgres config"),
    };
    let pool = build_pool_from_config(path, &config, true).await?;
    let effective_timeout = get_effective_timeout(&config, timeout);
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let schema = &config.schema;
    let migrations_table = &config.tables.migrations;

    // Normalize the migration ID to remove "id=" prefix if present
    let target_migration_id = normalize_migration_id(&id);

    let mut tx = pool.begin().await?;

    // Get current applied migrations
    let applied_migrations = get_applied_migrations(&mut tx, &schema, &migrations_table).await?;

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
        let mut tx = pool.begin().await?;
        let sql = get_migration_down_sql(&mut tx, &schema, &migrations_table, &target_migration_id).await?;
        tx.commit().await?;
        sql
    } else {
        let down_sql_path = migration_dir.join(&target_migration_id).join("down.sql");
        std::fs::read_to_string(&down_sql_path)
            .with_context(|| format!("Failed to read down migration: {}", down_sql_path.display()))?
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

    delete_migration_record(&mut *revert_tx, &schema, &migrations_table, &target_migration_id).await?;

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

pub async fn list(path: &Path, schema: &str, migrations_table: &str, pool: &Pool<Postgres>) -> Result<()> {
    let local_migrations = get_local_migrations(path)?;
    let schema = schema;

    let mut tx = pool.begin().await?;

    let applied_migrations = get_migration_history(&mut tx, &schema, &migrations_table).await?;
    let mut remote: Vec<(String, chrono::NaiveDateTime, Option<String>, bool)> = applied_migrations
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

pub async fn history_fix(path: &Path, schema: &str, migrations_table: &str, pool: &Pool<Postgres>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;
    let schema = schema;

    let mut tx = pool.begin().await?;

    let applied_migrations = get_applied_migrations(&mut tx, &schema, &migrations_table).await?;

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

pub async fn history_sync(path: &Path, schema: &str, migrations_table: &str, pool: &Pool<Postgres>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let schema = schema;

    let mut tx = pool.begin().await?;

    let all_migrations = get_all_migration_data(&mut tx, &schema, &migrations_table).await?;

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

pub async fn diff(path: &Path, schema: &str, migrations_table: &str, pool: &Pool<Postgres>) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = get_local_migrations(path)?;
    let schema = schema;

    let mut tx = pool.begin().await?;

    let applied_migrations = get_applied_migrations(&mut tx, &schema, &migrations_table).await?;

    tx.commit().await?;

    let mut migrations_to_apply: Vec<String> = local_migrations.difference(&applied_migrations).cloned().collect();

    migrations_to_apply.sort();

    if migrations_to_apply.is_empty() {
        println!("All migrations are up to date.");
    } else {
        for migration_id in &migrations_to_apply {
            let (up_sql, _down_sql) = crate::core::migration::read_migration_files(migration_dir, migration_id)?;
            // Render with same formatting as interactive 'd'
            crate::core::migration::display_sql_migration(migration_id, &up_sql, "UP")?;
        }
    }

    Ok(())
}
