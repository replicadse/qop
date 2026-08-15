use {
    crate::config::DataSource,
    crate::subsystem::surrealdb::config::SubsystemSurrealdb,
    anyhow::{Context, Result},
    chrono::{DateTime, NaiveDateTime, Utc},
    serde::{Deserialize, de::DeserializeOwned},
    serde_json::Value,
    std::{collections::HashSet, path::Path, time::Duration},
};

#[derive(Debug, Deserialize)]
struct SurrealdbStatement {
    status: String,
    result: Value,
}

#[derive(Clone)]
pub struct SurrealdbClient {
    client: reqwest::Client,
    endpoint: String,
    namespace: String,
    database: String,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MigrationIdRow {
    migration_id: String,
}

#[derive(Debug, Deserialize)]
struct VersionRow {
    version: String,
}

#[derive(Debug, Deserialize)]
struct LockedRow {
    #[serde(default)]
    locked: bool,
}

#[derive(Debug, Deserialize)]
struct DownRow {
    migration_id: String,
    down: String,
}

#[derive(Debug, Deserialize)]
struct HistoryRow {
    migration_id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    comment: Option<String>,
    #[serde(default)]
    locked: bool,
}

#[derive(Debug, Deserialize)]
struct MigrationDataRow {
    migration_id: String,
    up: String,
    down: String,
    #[serde(default)]
    comment: Option<String>,
}

impl SurrealdbClient {
    fn from_config(path: &Path, config: &SubsystemSurrealdb) -> Result<Self> {
        let connection = resolve_data_source(path, &config.connection, "connection")?;
        if connection.is_empty() {
            anyhow::bail!("SurrealDB connection cannot be empty");
        }
        if config.namespace.is_empty() {
            anyhow::bail!("SurrealDB namespace cannot be empty");
        }
        if config.database.is_empty() {
            anyhow::bail!("SurrealDB database cannot be empty");
        }

        let username = resolve_optional_data_source(path, config.username.as_ref(), "username")?;
        let password = resolve_optional_data_source(path, config.password.as_ref(), "password")?;
        if username.is_some() != password.is_some() {
            anyhow::bail!("SurrealDB username and password must be configured together");
        }

        Ok(Self {
            client: reqwest::Client::new(),
            endpoint: sql_endpoint(&connection),
            namespace: config.namespace.clone(),
            database: config.database.clone(),
            username,
            password,
        })
    }

    async fn execute(&self, sql: &str, timeout_seconds: Option<u64>) -> Result<Vec<SurrealdbStatement>> {
        let mut request = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .header("Content-Type", "application/surrealql")
            .header("Surreal-NS", &self.namespace)
            .header("Surreal-DB", &self.database)
            .body(sql.to_string());

        if let (Some(username), Some(password)) = (&self.username, &self.password) {
            request = request.basic_auth(username, Some(password));
        }

        if let Some(seconds) = timeout_seconds {
            request = request.timeout(Duration::from_secs(seconds));
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to send SurrealDB request to {}", self.endpoint))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .context("Failed to read SurrealDB response body")?;

        if !status.is_success() {
            anyhow::bail!("SurrealDB request failed ({}): {}", status, body);
        }

        let statements: Vec<SurrealdbStatement> =
            serde_json::from_str(&body).with_context(|| format!("Failed to parse SurrealDB response: {}", body))?;
        for (index, statement) in statements.iter().enumerate() {
            if !statement.status.eq_ignore_ascii_case("OK") {
                anyhow::bail!(
                    "SurrealDB statement {} failed: {}",
                    index + 1,
                    display_result(&statement.result)
                );
            }
        }

        Ok(statements)
    }

    async fn query_rows<T>(&self, sql: &str, timeout_seconds: Option<u64>) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let statements = self.execute(sql, timeout_seconds).await?;
        let statement = statements
            .last()
            .ok_or_else(|| anyhow::anyhow!("SurrealDB returned no statement results"))?;
        serde_json::from_value(statement.result.clone())
            .with_context(|| format!("Failed to decode SurrealDB result: {}", statement.result))
    }
}

pub(crate) async fn build_client_from_config(
    path: &Path,
    config: &SubsystemSurrealdb,
    check_cli_version: bool,
) -> Result<SurrealdbClient> {
    let client = SurrealdbClient::from_config(path, config)?;
    if check_cli_version {
        if let Some(version) = get_table_version(&client, &config.tables.migrations, config.timeout).await? {
            let cli_version = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
            if !(cli_version.major == 0 && cli_version.minor == 0 && cli_version.patch == 0) {
                let last_migration_version = semver::Version::parse(&version)?;
                if last_migration_version > cli_version {
                    anyhow::bail!(
                        "Latest migration table version is newer than the CLI version. Please upgrade qop before continuing."
                    );
                }
            }
        }
    }
    Ok(client)
}

pub(crate) async fn init_store(client: &SurrealdbClient, config: &SubsystemSurrealdb) -> Result<()> {
    let migrations_table = table_ident(&config.tables.migrations)?;
    let log_table = table_ident(&config.tables.log)?;
    let sql = format!(
        "DEFINE TABLE IF NOT EXISTS {} SCHEMALESS;\nDEFINE TABLE IF NOT EXISTS {} SCHEMALESS;",
        migrations_table, log_table
    );
    client.execute(&sql, config.timeout).await?;
    println!("Initialized migration tables.");
    Ok(())
}

pub(crate) async fn get_applied_migrations(
    client: &SurrealdbClient,
    table: &str,
    timeout: Option<u64>,
) -> Result<HashSet<String>> {
    let table = table_ident(table)?;
    let rows: Vec<MigrationIdRow> = client
        .query_rows(
            &format!("SELECT migration_id FROM {} ORDER BY migration_id ASC;", table),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().map(|row| row.migration_id).collect())
}

pub(crate) async fn get_last_migration_id(
    client: &SurrealdbClient,
    table: &str,
    timeout: Option<u64>,
) -> Result<Option<String>> {
    let table = table_ident(table)?;
    let rows: Vec<MigrationIdRow> = client
        .query_rows(
            &format!("SELECT migration_id FROM {} ORDER BY migration_id DESC LIMIT 1;", table),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().next().map(|row| row.migration_id))
}

pub(crate) async fn apply_migration(
    client: &SurrealdbClient,
    config: &SubsystemSurrealdb,
    id: &str,
    up_sql: &str,
    down_sql: &str,
    comment: Option<&str>,
    pre_migration_id: Option<&str>,
    timeout: Option<u64>,
    dry_run: bool,
    locked: bool,
) -> Result<()> {
    let timeout = get_effective_timeout(config, timeout);
    let mut sql = String::from("BEGIN TRANSACTION;\n");
    push_migration_sql(&mut sql, up_sql);
    sql.push_str(&migration_record_statement(
        &config.tables.migrations,
        id,
        up_sql,
        down_sql,
        comment,
        pre_migration_id,
        locked,
    )?);
    sql.push_str(&log_record_statement(&config.tables.log, id, "up", up_sql)?);
    if dry_run {
        sql.push_str("CANCEL TRANSACTION;");
    } else {
        sql.push_str("COMMIT TRANSACTION;");
    }

    client
        .execute(&sql, timeout)
        .await
        .with_context(|| format!("Failed to apply SurrealDB migration {}", id))?;
    Ok(())
}

pub(crate) async fn revert_migration(
    client: &SurrealdbClient,
    config: &SubsystemSurrealdb,
    id: &str,
    down_sql: &str,
    timeout: Option<u64>,
    dry_run: bool,
    unlock: bool,
) -> Result<()> {
    let timeout = get_effective_timeout(config, timeout);
    if is_migration_locked(client, &config.tables.migrations, id, timeout).await? && !unlock {
        anyhow::bail!(
            "Migration {} is locked and cannot be reverted without --unlock flag",
            id
        );
    }

    let mut sql = String::from("BEGIN TRANSACTION;\n");
    push_migration_sql(&mut sql, down_sql);
    sql.push_str(&format!("DELETE {};\n", thing_expr(&config.tables.migrations, id)?));
    sql.push_str(&log_record_statement(&config.tables.log, id, "down", down_sql)?);
    if dry_run {
        sql.push_str("CANCEL TRANSACTION;");
    } else {
        sql.push_str("COMMIT TRANSACTION;");
    }

    client
        .execute(&sql, timeout)
        .await
        .with_context(|| format!("Failed to revert SurrealDB migration {}", id))?;
    Ok(())
}

pub(crate) async fn get_migration_history(
    client: &SurrealdbClient,
    table: &str,
    timeout: Option<u64>,
) -> Result<Vec<(String, NaiveDateTime, Option<String>, bool)>> {
    let table = table_ident(table)?;
    let rows: Vec<HistoryRow> = client
        .query_rows(
            &format!(
                "SELECT migration_id, created_at, comment, locked FROM {} ORDER BY migration_id ASC;",
                table
            ),
            timeout,
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.migration_id, row.created_at.naive_utc(), row.comment, row.locked))
        .collect())
}

pub(crate) async fn get_recent_migrations_for_revert(
    client: &SurrealdbClient,
    table: &str,
    timeout: Option<u64>,
) -> Result<Vec<(String, String)>> {
    let table = table_ident(table)?;
    let rows: Vec<DownRow> = client
        .query_rows(
            &format!("SELECT migration_id, down FROM {} ORDER BY migration_id DESC;", table),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().map(|row| (row.migration_id, row.down)).collect())
}

pub(crate) async fn get_migration_down_sql(
    client: &SurrealdbClient,
    table: &str,
    migration_id: &str,
    timeout: Option<u64>,
) -> Result<Option<String>> {
    let rows: Vec<DownRow> = client
        .query_rows(
            &format!("SELECT migration_id, down FROM {};", thing_expr(table, migration_id)?),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().next().map(|row| row.down))
}

pub(crate) async fn get_all_migration_data(
    client: &SurrealdbClient,
    table: &str,
    timeout: Option<u64>,
) -> Result<Vec<(String, String, String, Option<String>)>> {
    let table = table_ident(table)?;
    let rows: Vec<MigrationDataRow> = client
        .query_rows(
            &format!(
                "SELECT migration_id, up, down, comment FROM {} ORDER BY migration_id ASC;",
                table
            ),
            timeout,
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.migration_id, row.up, row.down, row.comment))
        .collect())
}

pub async fn history_fix(path: &Path, config: &SubsystemSurrealdb, client: &SurrealdbClient) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = crate::core::migration::get_local_migrations(path)?;
    let applied_migrations = get_applied_migrations(client, &config.tables.migrations, config.timeout).await?;

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

    Ok(())
}

pub async fn history_sync(path: &Path, config: &SubsystemSurrealdb, client: &SurrealdbClient) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let all_migrations = get_all_migration_data(client, &config.tables.migrations, config.timeout).await?;

    if all_migrations.is_empty() {
        println!("No migrations to sync.");
    } else {
        for (id, up_sql, down_sql, _comment) in all_migrations {
            let migration_id_path = migration_dir.join(format!("id={}", id));
            std::fs::create_dir_all(&migration_id_path)
                .with_context(|| format!("Failed to create directory: {}", migration_id_path.display()))?;

            let up_path = migration_id_path.join("up.surql");
            let down_path = migration_id_path.join("down.surql");

            std::fs::write(&up_path, up_sql)
                .with_context(|| format!("Failed to write up migration: {}", up_path.display()))?;
            std::fs::write(&down_path, down_sql)
                .with_context(|| format!("Failed to write down migration: {}", down_path.display()))?;

            println!("Synced migration: {}", id);
        }
    }

    Ok(())
}

pub async fn diff(path: &Path, config: &SubsystemSurrealdb, client: &SurrealdbClient) -> Result<()> {
    let migration_dir = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid migration path: {}", path.display()))?;
    let local_migrations = crate::core::migration::get_local_migrations(path)?;
    let applied_migrations = get_applied_migrations(client, &config.tables.migrations, config.timeout).await?;
    let mut pending_migrations: Vec<String> = local_migrations.difference(&applied_migrations).cloned().collect();

    pending_migrations.sort();

    if pending_migrations.is_empty() {
        println!("All migrations are up to date.");
    } else {
        for migration_id in &pending_migrations {
            let (up_sql, _down_sql) = crate::core::migration::read_migration_files_with_extension(
                migration_dir,
                migration_id,
                "surql",
            )?;
            crate::core::migration::display_sql_migration(migration_id, &up_sql, "UP")?;
        }
    }

    Ok(())
}

fn get_effective_timeout(config: &SubsystemSurrealdb, provided_timeout: Option<u64>) -> Option<u64> {
    provided_timeout.or(config.timeout)
}

async fn get_table_version(client: &SurrealdbClient, table: &str, timeout: Option<u64>) -> Result<Option<String>> {
    let table = table_ident(table)?;
    let rows: Vec<VersionRow> = client
        .query_rows(
            &format!(
                "SELECT version, migration_id FROM {} ORDER BY migration_id DESC LIMIT 1;",
                table
            ),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().next().map(|row| row.version))
}

async fn is_migration_locked(
    client: &SurrealdbClient,
    table: &str,
    migration_id: &str,
    timeout: Option<u64>,
) -> Result<bool> {
    let rows: Vec<LockedRow> = client
        .query_rows(
            &format!("SELECT locked FROM {};", thing_expr(table, migration_id)?),
            timeout,
        )
        .await?;
    Ok(rows.into_iter().next().map(|row| row.locked).unwrap_or(false))
}

fn migration_record_statement(
    table: &str,
    id: &str,
    up_sql: &str,
    down_sql: &str,
    comment: Option<&str>,
    pre_migration_id: Option<&str>,
    locked: bool,
) -> Result<String> {
    Ok(format!(
        "CREATE {} CONTENT {{ migration_id: {}, version: {}, up: {}, down: {}, created_at: time::now(), pre: {}, comment: {}, locked: {} }};\n",
        thing_expr(table, id)?,
        string_literal(id)?,
        string_literal(env!("CARGO_PKG_VERSION"))?,
        string_literal(up_sql)?,
        string_literal(down_sql)?,
        optional_string_literal(pre_migration_id)?,
        optional_string_literal(comment)?,
        locked,
    ))
}

fn log_record_statement(table: &str, migration_id: &str, operation: &str, sql_command: &str) -> Result<String> {
    let log_id = uuid::Uuid::now_v7().to_string();
    Ok(format!(
        "CREATE {} CONTENT {{ log_id: {}, migration_id: {}, operation: {}, query: {}, executed_at: time::now() }};\n",
        thing_expr(table, &log_id)?,
        string_literal(&log_id)?,
        string_literal(migration_id)?,
        string_literal(operation)?,
        string_literal(sql_command)?,
    ))
}

fn push_migration_sql(output: &mut String, sql: &str) {
    output.push_str(sql);
    if !sql.ends_with('\n') {
        output.push('\n');
    }
}

fn resolve_data_source(path: &Path, source: &DataSource<String>, field: &str) -> Result<String> {
    match source {
        | DataSource::Static(value) => Ok(value.clone()),
        | DataSource::FromEnv(var) => std::env::var(var).with_context(|| {
            format!(
                "Missing environment variable '{}' referenced by [subsystem.surrealdb].{} in {}",
                var,
                field,
                path.display()
            )
        }),
    }
}

fn resolve_optional_data_source(
    path: &Path,
    source: Option<&DataSource<String>>,
    field: &str,
) -> Result<Option<String>> {
    source
        .map(|source| resolve_data_source(path, source, field))
        .transpose()
}

fn sql_endpoint(connection: &str) -> String {
    let connection = connection.trim_end_matches('/');
    if connection.ends_with("/sql") {
        connection.to_string()
    } else {
        format!("{}/sql", connection)
    }
}

fn table_ident(table: &str) -> Result<&str> {
    let mut chars = table.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("SurrealDB table name cannot be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_') || !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        anyhow::bail!(
            "Invalid SurrealDB table name '{}'. Use ASCII letters, numbers, and underscores, starting with a letter or underscore.",
            table
        );
    }
    Ok(table)
}

fn thing_expr(table: &str, id: &str) -> Result<String> {
    table_ident(table)?;
    Ok(format!(
        "type::record({}, {})",
        string_literal(table)?,
        string_literal(id)?
    ))
}

fn string_literal(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn optional_string_literal(value: Option<&str>) -> Result<String> {
    match value {
        | Some(value) => string_literal(value),
        | None => Ok("NULL".to_string()),
    }
}

fn display_result(value: &Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}
