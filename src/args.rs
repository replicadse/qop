use {
    anyhow::Result,
    clap::Arg,
    clap::ArgAction,
    path_clean::PathClean,
    std::{path::PathBuf, str::FromStr},
};

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Privilege {
    Normal,
    Experimental,
}

#[derive(Debug)]
pub(crate) enum ManualFormat {
    Manpages,
    Markdown,
}

#[derive(Debug)]
pub(crate) struct CallArgs {
    #[allow(dead_code)]
    pub privileges: Privilege,
    pub command: Command,
}

impl CallArgs {
    pub(crate) fn validate(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) enum Subsystem {
    #[cfg(feature = "sub+postgres")]
    Postgres {
        path: PathBuf,
        config: crate::subsystem::postgres::config::SubsystemPostgres,
        command: crate::subsystem::postgres::commands::Command,
    },
    #[cfg(feature = "sub+surrealdb")]
    Surrealdb {
        path: PathBuf,
        config: crate::subsystem::surrealdb::config::SubsystemSurrealdb,
        command: crate::subsystem::surrealdb::commands::Command,
    },
    #[cfg(feature = "sub+sqlite")]
    Sqlite {
        path: PathBuf,
        config: crate::subsystem::sqlite::config::SubsystemSqlite,
        command: crate::subsystem::sqlite::commands::Command,
    },
}

#[derive(Debug)]
pub(crate) enum Command {
    Manual { path: PathBuf, format: ManualFormat },
    Autocomplete { path: PathBuf, shell: clap_complete::Shell },
    Subsystem(Subsystem),
}

pub(crate) struct ClapArgumentLoader {}

impl ClapArgumentLoader {
    fn get_absolute_path(matches: &clap::ArgMatches, name: &str) -> Result<PathBuf> {
        let path_str: &String = matches.get_one(name).unwrap();
        let path = std::path::Path::new(path_str);
        if path.is_absolute() {
            Ok(path.to_path_buf().clean())
        } else {
            Ok(std::env::current_dir()?.join(path).clean())
        }
    }
    pub(crate) fn root_command() -> clap::Command {
        let mut enabled: Vec<&str> = Vec::new();
        #[cfg(feature = "sub+postgres")]
        {
            enabled.push("postgres");
        }
        #[cfg(feature = "sub+surrealdb")]
        {
            enabled.push("surrealdb");
        }
        #[cfg(feature = "sub+sqlite")]
        {
            enabled.push("sqlite");
        }
        let enabled_str = if enabled.is_empty() {
            String::from("none")
        } else {
            enabled.join(", ")
        };

        let mut root = clap::Command::new("qop")
            .version(env!("CARGO_PKG_VERSION"))
            .about(format!(
                "Database migrations for savages.\n\nEnabled subsystems: {}",
                enabled_str
            ))
            .author("cchexcode <alexanderh.weber@outlook.com>")
            .propagate_version(true)
            .subcommand_required(false)
            .args([Arg::new("experimental")
                .short('e')
                .long("experimental")
                .help("Enables experimental features.")
                .num_args(0)])
            .subcommand(
                clap::Command::new("man")
                    .about("Renders the manual.")
                    .arg(clap::Arg::new("out").short('o').long("out").required(true))
                    .arg(
                        clap::Arg::new("format")
                            .short('f')
                            .long("format")
                            .value_parser(["manpages", "markdown"])
                            .required(true),
                    ),
            )
            .subcommand(
                clap::Command::new("autocomplete")
                    .about("Renders shell completion scripts.")
                    .arg(clap::Arg::new("out").short('o').long("out").required(true))
                    .arg(
                        clap::Arg::new("shell")
                            .short('s')
                            .long("shell")
                            .value_parser(["bash", "zsh", "fish", "elvish", "powershell"])
                            .required(true),
                    ),
            );

        #[cfg(any(feature = "sub+postgres", feature = "sub+sqlite", feature = "sub+surrealdb"))]
        {
            let mut subsystem = clap::Command::new("subsystem")
                .about(format!("Manages subsystems (enabled: {}).", enabled_str))
                .subcommand_required(true)
                .aliases(["sub", "s"]);

            #[cfg(feature = "sub+postgres")]
            {
                let pg = clap::Command::new("postgres")
                    .aliases(["pg"])
                    .about("Manages PostgreSQL migrations.")
                    .arg(clap::Arg::new("path").short('p').long("path").default_value("qop.toml"))
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("config")
                            .about("Configuration commands.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("init")
                                    .about("Writes a sample configuration for Postgres.")
                                    .arg(
                                        clap::Arg::new("conn")
                                            .short('c')
                                            .long("conn")
                                            .help("Database connection string")
                                            .required(true),
                                    ),
                            ),
                    )
                    .subcommand(clap::Command::new("init").about("Initializes the database."))
                    .subcommand(
                        clap::Command::new("new")
                            .about("Creates a new migration.")
                            .arg(
                                clap::Arg::new("comment")
                                    .short('c')
                                    .long("comment")
                                    .help("Comment for the migration"),
                            )
                            .arg(
                                clap::Arg::new("locked")
                                    .long("lock")
                                    .num_args(0)
                                    .help("Mark migration as locked (cannot be reverted without --unlock)"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("up")
                            .about("Runs the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(clap::Arg::new("count").short('c').long("count").required(false))
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            )
                            .arg(
                                clap::Arg::new("unlock")
                                    .long("unlock")
                                    .num_args(0)
                                    .help("Allow reverting locked migrations"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("down")
                            .about("Rolls back the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(
                                clap::Arg::new("remote")
                                    .short('r')
                                    .long("remote")
                                    .required(false)
                                    .num_args(0),
                            )
                            .arg(clap::Arg::new("count").short('c').long("count").required(false))
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            )
                            .arg(
                                clap::Arg::new("unlock")
                                    .long("unlock")
                                    .num_args(0)
                                    .help("Allow reverting locked migrations"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("list").about("Lists all applied migrations.").arg(
                            clap::Arg::new("output")
                                .short('o')
                                .long("output")
                                .required(false)
                                .value_parser(["human", "json"])
                                .help("Output format"),
                        ),
                    )
                    .subcommand(
                        clap::Command::new("history")
                            .about("Manages migration history.")
                            .subcommand_required(true)
                            .subcommand(clap::Command::new("sync").about("Upserts all remote migrations locally."))
                            .subcommand(
                                clap::Command::new("fix")
                                    .about("Shuffles all non-run local migrations to the end of the chain."),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("diff").about("Shows pending migration operations without applying them."),
                    )
                    .subcommand(
                        clap::Command::new("apply")
                            .about("Applies or reverts a specific migration by ID.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("up")
                                    .about("Applies a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to apply").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    )
                                    .arg(clap::Arg::new("locked").long("lock").num_args(0).help(
                                        "Mark applied migration as locked (cannot be reverted without --unlock)",
                                    )),
                            )
                            .subcommand(
                                clap::Command::new("down")
                                    .about("Reverts a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to revert").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("remote")
                                            .short('r')
                                            .long("remote")
                                            .required(false)
                                            .num_args(0),
                                    )
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    )
                                    .arg(
                                        clap::Arg::new("unlock")
                                            .long("unlock")
                                            .num_args(0)
                                            .action(ArgAction::SetTrue)
                                            .help("Allow reverting locked migrations"),
                                    ),
                            ),
                    );
                subsystem = subsystem.subcommand(pg);
            }

            #[cfg(feature = "sub+sqlite")]
            {
                let sql = clap::Command::new("sqlite")
                    .aliases(["sql"])
                    .about("Manages SQLite migrations.")
                    .arg(clap::Arg::new("path").short('p').long("path").default_value("qop.toml"))
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("config")
                            .about("Configuration commands.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("init")
                                    .about("Writes a sample configuration for SQLite.")
                                    .arg(
                                        clap::Arg::new("db")
                                            .short('d')
                                            .long("db")
                                            .help("Database file path")
                                            .required(true),
                                    ),
                            ),
                    )
                    .subcommand(clap::Command::new("init").about("Initializes the database."))
                    .subcommand(
                        clap::Command::new("new")
                            .about("Creates a new migration.")
                            .arg(
                                clap::Arg::new("comment")
                                    .short('c')
                                    .long("comment")
                                    .help("Comment for the migration"),
                            )
                            .arg(
                                clap::Arg::new("locked")
                                    .long("lock")
                                    .num_args(0)
                                    .help("Mark migration as locked (cannot be reverted without --unlock)"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("up")
                            .about("Runs the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(clap::Arg::new("count").short('c').long("count").required(false))
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            )
                            .arg(
                                clap::Arg::new("unlock")
                                    .long("unlock")
                                    .num_args(0)
                                    .help("Allow reverting locked migrations"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("down")
                            .about("Rolls back the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(
                                clap::Arg::new("remote")
                                    .short('r')
                                    .long("remote")
                                    .required(false)
                                    .num_args(0),
                            )
                            .arg(clap::Arg::new("count").short('c').long("count").required(false))
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            )
                            .arg(
                                clap::Arg::new("unlock")
                                    .long("unlock")
                                    .num_args(0)
                                    .action(ArgAction::SetTrue)
                                    .help("Allow reverting locked migrations"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("list").about("Lists all applied migrations.").arg(
                            clap::Arg::new("output")
                                .short('o')
                                .long("output")
                                .required(false)
                                .value_parser(["human", "json"])
                                .help("Output format"),
                        ),
                    )
                    .subcommand(
                        clap::Command::new("history")
                            .about("Manages migration history.")
                            .subcommand_required(true)
                            .subcommand(clap::Command::new("sync").about("Upserts all remote migrations locally."))
                            .subcommand(
                                clap::Command::new("fix")
                                    .about("Shuffles all non-run local migrations to the end of the chain."),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("diff").about("Shows pending migration operations without applying them."),
                    )
                    .subcommand(
                        clap::Command::new("apply")
                            .about("Applies or reverts a specific migration by ID.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("up")
                                    .about("Applies a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to apply").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    )
                                    .arg(clap::Arg::new("locked").long("lock").num_args(0).help(
                                        "Mark applied migration as locked (cannot be reverted without --unlock)",
                                    )),
                            )
                            .subcommand(
                                clap::Command::new("down")
                                    .about("Reverts a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to revert").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("remote")
                                            .short('r')
                                            .long("remote")
                                            .required(false)
                                            .num_args(0),
                                    )
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    )
                                    .arg(clap::Arg::new("locked").long("lock").num_args(0).help(
                                        "Mark applied migration as locked (cannot be reverted without --unlock)",
                                    )),
                            ),
                    );
                subsystem = subsystem.subcommand(sql);
            }

            #[cfg(feature = "sub+surrealdb")]
            {
                let surreal = clap::Command::new("surrealdb")
                    .aliases(["surreal"])
                    .about("Manages SurrealDB migrations.")
                    .arg(clap::Arg::new("path").short('p').long("path").default_value("qop.toml"))
                    .subcommand_required(true)
                    .subcommand(
                        clap::Command::new("config")
                            .about("Configuration commands.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("init")
                                    .about("Writes a sample configuration for SurrealDB.")
                                    .arg(
                                        clap::Arg::new("endpoint")
                                            .short('e')
                                            .long("endpoint")
                                            .help("SurrealDB HTTP endpoint")
                                            .required(true),
                                    )
                                    .arg(
                                        clap::Arg::new("namespace")
                                            .short('n')
                                            .long("namespace")
                                            .help("SurrealDB namespace")
                                            .required(true),
                                    )
                                    .arg(
                                        clap::Arg::new("database")
                                            .short('d')
                                            .long("database")
                                            .help("SurrealDB database")
                                            .required(true),
                                    )
                                    .arg(
                                        clap::Arg::new("username")
                                            .long("user")
                                            .help("SurrealDB username")
                                            .requires("password"),
                                    )
                                    .arg(
                                        clap::Arg::new("password")
                                            .long("password")
                                            .help("SurrealDB password")
                                            .requires("username"),
                                    ),
                            ),
                    )
                    .subcommand(clap::Command::new("init").about("Initializes the database."))
                    .subcommand(
                        clap::Command::new("new")
                            .about("Creates a new migration.")
                            .arg(
                                clap::Arg::new("comment")
                                    .short('c')
                                    .long("comment")
                                    .help("Comment for the migration"),
                            )
                            .arg(
                                clap::Arg::new("locked")
                                    .long("lock")
                                    .num_args(0)
                                    .help("Mark migration as locked (cannot be reverted without --unlock)"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("up")
                            .about("Runs the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(clap::Arg::new("count").short('c').long("count").required(false))
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("down")
                            .about("Rolls back the migrations.")
                            .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                            .arg(
                                clap::Arg::new("remote")
                                    .short('r')
                                    .long("remote")
                                    .required(false)
                                    .num_args(0),
                            )
                            .arg(
                                clap::Arg::new("count")
                                    .short('c')
                                    .long("count")
                                    .required(false)
                                    .default_value("1"),
                            )
                            .arg(
                                clap::Arg::new("diff")
                                    .short('d')
                                    .long("diff")
                                    .required(false)
                                    .num_args(0)
                                    .help("Show migration diff before applying"),
                            )
                            .arg(
                                clap::Arg::new("dry")
                                    .long("dry")
                                    .required(false)
                                    .num_args(0)
                                    .help("Execute migration in a transaction but rollback instead of committing")
                                    .conflicts_with("yes"),
                            )
                            .arg(
                                clap::Arg::new("yes")
                                    .short('y')
                                    .long("yes")
                                    .required(false)
                                    .num_args(0)
                                    .help("Skip confirmation prompts"),
                            )
                            .arg(
                                clap::Arg::new("unlock")
                                    .long("unlock")
                                    .num_args(0)
                                    .action(ArgAction::SetTrue)
                                    .help("Allow reverting locked migrations"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("list").about("Lists all applied migrations.").arg(
                            clap::Arg::new("output")
                                .short('o')
                                .long("output")
                                .required(false)
                                .value_parser(["human", "json"])
                                .help("Output format"),
                        ),
                    )
                    .subcommand(
                        clap::Command::new("history")
                            .about("Manages migration history.")
                            .subcommand_required(true)
                            .subcommand(clap::Command::new("sync").about("Upserts all remote migrations locally."))
                            .subcommand(
                                clap::Command::new("fix")
                                    .about("Shuffles all non-run local migrations to the end of the chain."),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("diff").about("Shows pending migration operations without applying them."),
                    )
                    .subcommand(
                        clap::Command::new("apply")
                            .about("Applies or reverts a specific migration by ID.")
                            .subcommand_required(true)
                            .subcommand(
                                clap::Command::new("up")
                                    .about("Applies a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to apply").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    ),
                            )
                            .subcommand(
                                clap::Command::new("down")
                                    .about("Reverts a specific migration.")
                                    .arg(clap::Arg::new("id").help("Migration ID to revert").required(true))
                                    .arg(clap::Arg::new("timeout").short('t').long("timeout").required(false))
                                    .arg(
                                        clap::Arg::new("remote")
                                            .short('r')
                                            .long("remote")
                                            .required(false)
                                            .num_args(0),
                                    )
                                    .arg(
                                        clap::Arg::new("dry")
                                            .long("dry")
                                            .required(false)
                                            .num_args(0)
                                            .help(
                                                "Execute migration in a transaction but rollback instead of committing",
                                            )
                                            .conflicts_with("yes"),
                                    )
                                    .arg(
                                        clap::Arg::new("yes")
                                            .short('y')
                                            .long("yes")
                                            .required(false)
                                            .num_args(0)
                                            .help("Skip confirmation prompts"),
                                    )
                                    .arg(
                                        clap::Arg::new("unlock")
                                            .long("unlock")
                                            .num_args(0)
                                            .action(ArgAction::SetTrue)
                                            .help("Allow reverting locked migrations"),
                                    ),
                            ),
                    );
                subsystem = subsystem.subcommand(surreal);
            }

            root = root.subcommand(subsystem);
        }

        root
    }

    pub(crate) fn load() -> Result<CallArgs> {
        let command = Self::root_command().get_matches();

        let privileges = if command.get_flag("experimental") {
            Privilege::Experimental
        } else {
            Privilege::Normal
        };

        let cmd = if let Some(subc) = command.subcommand_matches("man") {
            Command::Manual {
                path: Self::get_absolute_path(subc, "out")?,
                format: match subc.get_one::<String>("format").unwrap().as_str() {
                    | "manpages" => ManualFormat::Manpages,
                    | "markdown" => ManualFormat::Markdown,
                    | _ => return Err(anyhow::anyhow!("argument \"format\": unknown format")),
                },
            }
        } else if let Some(subc) = command.subcommand_matches("autocomplete") {
            Command::Autocomplete {
                path: Self::get_absolute_path(subc, "out")?,
                shell: clap_complete::Shell::from_str(subc.get_one::<String>("shell").unwrap().as_str()).unwrap(),
            }
        } else if let Some(subsystem_subc) = command.subcommand_matches("subsystem") {
            // Try postgres branch if feature enabled
            #[cfg(feature = "sub+postgres")]
            {
                if let Some(postgres_subc) = subsystem_subc.subcommand_matches("postgres") {
                    let path = Self::get_absolute_path(postgres_subc, "path")?;
                    let (pg_cfg, postgres_cmd) = if let Some(config_subc) = postgres_subc.subcommand_matches("config") {
                        if let Some(init_subc) = config_subc.subcommand_matches("init") {
                            let conn = init_subc.get_one::<String>("conn").unwrap().clone();
                            (
                                crate::subsystem::postgres::config::SubsystemPostgres::default(),
                                crate::subsystem::postgres::commands::Command::Config(
                                    crate::subsystem::postgres::commands::ConfigCommand::Init { connection: conn },
                                ),
                            )
                        } else {
                            unreachable!()
                        }
                    } else {
                        let cfg: crate::config::Config = toml::from_str(&std::fs::read_to_string(&path)?)?;
                        // Validate CLI version against config requirement
                        crate::config::WithVersion {
                            version: cfg.version.clone(),
                        }
                        .validate(env!("CARGO_PKG_VERSION"))?;
                        #[cfg(any(feature = "sub+sqlite", feature = "sub+surrealdb"))]
                        let pg_cfg = match cfg.subsystem {
                            | crate::config::Subsystem::Postgres(c) => c,
                            | _ => anyhow::bail!("config is not postgres"),
                        };
                        #[cfg(not(any(feature = "sub+sqlite", feature = "sub+surrealdb")))]
                        let pg_cfg = match cfg.subsystem {
                            | crate::config::Subsystem::Postgres(c) => c,
                        };
                        let postgres_cmd = if let Some(_) = postgres_subc.subcommand_matches("init") {
                            crate::subsystem::postgres::commands::Command::Init
                        } else if let Some(new_subc) = postgres_subc.subcommand_matches("new") {
                            crate::subsystem::postgres::commands::Command::New {
                                comment: new_subc.get_one::<String>("comment").cloned(),
                                locked: new_subc.get_flag("locked"),
                            }
                        } else if let Some(up_subc) = postgres_subc.subcommand_matches("up") {
                            crate::subsystem::postgres::commands::Command::Up {
                                timeout: up_subc.get_one::<String>("timeout").map(|s| s.parse::<u64>().unwrap()),
                                count: up_subc.get_one::<String>("count").map(|s| s.parse::<usize>().unwrap()),
                                diff: up_subc.get_flag("diff"),
                                dry: up_subc.get_flag("dry"),
                                yes: up_subc.get_flag("yes"),
                            }
                        } else if let Some(down_subc) = postgres_subc.subcommand_matches("down") {
                            crate::subsystem::postgres::commands::Command::Down {
                                timeout: down_subc
                                    .get_one::<String>("timeout")
                                    .map(|s| s.parse::<u64>().unwrap()),
                                count: down_subc.get_one::<String>("count").unwrap().parse::<usize>().unwrap(),
                                remote: down_subc.get_flag("remote"),
                                diff: down_subc.get_flag("diff"),
                                dry: down_subc.get_flag("dry"),
                                yes: down_subc.get_flag("yes"),
                                unlock: down_subc.get_flag("unlock"),
                            }
                        } else if let Some(list_subc) = postgres_subc.subcommand_matches("list") {
                            let out = match list_subc
                                .get_one::<String>("output")
                                .map(|s| s.as_str())
                                .unwrap_or("human")
                            {
                                | "human" => crate::subsystem::postgres::commands::Output::Human,
                                | "json" => crate::subsystem::postgres::commands::Output::Json,
                                | _ => crate::subsystem::postgres::commands::Output::Human,
                            };
                            crate::subsystem::postgres::commands::Command::List { output: out }
                        } else if let Some(history_subc) = postgres_subc.subcommand_matches("history") {
                            let history_cmd = if let Some(_) = history_subc.subcommand_matches("sync") {
                                crate::subsystem::postgres::commands::HistoryCommand::Sync
                            } else if let Some(_) = history_subc.subcommand_matches("fix") {
                                crate::subsystem::postgres::commands::HistoryCommand::Fix
                            } else {
                                unreachable!();
                            };
                            crate::subsystem::postgres::commands::Command::History(history_cmd)
                        } else if let Some(_) = postgres_subc.subcommand_matches("diff") {
                            crate::subsystem::postgres::commands::Command::Diff
                        } else if let Some(apply_subc) = postgres_subc.subcommand_matches("apply") {
                            if let Some(up_subc) = apply_subc.subcommand_matches("up") {
                                crate::subsystem::postgres::commands::Command::Apply(
                                    crate::subsystem::postgres::commands::MigrationApply::Up {
                                        id: up_subc.get_one::<String>("id").unwrap().clone(),
                                        timeout: up_subc
                                            .get_one::<String>("timeout")
                                            .map(|s| s.parse::<u64>().unwrap()),
                                        dry: up_subc.get_flag("dry"),
                                        yes: up_subc.get_flag("yes"),
                                    },
                                )
                            } else if let Some(down_subc) = apply_subc.subcommand_matches("down") {
                                crate::subsystem::postgres::commands::Command::Apply(
                                    crate::subsystem::postgres::commands::MigrationApply::Down {
                                        id: down_subc.get_one::<String>("id").unwrap().clone(),
                                        timeout: down_subc
                                            .get_one::<String>("timeout")
                                            .map(|s| s.parse::<u64>().unwrap()),
                                        remote: down_subc.get_flag("remote"),
                                        dry: down_subc.get_flag("dry"),
                                        yes: down_subc.get_flag("yes"),
                                        unlock: down_subc.get_flag("unlock"),
                                    },
                                )
                            } else {
                                unreachable!();
                            }
                        } else {
                            unreachable!();
                        };
                        (pg_cfg, postgres_cmd)
                    };
                    return Ok(CallArgs {
                        privileges,
                        command: Command::Subsystem(Subsystem::Postgres {
                            path,
                            config: pg_cfg,
                            command: postgres_cmd,
                        }),
                    });
                }
            }
            // Try sqlite branch if feature enabled
            #[cfg(feature = "sub+sqlite")]
            {
                if let Some(sqlite_subc) = subsystem_subc.subcommand_matches("sqlite") {
                    let path = Self::get_absolute_path(sqlite_subc, "path")?;
                    let (sql_cfg, sqlite_cmd) = if let Some(config_subc) = sqlite_subc.subcommand_matches("config") {
                        if let Some(init_subc) = config_subc.subcommand_matches("init") {
                            let db = init_subc.get_one::<String>("db").unwrap().clone();
                            (
                                crate::subsystem::sqlite::config::SubsystemSqlite::default(),
                                crate::subsystem::sqlite::commands::Command::Config(
                                    crate::subsystem::sqlite::commands::ConfigCommand::Init { path: db },
                                ),
                            )
                        } else {
                            unreachable!()
                        }
                    } else {
                        let cfg: crate::config::Config = toml::from_str(&std::fs::read_to_string(&path)?)?;
                        // Validate CLI version against config requirement
                        crate::config::WithVersion {
                            version: cfg.version.clone(),
                        }
                        .validate(env!("CARGO_PKG_VERSION"))?;
                        #[cfg(any(feature = "sub+postgres", feature = "sub+surrealdb"))]
                        let sql_cfg = match cfg.subsystem {
                            | crate::config::Subsystem::Sqlite(c) => c,
                            | _ => anyhow::bail!("config is not sqlite"),
                        };
                        #[cfg(not(any(feature = "sub+postgres", feature = "sub+surrealdb")))]
                        let sql_cfg = match cfg.subsystem {
                            | crate::config::Subsystem::Sqlite(c) => c,
                        };
                        let sqlite_cmd = if let Some(_) = sqlite_subc.subcommand_matches("init") {
                            crate::subsystem::sqlite::commands::Command::Init
                        } else if let Some(new_subc) = sqlite_subc.subcommand_matches("new") {
                            crate::subsystem::sqlite::commands::Command::New {
                                comment: new_subc.get_one::<String>("comment").cloned(),
                                locked: new_subc.get_flag("locked"),
                            }
                        } else if let Some(up_subc) = sqlite_subc.subcommand_matches("up") {
                            crate::subsystem::sqlite::commands::Command::Up {
                                timeout: up_subc.get_one::<String>("timeout").map(|s| s.parse::<u64>().unwrap()),
                                count: up_subc.get_one::<String>("count").map(|s| s.parse::<usize>().unwrap()),
                                diff: up_subc.get_flag("diff"),
                                dry: up_subc.get_flag("dry"),
                                yes: up_subc.get_flag("yes"),
                            }
                        } else if let Some(down_subc) = sqlite_subc.subcommand_matches("down") {
                            crate::subsystem::sqlite::commands::Command::Down {
                                timeout: down_subc
                                    .get_one::<String>("timeout")
                                    .map(|s| s.parse::<u64>().unwrap()),
                                count: down_subc.get_one::<String>("count").unwrap().parse::<usize>().unwrap(),
                                remote: down_subc.get_flag("remote"),
                                diff: down_subc.get_flag("diff"),
                                dry: down_subc.get_flag("dry"),
                                yes: down_subc.get_flag("yes"),
                                unlock: down_subc.get_flag("unlock"),
                            }
                        } else if let Some(list_subc) = sqlite_subc.subcommand_matches("list") {
                            let out = match list_subc
                                .get_one::<String>("output")
                                .map(|s| s.as_str())
                                .unwrap_or("human")
                            {
                                | "human" => crate::subsystem::sqlite::commands::Output::Human,
                                | "json" => crate::subsystem::sqlite::commands::Output::Json,
                                | _ => crate::subsystem::sqlite::commands::Output::Human,
                            };
                            crate::subsystem::sqlite::commands::Command::List { output: out }
                        } else if let Some(history_subc) = sqlite_subc.subcommand_matches("history") {
                            let history_cmd = if let Some(_) = history_subc.subcommand_matches("sync") {
                                crate::subsystem::sqlite::commands::HistoryCommand::Sync
                            } else if let Some(_) = history_subc.subcommand_matches("fix") {
                                crate::subsystem::sqlite::commands::HistoryCommand::Fix
                            } else {
                                unreachable!();
                            };
                            crate::subsystem::sqlite::commands::Command::History(history_cmd)
                        } else if let Some(_) = sqlite_subc.subcommand_matches("diff") {
                            crate::subsystem::sqlite::commands::Command::Diff
                        } else if let Some(apply_subc) = sqlite_subc.subcommand_matches("apply") {
                            if let Some(up_subc) = apply_subc.subcommand_matches("up") {
                                crate::subsystem::sqlite::commands::Command::Apply(
                                    crate::subsystem::sqlite::commands::MigrationApply::Up {
                                        id: up_subc.get_one::<String>("id").unwrap().clone(),
                                        timeout: up_subc
                                            .get_one::<String>("timeout")
                                            .map(|s| s.parse::<u64>().unwrap()),
                                        dry: up_subc.get_flag("dry"),
                                        yes: up_subc.get_flag("yes"),
                                    },
                                )
                            } else if let Some(down_subc) = apply_subc.subcommand_matches("down") {
                                crate::subsystem::sqlite::commands::Command::Apply(
                                    crate::subsystem::sqlite::commands::MigrationApply::Down {
                                        id: down_subc.get_one::<String>("id").unwrap().clone(),
                                        timeout: down_subc
                                            .get_one::<String>("timeout")
                                            .map(|s| s.parse::<u64>().unwrap()),
                                        remote: down_subc.get_flag("remote"),
                                        dry: down_subc.get_flag("dry"),
                                        yes: down_subc.get_flag("yes"),
                                        unlock: down_subc.get_flag("unlock"),
                                    },
                                )
                            } else {
                                unreachable!();
                            }
                        } else {
                            unreachable!();
                        };
                        (sql_cfg, sqlite_cmd)
                    };
                    return Ok(CallArgs {
                        privileges,
                        command: Command::Subsystem(Subsystem::Sqlite {
                            path,
                            config: sql_cfg,
                            command: sqlite_cmd,
                        }),
                    });
                }
            }
            // Try surrealdb branch if feature enabled
            #[cfg(feature = "sub+surrealdb")]
            {
                if let Some(surrealdb_subc) = subsystem_subc.subcommand_matches("surrealdb") {
                    let path = Self::get_absolute_path(surrealdb_subc, "path")?;
                    let (surrealdb_cfg, surrealdb_cmd) =
                        if let Some(config_subc) = surrealdb_subc.subcommand_matches("config") {
                            if let Some(init_subc) = config_subc.subcommand_matches("init") {
                                let connection = init_subc.get_one::<String>("endpoint").unwrap().clone();
                                let namespace = init_subc.get_one::<String>("namespace").unwrap().clone();
                                let database = init_subc.get_one::<String>("database").unwrap().clone();
                                let username = init_subc.get_one::<String>("username").cloned();
                                let password = init_subc.get_one::<String>("password").cloned();
                                (
                                    crate::subsystem::surrealdb::config::SubsystemSurrealdb::default(),
                                    crate::subsystem::surrealdb::commands::Command::Config(
                                        crate::subsystem::surrealdb::commands::ConfigCommand::Init {
                                            connection,
                                            namespace,
                                            database,
                                            username,
                                            password,
                                        },
                                    ),
                                )
                            } else {
                                unreachable!()
                            }
                        } else {
                            let cfg: crate::config::Config = toml::from_str(&std::fs::read_to_string(&path)?)?;
                            // Validate CLI version against config requirement
                            crate::config::WithVersion {
                                version: cfg.version.clone(),
                            }
                            .validate(env!("CARGO_PKG_VERSION"))?;
                            #[cfg(any(feature = "sub+postgres", feature = "sub+sqlite"))]
                            let surrealdb_cfg = match cfg.subsystem {
                                | crate::config::Subsystem::Surrealdb(c) => c,
                                | _ => anyhow::bail!("config is not surrealdb"),
                            };
                            #[cfg(not(any(feature = "sub+postgres", feature = "sub+sqlite")))]
                            let surrealdb_cfg = match cfg.subsystem {
                                | crate::config::Subsystem::Surrealdb(c) => c,
                            };
                            let surrealdb_cmd = if let Some(_) = surrealdb_subc.subcommand_matches("init") {
                                crate::subsystem::surrealdb::commands::Command::Init
                            } else if let Some(new_subc) = surrealdb_subc.subcommand_matches("new") {
                                crate::subsystem::surrealdb::commands::Command::New {
                                    comment: new_subc.get_one::<String>("comment").cloned(),
                                    locked: new_subc.get_flag("locked"),
                                }
                            } else if let Some(up_subc) = surrealdb_subc.subcommand_matches("up") {
                                crate::subsystem::surrealdb::commands::Command::Up {
                                    timeout: up_subc.get_one::<String>("timeout").map(|s| s.parse::<u64>().unwrap()),
                                    count: up_subc.get_one::<String>("count").map(|s| s.parse::<usize>().unwrap()),
                                    diff: up_subc.get_flag("diff"),
                                    dry: up_subc.get_flag("dry"),
                                    yes: up_subc.get_flag("yes"),
                                }
                            } else if let Some(down_subc) = surrealdb_subc.subcommand_matches("down") {
                                crate::subsystem::surrealdb::commands::Command::Down {
                                    timeout: down_subc
                                        .get_one::<String>("timeout")
                                        .map(|s| s.parse::<u64>().unwrap()),
                                    count: down_subc.get_one::<String>("count").unwrap().parse::<usize>().unwrap(),
                                    remote: down_subc.get_flag("remote"),
                                    diff: down_subc.get_flag("diff"),
                                    dry: down_subc.get_flag("dry"),
                                    yes: down_subc.get_flag("yes"),
                                    unlock: down_subc.get_flag("unlock"),
                                }
                            } else if let Some(list_subc) = surrealdb_subc.subcommand_matches("list") {
                                let out = match list_subc
                                    .get_one::<String>("output")
                                    .map(|s| s.as_str())
                                    .unwrap_or("human")
                                {
                                    | "human" => crate::subsystem::surrealdb::commands::Output::Human,
                                    | "json" => crate::subsystem::surrealdb::commands::Output::Json,
                                    | _ => crate::subsystem::surrealdb::commands::Output::Human,
                                };
                                crate::subsystem::surrealdb::commands::Command::List { output: out }
                            } else if let Some(history_subc) = surrealdb_subc.subcommand_matches("history") {
                                let history_cmd = if let Some(_) = history_subc.subcommand_matches("sync") {
                                    crate::subsystem::surrealdb::commands::HistoryCommand::Sync
                                } else if let Some(_) = history_subc.subcommand_matches("fix") {
                                    crate::subsystem::surrealdb::commands::HistoryCommand::Fix
                                } else {
                                    unreachable!();
                                };
                                crate::subsystem::surrealdb::commands::Command::History(history_cmd)
                            } else if let Some(_) = surrealdb_subc.subcommand_matches("diff") {
                                crate::subsystem::surrealdb::commands::Command::Diff
                            } else if let Some(apply_subc) = surrealdb_subc.subcommand_matches("apply") {
                                if let Some(up_subc) = apply_subc.subcommand_matches("up") {
                                    crate::subsystem::surrealdb::commands::Command::Apply(
                                        crate::subsystem::surrealdb::commands::MigrationApply::Up {
                                            id: up_subc.get_one::<String>("id").unwrap().clone(),
                                            timeout: up_subc
                                                .get_one::<String>("timeout")
                                                .map(|s| s.parse::<u64>().unwrap()),
                                            dry: up_subc.get_flag("dry"),
                                            yes: up_subc.get_flag("yes"),
                                        },
                                    )
                                } else if let Some(down_subc) = apply_subc.subcommand_matches("down") {
                                    crate::subsystem::surrealdb::commands::Command::Apply(
                                        crate::subsystem::surrealdb::commands::MigrationApply::Down {
                                            id: down_subc.get_one::<String>("id").unwrap().clone(),
                                            timeout: down_subc
                                                .get_one::<String>("timeout")
                                                .map(|s| s.parse::<u64>().unwrap()),
                                            remote: down_subc.get_flag("remote"),
                                            dry: down_subc.get_flag("dry"),
                                            yes: down_subc.get_flag("yes"),
                                            unlock: down_subc.get_flag("unlock"),
                                        },
                                    )
                                } else {
                                    unreachable!();
                                }
                            } else {
                                unreachable!();
                            };
                            (surrealdb_cfg, surrealdb_cmd)
                        };
                    return Ok(CallArgs {
                        privileges,
                        command: Command::Subsystem(Subsystem::Surrealdb {
                            path,
                            config: surrealdb_cfg,
                            command: surrealdb_cmd,
                        }),
                    });
                }
            }
            return Err(anyhow::anyhow!("subsystem required"));
        } else {
            anyhow::bail!("unknown command")
        };

        let callargs = CallArgs {
            privileges,
            command: cmd,
        };

        callargs.validate()?;
        Ok(callargs)
    }
}
