# ADR-0011: Migration File Format and Metadata Semantics

## Status

Accepted

## Date

2025-01-31T00:05:00Z

## Context

Migrations are stored on disk and synchronized with the database migration store. A consistent, simple on-disk format enables portability, code review friendliness, and deterministic application. The codebase expects specific file names and a metadata file per migration.

## Decision

A migration MUST be a directory named with the pattern `id=<timestamp>` containing backend-specific up/down migration files and metadata:
- SQL subsystems use `up.sql` and `down.sql`.
- SurrealDB uses `up.surql` and `down.surql` because migrations are SurrealQL, not SQL.
- `meta.toml` stores optional metadata associated with the migration.

### Semantics
- The `id` MUST be a millisecond UNIX timestamp in string form. The code normalizes references by stripping an optional `id=` prefix.
- Missing `meta.toml` MUST be treated as having default metadata for backward compatibility.
- Metadata MUST be serialized as TOML using the `MigrationMeta` schema.

### Metadata schema
```toml
# meta.toml
comment = "Created by <user> at <timestamp>"
locked = true # optional; defaults to false when absent
```

Rust type:
```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct MigrationMeta {
    pub comment: Option<String>,
    pub locked: Option<bool>,
}
```

Behavioral rules:
- If `comment` is absent, a default SHOULD be generated including user and UTC timestamp when creating the migration.
- If `locked` is `true`, the migration SHOULD be treated as protected; reverts MUST require explicit unlocking (`--unlock`).

## Consequences

### Positive
- Simple, grep-friendly, VCS-friendly format
- Deterministic application order via timestamp IDs
- Extensible metadata via TOML

### Negative
- Timestamp-based IDs can collide in rare cases (mitigated by millisecond precision)
- Requires discipline to keep down migrations valid and in sync

## Implementation

- Directory scanning MUST only accept folders starting with `id=` and then normalize IDs by removing the prefix.
- Helper functions MUST provide IO with clear error contexts:
  - read_migration_files(id)
  - read_migration_files_with_extension(id, extension)
  - read_migration_meta(id)
  - read_migration_with_meta(id)
  - read_migration_with_meta_with_extension(id, extension)
  - create_migration_directory(path, comment, locked)
  - create_migration_directory_with_extension(path, comment, locked, extension)
- Default metadata SHOULD include `whoami::username()` and current UTC timestamp.

## References

- `core::migration::{create_migration_directory, read_migration_meta, read_migration_files, read_migration_with_meta}`
- `core::migration::normalize_migration_id`
