pub mod commands;
pub mod config;
pub mod migration;
#[cfg(feature = "sub+sqlite")]
pub mod repo;

#[cfg(feature = "sub+sqlite")]
use crate::config::{Config, DataSource, Subsystem};
#[cfg(feature = "sub+sqlite")]
use crate::subsystem::sqlite::config::SubsystemSqlite;

#[cfg(feature = "sub+sqlite")]
pub fn build_sample_with_db_path(db_path: &std::path::Path) -> crate::config::Config {
    use crate::subsystem::sqlite::config::Tables;

    Config {
        version: env!("CARGO_PKG_VERSION").to_string(),
        subsystem: Subsystem::Sqlite(SubsystemSqlite {
            connection: DataSource::Static(db_path.to_string_lossy().to_string()),
            timeout: Some(60),
            tables: Tables {
                migrations: "__qop_migrations".to_string(),
                log: "__qop_log".to_string(),
            },
        }),
    }
}
