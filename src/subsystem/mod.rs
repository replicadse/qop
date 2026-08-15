#[cfg(not(any(feature = "sub+postgres", feature = "sub+sqlite", feature = "sub+surrealdb")))]
compile_error!("At least one subsystem feature must be enabled: 'postgres', 'sqlite', or 'surrealdb'.");

pub mod driver;
#[cfg(feature = "sub+postgres")]
pub mod postgres;
#[cfg(feature = "sub+sqlite")]
pub mod sqlite;
#[cfg(feature = "sub+surrealdb")]
pub mod surrealdb;
pub mod prelude {
    pub use crate::core::{repo::MigrationRepository, service::MigrationService};
}
