pub mod commands;
pub mod config;
pub mod migration;
pub mod repo;

#[cfg(feature = "sub+postgres")]
use crate::config::{Config, DataSource, Subsystem};
#[cfg(feature = "sub+postgres")]
use crate::subsystem::postgres::config::SubsystemPostgres;

#[cfg(feature = "sub+postgres")]
pub fn build_sample(connection: &str) -> crate::config::Config {
    use crate::subsystem::postgres::config::Tables;

    Config {
        version: env!("CARGO_PKG_VERSION").to_string(),
        subsystem: Subsystem::Postgres(SubsystemPostgres {
            connection: DataSource::Static(connection.to_string()),
            timeout: Some(60),
            tables: Tables {
                migrations: "__qop_migrations".to_string(),
                log: "__qop_log".to_string(),
            },
            schema: "public".to_string(),
        }),
    }
}
