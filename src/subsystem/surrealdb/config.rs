use crate::config::DataSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SubsystemSurrealdb {
    pub connection: DataSource<String>,
    pub namespace: String,
    pub database: String,
    pub username: Option<DataSource<String>>,
    pub password: Option<DataSource<String>>,
    pub timeout: Option<u64>,
    pub tables: Tables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Tables {
    pub migrations: String,
    pub log: String,
}

impl Default for SubsystemSurrealdb {
    fn default() -> Self {
        Self {
            connection: DataSource::Static(String::new()),
            namespace: String::new(),
            database: String::new(),
            username: None,
            password: None,
            timeout: None,
            tables: Tables {
                migrations: "__qop_migrations".to_string(),
                log: "__qop_log".to_string(),
            },
        }
    }
}
