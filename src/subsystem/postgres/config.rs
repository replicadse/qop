use crate::config::DataSource;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SubsystemPostgres {
    pub connection: DataSource<String>,
    pub timeout: Option<u64>,
    pub schema: String,
    pub tables: Tables,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Tables {
    pub migrations: String,
    pub log: String,
}

impl Default for SubsystemPostgres {
    fn default() -> Self {
        Self {
            connection: DataSource::Static(String::new()),
            timeout: None,
            schema: "public".to_string(),
            tables: Tables {
                migrations: "__qop_migrations".to_string(),
                log: "__qop_log".to_string(),
            },
        }
    }
}
