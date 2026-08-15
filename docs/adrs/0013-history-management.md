# ADR-0013: History Management (Sync and Fix for Non-Linear Chains)

## Status

Accepted

## Date

2025-01-31T00:12:00Z

## Context

Migration IDs are millisecond timestamps. Teams may create migrations concurrently, leading to out-of-order directories relative to applied remote history. This produces non-linear histories that can surprise operators and break assumptions.

The CLI offers tools to either accept and proceed with caution (after operator confirmation) or to repair local directories to match the remote linear history.

## Decision

Provide two explicit history management operations:
- `history sync`: Upsert remote migrations to local filesystem by writing backend-specific up/down files for all known remote entries.
- `history fix`: Rename local out-of-order migrations to preserve a linear chain relative to the latest applied remote ID.

## Consequences

### Positive
- Clear, explicit tools to reconcile local and remote state
- Reduced ambiguity during collaboration
- Maintains linear, timestamp-ordered migration chains

### Negative
- Renaming directories requires VCS coordination
- Potential merge conflicts if performed concurrently by multiple developers

## Implementation

- Detection of non-linear state uses a simple lexicographic comparison of string timestamps against the max applied ID.
- `history fix` increments from max(remote_ts, now) and renames local `id=<old>` to an increasing sequence `id=<new>`.
- `history sync` writes backend-specific up/down files from remote tables to local directories, creating directories as needed.

### User Interaction
- Normal `up`/`down` flows MUST warn when a non-linear sequence is detected and ask for confirmation.
- The warning SHOULD recommend `history fix` to remediate before proceeding.

## References

- `subsystem::<backend>::migration::{history_sync, history_fix}`
- `core::migration::{check_non_linear_history, handle_non_linear_warning}`
