use semver::{Version, VersionReq};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct WithVersion {
    pub version: String,
}

impl WithVersion {
    pub fn validate(&self, cli: &str) -> Result<(), anyhow::Error> {
        // if cli == "0.0.0" {
        //     return Ok(());
        // }

        // Parse CLI version (Cargo semver)
        let cli_version = Version::parse(cli).map_err(|e| anyhow::anyhow!("Invalid CLI version '{}': {}", cli, e))?;

        // Parse version requirement from config (Cargo semver expressions)
        // Examples: ">=0.5.0, <0.6.0", "^0.5", "~0.5.2", "=0.5.3"
        let version_req = VersionReq::parse(&self.version)
            .map_err(|e| anyhow::anyhow!("Invalid version requirement '{}': {}", self.version, e))?;

        // Check if CLI version matches the specification
        if !version_req.matches(&cli_version) {
            return Err(anyhow::anyhow!(
                "Version mismatch: Config indicates required CLI version '{}', but current CLI version is '{}'",
                self.version,
                cli
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Config {
    pub version: String,
    pub subsystem: Subsystem,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(bound(serialize = "T: Serialize", deserialize = "T: DeserializeOwned"))]
pub enum DataSource<T: Serialize + DeserializeOwned> {
    Static(T),
    FromEnv(String),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsystem {
    #[cfg(feature = "sub+postgres")]
    Postgres(crate::subsystem::postgres::config::SubsystemPostgres),
    #[cfg(feature = "sub+surrealdb")]
    Surrealdb(crate::subsystem::surrealdb::config::SubsystemSurrealdb),
    #[cfg(feature = "sub+sqlite")]
    Sqlite(crate::subsystem::sqlite::config::SubsystemSqlite),
}
