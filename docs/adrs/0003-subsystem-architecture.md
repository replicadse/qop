# ADR-0003: Pluggable Subsystem Architecture

## Status

Accepted

## Date

2025-01-27T22:35:00Z

## Context

The `qop` migration tool needs to support multiple database backends (PostgreSQL, SQLite, SurrealDB) with potentially more backends added in the future. Each database has unique connection patterns, query dialects, and operational characteristics, but the core migration logic (applying migrations, tracking history, etc.) should remain consistent across all backends.

A pluggable architecture is needed that allows for:
1. Adding new database backends without modifying core logic
2. Sharing common migration operations across all backends
3. Database-specific optimizations and features
4. Clean separation of concerns between database-specific and generic code

## Decision

The codebase MUST implement a pluggable subsystem architecture where each database backend is implemented as a separate subsystem following standardized interfaces and patterns.

### Architecture Components

1. **Core Layer**: Database-agnostic business logic (`core::service`, `core::migration`)
2. **Repository Layer**: Database-specific implementations behind a common trait (`core::repo::MigrationRepository`)
3. **Subsystem Layer**: Complete backend implementations (`subsystem::postgres`, `subsystem::sqlite`, `subsystem::surrealdb`)
4. **Driver Layer**: Subsystem dispatch and coordination (`subsystem::driver`)

### Implementation Requirements

1. **Repository Pattern**: Each subsystem MUST implement `MigrationRepository` trait
2. **Service Pattern**: Core business logic MUST be database-agnostic via `MigrationService<R>`
3. **Configuration Isolation**: Each subsystem MUST have its own configuration structure
4. **Command Isolation**: Each subsystem MUST define its own command structures
5. **Migration Utilities**: Database-specific migration logic MUST be contained within subsystem modules

### Subsystem Structure

Each subsystem MUST follow this module structure:
```
subsystem/
├── mod.rs              # Module exports and prelude
├── driver.rs           # Dispatch coordination
├── <backend>/
│   ├── mod.rs          # Backend module
│   ├── config.rs       # Backend-specific configuration
│   ├── commands.rs     # Backend-specific CLI commands
│   ├── repo.rs         # MigrationRepository implementation
│   └── migration.rs    # Backend-specific migration utilities
```

### Interface Contracts

```rust
#[async_trait::async_trait(?Send)]
pub trait MigrationRepository {
    async fn init_store(&self) -> Result<()>;
    async fn fetch_applied_ids(&self) -> Result<HashSet<String>>;
    async fn apply_migration(&self, ...) -> Result<()>;
    async fn revert_migration(&self, ...) -> Result<()>;
    // ... other operations
    fn get_path(&self) -> &Path;
}
```

## Consequences

### Positive

- **Extensibility**: New database backends can be added without modifying existing code
- **Maintainability**: Database-specific logic is isolated and clearly organized
- **Reusability**: Core migration logic is shared across all backends
- **Testability**: Each subsystem can be tested independently
- **Single Responsibility**: Clear separation between business logic and database operations
- **Consistency**: All backends provide the same set of operations through common interfaces

### Negative

- **Abstraction Overhead**: Additional layers of indirection may impact performance
- **Code Duplication**: Some patterns are duplicated across subsystem implementations
- **Learning Curve**: Developers need to understand the multi-layer architecture
- **Interface Evolution**: Changes to core interfaces affect all subsystem implementations

## Implementation

### Repository Implementations

Each backend MUST implement `MigrationRepository`:
```rust
pub struct PostgresRepo {
    pub config: SubsystemPostgres,
    pub pool: Pool<Postgres>,
    pub path: PathBuf,
}

#[async_trait::async_trait(?Send)]
impl MigrationRepository for PostgresRepo {
    // Implementation details...
}
```

### Service Layer Usage

Core business logic MUST use the generic service:
```rust
pub struct MigrationService<R: MigrationRepository> {
    repo: R,
}

impl<R: MigrationRepository> MigrationService<R> {
    pub async fn up(&self, ...) -> Result<()> {
        // Database-agnostic migration logic
    }
}
```

### Dispatch Pattern

Subsystem dispatch MUST handle feature-conditional routing:
```rust
pub async fn dispatch(subsystem: Subsystem) -> Result<()> {
    match subsystem {
        #[cfg(feature = "sub+postgres")]
        Subsystem::Postgres { path, config, command } => {
            let repo = PostgresRepo::from_config(&path, config, true).await?;
            let svc = MigrationService::new(repo);
            // Handle postgres-specific commands...
        }
        #[cfg(feature = "sub+sqlite")]
        Subsystem::Sqlite { path, config, command } => {
            let repo = SqliteRepo::from_config(&path, config, true).await?;
            let svc = MigrationService::new(repo);
            // Handle sqlite-specific commands...
        }
    }
}
```

## Guidelines

1. **Backend Addition**: New backends MUST follow the established subsystem structure
2. **Interface Changes**: Modifications to `MigrationRepository` MUST be backward-compatible or require version bumps
3. **Database-Specific Logic**: MUST be contained within the appropriate subsystem module
4. **Error Handling**: MUST use consistent error patterns across all subsystems
5. **Configuration**: Each subsystem MUST provide sensible defaults and validation

## References

- [Repository Pattern](https://martinfowler.com/eaaCatalog/repository.html)
- [Service Layer Pattern](https://martinfowler.com/eaaCatalog/serviceLayer.html)
- [Rust async-trait documentation](https://docs.rs/async-trait/)
