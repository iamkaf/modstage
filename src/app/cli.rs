use super::*;

pub(super) const ROOT_HELP: &str = "\
modstage

Usage:
  modstage [--config <path>] <command>

Commands:
  init
  resolve [instance]
  run <client|server> <instance>
  inspect <config|lock|instance|run>
  clean <instance|cache>
  java <list|install|doctor>

Options:
  --config <path>  Use an explicit modstage.toml
  -h, --help       Show help
";

pub(crate) fn run(args: Vec<String>) -> Result<(), String> {
    let invocation = Invocation::parse(args)?;

    if invocation.args.is_empty() || is_help(&invocation.args) {
        print!("{ROOT_HELP}");
        return Ok(());
    }

    match invocation.args.as_slice() {
        [command, rest @ ..] if rest.last().is_some_and(|arg| is_help_arg(arg)) => {
            print!("{}", help_for(command, rest)?);
            Ok(())
        }
        [command] if command == "init" => init_project(),
        [command] if command == "resolve" => resolve_instance(invocation.config, None),
        [command, instance] if command == "resolve" => {
            resolve_instance(invocation.config, Some(instance))
        }
        [command, side, instance, rest @ ..] if command == "run" => {
            run_instance(invocation.config, side, instance, rest)
        }
        [command, subject] if command == "inspect" && subject == "config" => {
            inspect_config(invocation.config)
        }
        [command, subject] if command == "inspect" && subject == "lock" => {
            inspect_lock(invocation.config, None)
        }
        [command, subject, instance] if command == "inspect" && subject == "lock" => {
            inspect_lock(invocation.config, Some(instance))
        }
        [command, subject, run_id] if command == "inspect" && subject == "run" => {
            inspect_run(invocation.config, run_id)
        }
        [command, subject, instance, rest @ ..]
            if command == "inspect" && subject == "instance" =>
        {
            inspect_instance(invocation.config, instance, rest)
        }
        [command, ..] if command == "inspect" => {
            println!("inspect is not implemented yet");
            Ok(())
        }
        [command, subject, instance, rest @ ..] if command == "clean" && subject == "instance" => {
            clean_instance(invocation.config, instance, rest)
        }
        [command, subject] if command == "clean" && subject == "cache" => {
            clean_cache(invocation.config)
        }
        [command, ..] if command == "clean" => Err("unknown clean command".to_string()),
        [command, subject] if command == "java" && subject == "list" => java_list(),
        [command, subject, rest @ ..] if command == "java" && subject == "doctor" => {
            java_doctor(rest)
        }
        [command, subject, major, rest @ ..] if command == "java" && subject == "install" => {
            java_install(major, rest)
        }
        [command, ..] if command == "java" => Err("unknown java command".to_string()),
        [command, ..] => Err(format!("unknown command `{command}`\n\n{ROOT_HELP}")),
        [] => unreachable!("empty args handled above"),
    }
}

pub(super) struct Invocation {
    config: Option<PathBuf>,
    args: Vec<String>,
}

impl Invocation {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut config = None;
        let mut parsed = Vec::new();
        let mut iter = args.into_iter();

        while let Some(arg) = iter.next() {
            if arg == "--config" {
                let path = iter
                    .next()
                    .ok_or_else(|| "--config requires a path".to_string())?;
                config = Some(PathBuf::from(path));
            } else {
                parsed.push(arg);
                parsed.extend(iter);
                break;
            }
        }

        Ok(Self {
            config,
            args: parsed,
        })
    }
}

pub(super) fn is_help(args: &[String]) -> bool {
    matches!(args, [arg] if is_help_arg(arg))
}

pub(super) fn is_help_arg(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

pub(super) fn help_for(command: &str, rest: &[String]) -> Result<&'static str, String> {
    let help = match (command, rest) {
        ("init", _) => "Usage:\n  modstage init\n",
        ("resolve", _) => "Usage:\n  modstage resolve [instance]\n",
        ("run", _) => "Usage:\n  modstage run <client|server> <instance>\n",
        ("inspect", [subject, ..]) if subject == "config" => "Usage:\n  modstage inspect config\n",
        ("inspect", [subject, ..]) if subject == "lock" => {
            "Usage:\n  modstage inspect lock [instance]\n"
        }
        ("inspect", [subject, ..]) if subject == "instance" => {
            "Usage:\n  modstage inspect instance <instance> [--side <client|server>]\n"
        }
        ("inspect", [subject, ..]) if subject == "run" => {
            "Usage:\n  modstage inspect run <run-id>\n"
        }
        ("inspect", _) => "Usage:\n  modstage inspect <config|lock|instance|run>\n",
        ("clean", [subject, ..]) if subject == "instance" => {
            "Usage:\n  modstage clean instance <instance> [--side <client|server>]\n"
        }
        ("clean", [subject, ..]) if subject == "cache" => "Usage:\n  modstage clean cache\n",
        ("clean", _) => "Usage:\n  modstage clean <instance|cache>\n",
        ("java", [subject, ..]) if subject == "list" => "Usage:\n  modstage java list\n",
        ("java", [subject, ..]) if subject == "install" => {
            "Usage:\n  modstage java install <major>\n"
        }
        ("java", [subject, ..]) if subject == "doctor" => "Usage:\n  modstage java doctor\n",
        ("java", _) => "Usage:\n  modstage java <list|install|doctor>\n",
        _ => return Err(format!("unknown command `{command}`")),
    };

    Ok(help)
}
