pub mod commands;
pub mod config;
pub mod migration;
pub mod repo;

use crate::config::{Config, DataSource, Subsystem};
use crate::subsystem::surrealdb::config::SubsystemSurrealdb;

pub fn build_sample(
    connection: &str,
    namespace: &str,
    database: &str,
    username: Option<&str>,
    password: Option<&str>,
) -> crate::config::Config {
    use crate::subsystem::surrealdb::config::Tables;

    Config {
        version: env!("CARGO_PKG_VERSION").to_string(),
        subsystem: Subsystem::Surrealdb(SubsystemSurrealdb {
            connection: DataSource::Static(connection.to_string()),
            namespace: namespace.to_string(),
            database: database.to_string(),
            username: username.map(|value| DataSource::Static(value.to_string())),
            password: password.map(|value| DataSource::Static(value.to_string())),
            timeout: Some(60),
            tables: Tables {
                migrations: "__qop_migrations".to_string(),
                log: "__qop_log".to_string(),
            },
        }),
    }
}
