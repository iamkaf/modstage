use super::*;
use clap::{Arg, ArgAction, ArgMatches, Command, error::ErrorKind};

pub(crate) fn run(args: Vec<String>) -> Result<(), String> {
    let matches =
        match cli().try_get_matches_from(std::iter::once("modstage".to_string()).chain(args)) {
            Ok(matches) => matches,
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) =>
            {
                print!("{error}");
                return Ok(());
            }
            Err(error) => return Err(error.to_string()),
        };
    let config = matches.get_one::<PathBuf>("config").cloned();

    match matches.subcommand() {
        Some(("init", _)) => init_project(),
        Some(("resolve", command)) => resolve_instance(
            config,
            command.get_one::<String>("instance").map(String::as_str),
        ),
        Some(("run", command)) => run_instance(
            config,
            required(command, "side")?,
            required(command, "instance")?,
            RunOptions {
                locked: command.get_flag("locked"),
                keep_alive: command.get_flag("keep_alive"),
                java: command.get_one::<PathBuf>("java").cloned(),
                timeout: command.get_one::<String>("timeout").cloned(),
            },
        ),
        Some(("inspect", command)) => match command.subcommand() {
            Some(("config", _)) => inspect_config(config),
            Some(("lock", command)) => inspect_lock(
                config,
                command.get_one::<String>("instance").map(String::as_str),
            ),
            Some(("instance", command)) => {
                let rest = rest(command);
                inspect_instance(config, required(command, "instance")?, &rest)
            }
            Some(("run", command)) => inspect_run(config, required(command, "run_id")?),
            _ => Ok(()),
        },
        Some(("clean", command)) => match command.subcommand() {
            Some(("instance", command)) => {
                let rest = rest(command);
                clean_instance(config, required(command, "instance")?, &rest)
            }
            Some(("cache", _)) => clean_cache(config),
            _ => Ok(()),
        },
        Some(("java", command)) => match command.subcommand() {
            Some(("list", _)) => java_list(),
            Some(("doctor", command)) => {
                let rest = rest(command);
                java_doctor(&rest)
            }
            Some(("install", command)) => {
                let rest = rest(command);
                java_install(required(command, "major")?, &rest)
            }
            _ => Ok(()),
        },
        _ => {
            print!("{}", cli().render_help());
            Ok(())
        }
    }
}

fn cli() -> Command {
    Command::new("modstage")
        .arg(
            Arg::new("config")
                .long("config")
                .value_name("path")
                .value_parser(clap::value_parser!(PathBuf))
                .help("Use an explicit modstage.toml")
                .global(true),
        )
        .subcommand(Command::new("init"))
        .subcommand(Command::new("resolve").arg(Arg::new("instance")))
        .subcommand(
            Command::new("run")
                .arg(
                    Arg::new("side")
                        .required(true)
                        .value_parser(["client", "server"]),
                )
                .arg(Arg::new("instance").required(true))
                .arg(
                    Arg::new("locked")
                        .long("locked")
                        .action(ArgAction::SetTrue)
                        .help("Fail if the instance lock is missing or stale"),
                )
                .arg(
                    Arg::new("keep_alive")
                        .long("keep-alive")
                        .action(ArgAction::SetTrue)
                        .help("Leave a ready server running until timeout"),
                )
                .arg(
                    Arg::new("java")
                        .long("java")
                        .value_name("path")
                        .value_parser(clap::value_parser!(PathBuf))
                        .help("Use an explicit Java executable"),
                )
                .arg(
                    Arg::new("timeout")
                        .long("timeout")
                        .value_name("duration")
                        .help("Bound the launch, for example 120s"),
                ),
        )
        .subcommand(
            Command::new("inspect")
                .subcommand(Command::new("config"))
                .subcommand(Command::new("lock").arg(Arg::new("instance")))
                .subcommand(
                    Command::new("instance")
                        .arg(Arg::new("instance").required(true))
                        .arg(rest_arg()),
                )
                .subcommand(Command::new("run").arg(Arg::new("run_id").required(true))),
        )
        .subcommand(
            Command::new("clean")
                .subcommand(
                    Command::new("instance")
                        .arg(Arg::new("instance").required(true))
                        .arg(rest_arg()),
                )
                .subcommand(Command::new("cache")),
        )
        .subcommand(
            Command::new("java")
                .subcommand(Command::new("list"))
                .subcommand(Command::new("doctor").arg(rest_arg()))
                .subcommand(
                    Command::new("install")
                        .arg(Arg::new("major").required(true))
                        .arg(rest_arg()),
                ),
        )
}

fn rest_arg() -> Arg {
    Arg::new("rest")
        .action(ArgAction::Append)
        .num_args(0..)
        .trailing_var_arg(true)
        .allow_hyphen_values(true)
}

fn required<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a str, String> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| format!("missing required argument `{name}`"))
}

fn rest(matches: &ArgMatches) -> Vec<String> {
    matches
        .get_many::<String>("rest")
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}
