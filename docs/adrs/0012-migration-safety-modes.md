# ADR-0012: Migration Safety Modes (Confirmations, Diff, Dry-Run, Locking)

## Status

Accepted

## Date

2025-01-31T00:08:00Z

## Context

Applying and reverting database migrations is inherently risky. The CLI provides multiple safety mechanisms to prevent accidental destructive actions and to increase operator confidence.

## Decision

The tool MUST provide layered safety mechanisms: confirmations, diff previews, dry-run execution, and migration locking.

### Mechanisms
- Confirmations: Before executing, prompt with yes/no unless `--yes` is provided.
- Diff previews: Allow users to preview raw migration content via `--diff` or interactive `d/diff` during prompt.
- Dry-run: Execute migrations in a transaction and roll back; no persistent changes.
- Locking: Respect `locked` flag from metadata and remote store; reverts require `--unlock`.

## Consequences

### Positive
- Reduces risk of accidental schema changes
- Enables review of migration content prior to execution
- Supports safe testing of migrations in CI/CD

### Negative
- Additional interaction unless `--yes` used
- Slight runtime overhead for diff and dry-run

## Implementation

- Confirmations are handled through `core::migration::prompt_for_confirmation_with_diff` with an optional diff callback.
- Diff rendering uses `core::migration::display_sql_migration` and bulk wrappers per backend.
- Dry-run semantics:
  - Postgres: wrap in transaction and `rollback()` when `--dry`.
  - SQLite: wrap in transaction and `rollback()` when `--dry`.
  - SurrealDB: wrap in SurrealQL transaction and `CANCEL TRANSACTION` when `--dry`.
- Locking behavior:
  - Local `locked` from `meta.toml` is honored when applying (prevent accidental down-revert later when propagated).
  - Remote `locked` column in migrations table prevents revert unless `--unlock` is specified.

### CLI Flags
- `--yes` / `-y`: skip confirmations.
- `--dry`: execute within a transaction and rollback instead of commit.
- `--diff`: preview migration content to be executed.
- `--unlock`: allow reverting a locked migration.

## References

- `core::migration::{prompt_for_confirmation_with_diff, display_sql_migration}`
- `subsystem::<backend>::migration::{execute_sql_statements, insert_migration_record, delete_migration_record, is_migration_locked}`
- `core::service::{up, down, apply_up, apply_down}`
