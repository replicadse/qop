# qop - A simple database migration tool

`qop` is a command-line tool for managing database migrations for PostgreSQL, SQLite, and feature-gated SurrealDB. It's designed to be simple, straightforward, and easy to use. The software respects semantic versioning and will only introduce breaking changes in new `major` versions once passing the `1.0.0` version. While being in-development, breaking changes CAN occur in new `minor` versions.

## Features

*   Backend-agnostic design (supports PostgreSQL, SQLite, and SurrealDB)
*   Simple migration file format (`up.sql`, `down.sql`, `meta.toml`; SurrealDB uses `up.surql`, `down.surql`)
*   Migration metadata support (comments, locking status)
*   Migration locking system to prevent accidental reverts
*   Timestamp-based migration IDs
*   Command-line interface for managing migrations
*   Comprehensive audit logging of all migration operations
*   No interactive UI; all confirmations happen via CLI prompts or can be bypassed with `--yes`

## Installation

```bash
cargo install qop
# or
cargo install --path .
```

## Migrations

Please find more information about migration from one version to another in the dedicated [release notes](https://github.com/cchexcode/qop/blob/master/docs/releases/).

## Build features

`qop` is built with Cargo feature flags to include only the subsystems you need. SQLite support is enabled by default.

- Default features
  - Enabled: `sub+sqlite`
  - Disabled: `sub+postgres`, `sub+surrealdb` (optional)

- Enable PostgreSQL (keeping default SQLite):

```bash
cargo build --features "sub+postgres"
```

- PostgreSQL only (no SQLite):

```bash
cargo build --no-default-features --features "sub+postgres"
```

- Enable SurrealDB (keeping default SQLite):

```bash
cargo build --features "sub+surrealdb"
```

- SurrealDB only (no SQLite):

```bash
cargo build --no-default-features --features "sub+surrealdb"
```

- SQLite only (default):

```bash
cargo build            # or: cargo build --features "sub+sqlite"
```

- No subsystems (not allowed):

```bash
cargo build --no-default-features   # Fails at compile time with a clear error
```

Notes:
- Enabling a SQL subsystem feature also enables only the matching `sqlx` backend internally, keeping binaries small.
- SurrealDB support is HTTP-backed, enabled only by `sub+surrealdb`, and uses `reqwest` internally.
- Runtime uses Tokio and Rustls TLS by default. No `sqlx` macros are required.

## Getting Started

1.  **Create a migrations directory and config file:**
    - Create a directory to hold your migrations (for example, `migrations/`). Place your `qop.toml` inside this directory. The tool expects migration folders (like `id=.../`) to live alongside `qop.toml`.
    - Generate a sample config for your database:
      - PostgreSQL:
        ```bash
        qop subsystem postgres config init -p migrations/qop.toml -c "postgresql://postgres:password@localhost:5432/postgres"
        ```
      - SQLite:
        ```bash
        qop subsystem sqlite config init -p migrations/qop.toml -d ./app.db
        ```
      - SurrealDB:
        ```bash
        qop subsystem surrealdb config init -p migrations/qop.toml -e http://127.0.0.1:8000 -n test -d test --user root --password root
        ```

2.  **Initialize the migration table:**
    ```bash
    qop subsystem postgres init -p migrations/qop.toml
    qop subsystem sqlite   init -p migrations/qop.toml
    qop subsystem surrealdb init -p migrations/qop.toml
    ```

3.  **Create your first migration:**
    ```bash
    qop subsystem postgres new -p migrations/qop.toml    # For PostgreSQL
    qop subsystem sqlite   new -p migrations/qop.toml    # For SQLite
    qop subsystem surrealdb new -p migrations/qop.toml   # For SurrealDB
    ```
    This will create a new directory with backend-specific migration files. SQL backends use `up.sql` and `down.sql`; SurrealDB uses `up.surql` and `down.surql`.

4.  **Apply the migration:**
    ```bash
    qop subsystem postgres up -p migrations/qop.toml     # For PostgreSQL
    qop subsystem sqlite   up -p migrations/qop.toml     # For SQLite
    qop subsystem surrealdb up -p migrations/qop.toml    # For SurrealDB
    ```

## Configuration

`qop` is configured using a `qop.toml` file. Here are examples for supported backends:

### PostgreSQL Configuration

```toml
version = ">=0.1.0"

[subsystem.postgres]
connection = { static = "postgresql://postgres:password@localhost:5432/postgres" }
schema = "public"
table_prefix = "__qop"
timeout = 30
```

You can also use environment variables for the connection string:

```toml
version = ">=0.1.0"

[subsystem.postgres]
connection = { from_env = "DATABASE_URL" }
schema = "public"
table_prefix = "__qop"
timeout = 30
```

### SQLite Configuration

```toml
version = ">=0.1.0"

[subsystem.sqlite]
connection = { static = "sqlite:///path/to/database.db" }
table_prefix = "__qop"
timeout = 30
```

Or with environment variables:

```toml
version = ">=0.1.0"

[subsystem.sqlite]
connection = { from_env = "DATABASE_URL" }
table_prefix = "__qop"
timeout = 30
```

### SurrealDB Configuration

SurrealDB support is optional. Build with `sub+surrealdb`, then configure the HTTP endpoint, namespace, and database:

```toml
version = ">=0.1.0"

[subsystem.surrealdb]
connection = { static = "http://127.0.0.1:8000" }
namespace = "test"
database = "test"
username = { static = "root" }
password = { static = "root" }
timeout = 30

[subsystem.surrealdb.tables]
migrations = "__qop_migrations"
log = "__qop_log"
```

You can also load the endpoint and credentials from environment variables:

```toml
version = ">=0.1.0"

[subsystem.surrealdb]
connection = { from_env = "SURREALDB_URL" }
namespace = "test"
database = "test"
username = { from_env = "SURREALDB_USER" }
password = { from_env = "SURREALDB_PASSWORD" }
timeout = 30

[subsystem.surrealdb.tables]
migrations = "__qop_migrations"
log = "__qop_log"
```

The migration files live in the same directory as the `qop.toml` file (e.g., `migrations/`). Each migration is a folder named `id=<timestamp>/` containing backend-specific up/down files plus `meta.toml`. SQL backends use `up.sql` and `down.sql`; SurrealDB uses `up.surql` and `down.surql`.

## Usage

`qop` provides several commands to manage your database migrations through subsystems.

### `subsystem`

The core command for managing database-specific operations. Available aliases: `sub`, `s`

```bash
qop subsystem <DATABASE> <COMMAND>
```

#### PostgreSQL Commands

All PostgreSQL operations are accessed through the `postgres` (alias: `pg`) subsystem:

##### `qop subsystem postgres init`

Initializes the migration table in your PostgreSQL database.

```bash
qop subsystem postgres init --path path/to/your/qop.toml
```

##### `qop subsystem postgres new`

Creates a new migration directory with `up.sql`, `down.sql`, and `meta.toml` files.

```bash
qop subsystem postgres new --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --comment <COMMENT>`: Custom comment for the migration
*   `--lock`: Mark migration as locked (cannot be reverted without --unlock)

This will create a directory structure like:
```
migrations/
└── id=1678886400000/
    ├── up.sql
    ├── down.sql
    └── meta.toml
```

##### `qop subsystem postgres up`

Applies pending migrations. By default, it applies all pending migrations.

```bash
qop subsystem postgres up --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --count <COUNT>`: The number of migrations to apply. If not specified, all pending migrations are applied.
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `-y, --yes`: Skip confirmation prompts and apply migrations automatically

##### `qop subsystem postgres down`

Reverts applied migrations. By default, it reverts the last applied migration.

```bash
qop subsystem postgres down --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --count <COUNT>`: The number of migrations to revert. (default: 1)
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `-r, --remote`: Use the `down.sql` from the database instead of the local file.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--unlock`: Allow reverting locked migrations
*   `-y, --yes`: Skip confirmation prompts and revert migrations automatically

##### `qop subsystem postgres list`

Lists all migrations, showing their status (applied or not) and when they were applied.

```bash
qop subsystem postgres list --path path/to/your/qop.toml
```

**Arguments:**
*   `-o, --output <FORMAT>`: Output format (`human` or `json`). (default: `human`)

##### `qop subsystem postgres history`

Manages migration history with commands for syncing and fixing migration order.

###### `qop subsystem postgres history sync`

Upserts all remote migrations locally. This is useful for syncing migrations across multiple developers.

```bash
qop subsystem postgres history sync --path path/to/your/qop.toml
```

###### `qop subsystem postgres history fix`

Shuffles all non-run local migrations to the end of the chain. This is useful when you have created migrations out of order.

```bash
qop subsystem postgres history fix --path path/to/your/qop.toml
```

##### `qop subsystem postgres diff`

Shows the raw SQL content of pending migrations without applying them.

```bash
qop subsystem postgres diff --path path/to/your/qop.toml
```

This command outputs the exact SQL content for each pending migration using the same formatted preview as the interactive diff (with headers and separators).

##### `qop subsystem postgres apply`

Applies or reverts a specific migration by ID.

###### `qop subsystem postgres apply up`

Applies a specific migration.

```bash
qop subsystem postgres apply up <ID> --path path/to/your/qop.toml
```

**Arguments:**
*   `<ID>`: Migration ID to apply (required)
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--lock`: Mark applied migration as locked (cannot be reverted without --unlock)
*   `-y, --yes`: Skip confirmation prompts and apply migration automatically

###### `qop subsystem postgres apply down`

Reverts a specific migration.

```bash
qop subsystem postgres apply down <ID> --path path/to/your/qop.toml
```

**Arguments:**
*   `<ID>`: Migration ID to revert (required)
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `-r, --remote`: Use the `down.sql` from the database instead of the local file.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--unlock`: Allow reverting locked migrations
*   `-y, --yes`: Skip confirmation prompts and revert migration automatically

#### SQLite Commands

All SQLite operations are accessed through the `sqlite` (alias: `sql`) subsystem and support the same commands as PostgreSQL:

##### `qop subsystem sqlite init`

Initializes the migration table in your SQLite database.

```bash
qop subsystem sqlite init --path path/to/your/qop.toml
```

##### `qop subsystem sqlite new`

Creates a new migration directory with `up.sql`, `down.sql`, and `meta.toml` files.

```bash
qop subsystem sqlite new --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --comment <COMMENT>`: Custom comment for the migration
*   `--lock`: Mark migration as locked (cannot be reverted without --unlock)

##### `qop subsystem sqlite up`

Applies pending migrations.

```bash
qop subsystem sqlite up --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --count <COUNT>`: The number of migrations to apply.
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `-y, --yes`: Skip confirmation prompts and apply migrations automatically

##### `qop subsystem sqlite down`

Reverts applied migrations.

```bash
qop subsystem sqlite down --path path/to/your/qop.toml
```

**Arguments:**
*   `-p, --path <PATH>`: Path to the `qop.toml` configuration file. (default: `qop.toml`)
*   `-c, --count <COUNT>`: The number of migrations to revert.
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `-r, --remote`: Use the `down.sql` from the database instead of the local file.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--unlock`: Allow reverting locked migrations
*   `-y, --yes`: Skip confirmation prompts and revert migrations automatically

##### `qop subsystem sqlite list`

Lists all migrations, showing their status and when they were applied.

```bash
qop subsystem sqlite list --path path/to/your/qop.toml
```

**Arguments:**
*   `-o, --output <FORMAT>`: Output format (`human` or `json`). (default: `human`)

##### `qop subsystem sqlite history sync`

Upserts all remote migrations locally.

```bash
qop subsystem sqlite history sync --path path/to/your/qop.toml
```

##### `qop subsystem sqlite history fix`

Shuffles all non-run local migrations to the end of the chain.

```bash
qop subsystem sqlite history fix --path path/to/your/qop.toml
```

##### `qop subsystem sqlite diff`

Shows the raw SQL content of pending migrations without applying them.

```bash
qop subsystem sqlite diff --path path/to/your/qop.toml
```

This command outputs the exact SQL content for each pending migration using the same formatted preview as the interactive diff (with headers and separators).

##### `qop subsystem sqlite apply up`

Applies a specific migration by ID.

```bash
qop subsystem sqlite apply up <ID> --path path/to/your/qop.toml
```

**Arguments:**
*   `<ID>`: Migration ID to apply (required)
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--lock`: Mark applied migration as locked (cannot be reverted without --unlock)
*   `-y, --yes`: Skip confirmation prompts and apply migration automatically

##### `qop subsystem sqlite apply down`

Reverts a specific migration by ID.

```bash
qop subsystem sqlite apply down <ID> --path path/to/your/qop.toml
```

**Arguments:**
*   `<ID>`: Migration ID to revert (required)
*   `-t, --timeout <TIMEOUT>`: Statement timeout in seconds.
*   `-r, --remote`: Use the `down.sql` from the database instead of the local file.
*   `--dry`: Execute migration in a transaction but rollback instead of committing
*   `--unlock`: Allow reverting locked migrations
*   `-y, --yes`: Skip confirmation prompts and revert migration automatically

#### SurrealDB Commands

SurrealDB operations are available when built with `sub+surrealdb` and are accessed through the `surrealdb` (alias: `surreal`) subsystem. The command set matches the other subsystems: `config init`, `init`, `new`, `up`, `down`, `list`, `history sync`, `history fix`, `diff`, `apply up`, and `apply down`.

SurrealDB migrations are SurrealQL files named `up.surql` and `down.surql`, not SQL files.

```bash
qop subsystem surrealdb config init -p migrations/qop.toml -e http://127.0.0.1:8000 -n test -d test --user root --password root
qop subsystem surrealdb init -p migrations/qop.toml
qop subsystem surrealdb up -p migrations/qop.toml --yes
```

### `man`

Renders the manual.

#### `qop man`

```bash
qop man --out docs/manual --format markdown
```

**Arguments:**
*   `-o, --out <PATH>`: Path to write documentation to (required)
*   `-f, --format <FORMAT>`: Format for the documentation. Can be `manpages` or `markdown` (required)

### `autocomplete`

Renders shell completion scripts.

#### `qop autocomplete`

```bash
qop autocomplete --out completions --shell zsh
```

**Arguments:**
*   `-o, --out <PATH>`: Path to write completion script to (required)
*   `-s, --shell <SHELL>`: The shell to generate completions for (`bash`, `zsh`, `fish`, `elvish`, `powershell`) (required)

## Migration Preview and Safety Features

### Preview migrations during confirmation

During confirmation prompts, type `d` or `diff` to preview the exact migration content for the operation:

```bash
# Apply pending migrations (press 'd' at the prompt to preview content)
qop subsystem postgres up -p migrations/qop.toml

# Revert last migration (press 'd' at the prompt to preview content)
qop subsystem postgres down -p migrations/qop.toml
```

The preview shows the raw migration content exactly as it will be executed, with no additional formatting.

### Diff command

You can also print pending migration content without prompts using the diff command:

```bash
qop subsystem postgres diff -p migrations/qop.toml
qop subsystem sqlite   diff -p migrations/qop.toml
qop subsystem surrealdb diff -p migrations/qop.toml
```

**Example Output:**
```sql
CREATE TABLE users (
    id SERIAL PRIMARY KEY,
    email VARCHAR(255) UNIQUE NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_users_email ON users(email);
```

The output contains only the statements from your migration files, making it easy to redirect to files or pipe to other tools.

### Automated mode

**Skip confirmations with `--yes`:**
```bash
# Apply all pending migrations without prompts
qop subsystem postgres up --yes

# Revert last migration without prompts
qop subsystem postgres down --yes
```

The `--dry` flag is now available for all migration commands and executes migrations in a transaction that is rolled back instead of committed, allowing you to test migrations safely.

### Practical Examples

**Development Workflow:**
```bash
# 1. Check what migrations are pending
qop subsystem postgres diff

# 2. Apply with confirmation
qop subsystem postgres up
```

**CI/CD Pipeline:**
```bash
# Apply all pending migrations automatically
qop subsystem postgres up --yes
```

**Debugging:**
```bash
# Save pending SQL to a file for review
qop subsystem postgres diff > pending_migrations.sql

# Apply a specific migration
qop subsystem postgres apply up 123456789
```

**Database Rollback:**
```bash
# Preview what will be rolled back (press 'd' at the prompt)
qop subsystem postgres down

# Rollback for real
qop subsystem postgres down --yes
```
