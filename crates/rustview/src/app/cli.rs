//! The command line of the `rustview` binary (parsed with `clap`): no arguments start the application, `api ...` runs the headless
//! API for other tools, `check` runs the checks of Settings > Debug, `--migrate-xnview` runs the `XnView` MP database migration.

use clap::{Parser, Subcommand};
use rustview_core::cli_api::ApiSubcommand;
use rustview_core::xnviewdb_migrate::MigrateOptions;
use std::ffi::OsString;

#[derive(Debug, Parser)]
#[command(
    name = "rustview",
    version,
    about = "RustView - a fast media browser for images and videos",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(flatten)]
    pub migrate: MigrateOptions,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Headless command line API for external tools (works without starting the user interface)
    Api {
        #[command(subcommand)]
        command: ApiSubcommand,
    },
    /// Checks the programs this app needs and the state of its library, as Settings > Debug does (exit status 1 on a problem)
    Check,
    /// The API commands also work without the leading `api`
    #[command(flatten)]
    Direct(ApiSubcommand),
}

impl Cli {
    /// Parses the process arguments; `--help`, `--version` and usage errors are handled (and the process ended) by `clap`.
    pub fn parse_process_args() -> Self {
        Self::parse_from(legacy_spellings(std::env::args_os()))
    }
}

/// The API was once started as `rustview --api` or `rustview --api-<command>`; both spellings are rewritten to the subcommand form.
fn legacy_spellings(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut args: Vec<OsString> = args.into_iter().collect();
    let first = args.get(1).and_then(|arg| arg.to_str()).map(str::to_owned);
    match first.as_deref() {
        Some("--api") => args[1] = "api".into(),
        Some(spelling) if spelling.starts_with("--api-") => args[1] = spelling["--api-".len()..].into(),
        _ => {}
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustview_core::cli_api::ApiCommand;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn legacy_api_spellings_become_subcommands() {
        assert_eq!(
            legacy_spellings(os(&["rustview", "--api", "tag-tree"])),
            os(&["rustview", "api", "tag-tree"])
        );
        assert_eq!(
            legacy_spellings(os(&["rustview", "--api-get-media", "--path", "/a"])),
            os(&["rustview", "get-media", "--path", "/a"])
        );
        assert_eq!(legacy_spellings(os(&["rustview", "--yes"])), os(&["rustview", "--yes"]));
        assert_eq!(legacy_spellings(os(&["rustview"])), os(&["rustview"]));
    }

    #[test]
    fn the_api_commands_work_with_and_without_the_api_word() {
        for args in [
            ["rustview", "api", "get", "/a.jpg"],
            ["rustview", "get-media", "--path", "/a.jpg"],
        ] {
            let cli = Cli::try_parse_from(args).expect("api command");
            let Some(Command::Api { command } | Command::Direct(command)) = cli.command else {
                panic!("not an API command");
            };
            assert_eq!(
                command.into_command().expect("valid command"),
                ApiCommand::GetMedia {
                    path: "/a.jpg".into(),
                    json: false
                }
            );
        }
    }

    #[test]
    fn check_runs_the_checks() {
        let cli = Cli::try_parse_from(["rustview", "check"]).expect("check command");
        assert!(matches!(cli.command, Some(Command::Check)));
    }

    #[test]
    fn no_arguments_start_the_application() {
        let cli = Cli::try_parse_from(["rustview"]).expect("plain start");
        assert!(cli.command.is_none());
        assert!(cli.migrate.xnview_db.is_none());
    }

    #[test]
    fn migration_options_keep_their_old_spellings() {
        let cli = Cli::try_parse_from([
            "rustview",
            "-m",
            "/db/XnView.db",
            "--root=/pics",
            "--no-skip-missing",
            "--plain-logs",
            "--log-file",
            "/tmp/m.log",
            "--force",
        ])
        .expect("migration options");
        assert_eq!(
            cli.migrate.xnview_db.as_deref(),
            Some(std::path::Path::new("/db/XnView.db"))
        );
        assert_eq!(cli.migrate.root.as_deref(), Some(std::path::Path::new("/pics")));
        assert!(cli.migrate.include_missing && cli.migrate.no_obfuscate_logs && cli.migrate.yes);
        assert_eq!(
            cli.migrate.log_file.as_deref(),
            Some(std::path::Path::new("/tmp/m.log"))
        );
        // Options of the migration without the migration itself are refused.
        assert!(Cli::try_parse_from(["rustview", "--yes"]).is_err());
    }

    #[test]
    fn later_flags_win_like_before() {
        let cli = Cli::try_parse_from([
            "rustview",
            "-m",
            "/db",
            "--include-missing",
            "--skip-missing",
            "--obfuscate-logs",
            "--plain-logs",
        ])
        .expect("flags");
        assert!(!cli.migrate.include_missing);
        assert!(!cli.migrate.obfuscate_logs && cli.migrate.no_obfuscate_logs);
    }

    #[test]
    fn the_command_line_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }
}
