# ADR-0002: Feature-Based Conditional Compilation

## Status

Accepted

## Date

2025-01-27T22:30:00Z

## Context

The `qop` migration tool supports multiple database backends (PostgreSQL, SQLite, and SurrealDB), but users typically only need one or two backends in their applications. Including all backends by default would increase binary size and introduce unnecessary dependencies. Additionally, some environments may have restrictions on which database drivers can be included.

Rust's feature system allows conditional compilation based on feature flags, enabling users to include only the database backends they need while maintaining a clean, modular codebase.

## Decision

All subsystem-specific code MUST be conditionally compiled using Cargo feature flags with the pattern `sub+<backend>` (e.g., `sub+postgres`, `sub+sqlite`, `sub+surrealdb`).

### Implementation Requirements

1. **Feature Flag Naming**: All database subsystem features MUST use the `sub+<backend>` naming convention
2. **Compile-Time Validation**: The codebase MUST fail to compile if no subsystem features are enabled
3. **Code Organization**: Subsystem-specific modules and implementations MUST be guarded by appropriate `#[cfg(feature = "...")]` attributes
4. **Dependency Management**: Database-specific dependencies MUST only be included when their corresponding feature is enabled

### Feature Configuration

```toml
[features]
default = ["sub+sqlite"]
"sub+postgres" = ["dep:sqlx", "sqlx/postgres"]
"sub+sqlite" = ["dep:sqlx", "sqlx/sqlite"]
"sub+surrealdb" = ["dep:reqwest"]
```

### Code Patterns

1. **Module-level conditional compilation**:
   ```rust
   #[cfg(feature = "sub+postgres")]
   pub mod postgres;
    #[cfg(feature = "sub+sqlite")]
    pub mod sqlite;
    #[cfg(feature = "sub+surrealdb")]
    pub mod surrealdb;
   ```

2. **Enum variant conditional compilation**:
   ```rust
   pub enum Subsystem {
       #[cfg(feature = "sub+postgres")]
       Postgres(PostgresConfig),
       #[cfg(feature = "sub+sqlite")]
       Sqlite(SqliteConfig),
       #[cfg(feature = "sub+surrealdb")]
       Surrealdb(SurrealdbConfig),
   }
   ```

3. **Compile-time validation**:
   ```rust
   #[cfg(not(any(feature = "sub+postgres", feature = "sub+sqlite", feature = "sub+surrealdb")))]
   compile_error!("At least one subsystem feature must be enabled");
   ```

## Consequences

### Positive

- **Reduced Binary Size**: Users only pay for the backends they use, resulting in smaller binaries
- **Dependency Isolation**: Database-specific dependencies are only included when needed
- **Modular Architecture**: Clear separation between different subsystem implementations
- **Flexible Deployment**: Users can create specialized builds for specific environments
- **Compile-Time Safety**: Invalid configurations are caught at compile time rather than runtime

### Negative

- **Build Complexity**: Users must understand feature flags to customize builds
- **Testing Overhead**: All feature combinations MUST be tested to ensure compatibility
- **Code Complexity**: Conditional compilation attributes add visual noise to the codebase
- **Documentation Burden**: Feature combinations and their implications MUST be clearly documented

## Implementation

1. All existing subsystem code MUST be audited to ensure proper feature guards are in place
2. Build scripts and CI/CD pipelines MUST test all valid feature combinations
3. User documentation MUST clearly explain how to build with specific features
4. The compile-time validation MUST prevent builds without any subsystem features

## Examples

### Building with specific features:
```bash
# PostgreSQL only
cargo build --no-default-features --features "sub+postgres"

# SQLite only (default)
cargo build --features "sub+sqlite"

# Both backends
cargo build --features "sub+postgres,sub+sqlite"

# SurrealDB only
cargo build --no-default-features --features "sub+surrealdb"

# Invalid - fails at compile time
cargo build --no-default-features
```

## References

- [Rust Book: Conditional Compilation](https://doc.rust-lang.org/reference/conditional-compilation.html)
- [Cargo Book: Features](https://doc.rust-lang.org/cargo/reference/features.html)
