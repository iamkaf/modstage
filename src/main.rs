use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const ROOT_HELP: &str = "\
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

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
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
        [command] if command == "init" => {
            init_project()
        }
        [command] if command == "resolve" => {
            resolve_instance(invocation.config, None)
        }
        [command, instance] if command == "resolve" => {
            resolve_instance(invocation.config, Some(instance))
        }
        [command, _side, _instance, ..] if command == "run" => {
            println!("run is not implemented yet");
            Ok(())
        }
        [command, subject] if command == "inspect" && subject == "config" => {
            inspect_config(invocation.config)
        }
        [command, ..] if command == "inspect" => {
            println!("inspect is not implemented yet");
            Ok(())
        }
        [command, ..] if command == "clean" => {
            println!("clean is not implemented yet");
            Ok(())
        }
        [command, ..] if command == "java" => {
            println!("java is not implemented yet");
            Ok(())
        }
        [command, ..] => Err(format!("unknown command `{command}`\n\n{ROOT_HELP}")),
        [] => unreachable!("empty args handled above"),
    }
}

struct Invocation {
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

fn is_help(args: &[String]) -> bool {
    matches!(args, [arg] if is_help_arg(arg))
}

fn is_help_arg(arg: &str) -> bool {
    arg == "--help" || arg == "-h"
}

fn help_for(command: &str, rest: &[String]) -> Result<&'static str, String> {
    let help = match (command, rest) {
        ("init", _) => "Usage:\n  modstage init\n",
        ("resolve", _) => "Usage:\n  modstage resolve [instance]\n",
        ("run", _) => "Usage:\n  modstage run <client|server> <instance>\n",
        ("inspect", [subject, ..]) if subject == "config" => {
            "Usage:\n  modstage inspect config\n"
        }
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
        ("java", [subject, ..]) if subject == "doctor" => {
            "Usage:\n  modstage java doctor\n"
        }
        ("java", _) => "Usage:\n  modstage java <list|install|doctor>\n",
        _ => return Err(format!("unknown command `{command}`")),
    };

    Ok(help)
}

fn inspect_config(explicit_config: Option<PathBuf>) -> Result<(), String> {
    let config_path = match explicit_config {
        Some(path) => path,
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string())?,
    };

    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;

    println!("config: {}", config_path.display());
    println!("project: {project_name}");

    Ok(())
}

fn init_project() -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|error| error.to_string())?;
    let config_path = current_dir.join("modstage.toml");

    if config_path.exists() {
        return Err(format!("{} already exists", config_path.display()));
    }

    let project_name = current_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("modstage-project");
    let template = format!(
        r#"[project]
name = "{project_name}"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#
    );

    fs::write(&config_path, template)
        .map_err(|error| format!("failed to write {}: {error}", config_path.display()))?;
    println!("created {}", config_path.display());

    Ok(())
}

fn resolve_instance(explicit_config: Option<PathBuf>, selected: Option<&str>) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let config = Config::parse(&contents)?;
    let instance = match selected {
        Some(name) => config
            .instances
            .iter()
            .find(|instance| instance.name == name)
            .ok_or_else(|| format!("unknown instance `{name}`"))?,
        None => config
            .instances
            .first()
            .ok_or_else(|| "modstage.toml does not define any instances".to_string())?,
    };
    let lock_path = config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("modstage.lock");
    let lock = format!(
        "# This file is generated by modstage. Do not edit by hand.\n\
version = 1\n\
project = \"{}\"\n\
\n\
[[instance]]\n\
instance = \"{}\"\n\
minecraft = \"{}\"\n\
loader = \"{}\"\n\
sides = [{}]\n",
        config.project_name,
        instance.name,
        instance.minecraft,
        instance.loader,
        instance
            .sides
            .iter()
            .map(|side| format!("\"{side}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );

    fs::write(&lock_path, lock)
        .map_err(|error| format!("failed to write {}: {error}", lock_path.display()))?;
    println!("resolved {} into {}", instance.name, lock_path.display());

    Ok(())
}

fn config_path(explicit_config: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit_config {
        Some(path) => Ok(path),
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string()),
    }
}

struct Config {
    project_name: String,
    instances: Vec<Instance>,
}

struct Instance {
    name: String,
    minecraft: String,
    loader: String,
    sides: Vec<String>,
}

impl Config {
    fn parse(contents: &str) -> Result<Self, String> {
        let project_name = project_name(contents).ok_or_else(|| {
            "modstage.toml must contain [project] with a name".to_string()
        })?;
        let mut instances = Vec::new();
        let mut current = None;

        for line in contents.lines() {
            let line = line.trim();

            if line == "[[instance]]" {
                if let Some(instance) = current.take() {
                    instances.push(instance);
                }

                current = Some(Instance {
                    name: String::new(),
                    minecraft: String::new(),
                    loader: String::new(),
                    sides: Vec::new(),
                });
                continue;
            }

            let Some(instance) = current.as_mut() else {
                continue;
            };

            if let Some(value) = string_value(line, "name") {
                instance.name = value;
            } else if let Some(value) = string_value(line, "minecraft") {
                instance.minecraft = value;
            } else if let Some(value) = string_value(line, "loader") {
                instance.loader = value;
            } else if let Some(value) = string_array_value(line, "sides") {
                instance.sides = value;
            }
        }

        if let Some(instance) = current.take() {
            instances.push(instance);
        }

        for instance in &instances {
            if instance.name.is_empty() {
                return Err("instance is missing name".to_string());
            }
            if instance.minecraft.is_empty() {
                return Err(format!("instance `{}` is missing minecraft", instance.name));
            }
            if instance.loader.is_empty() {
                return Err(format!("instance `{}` is missing loader", instance.name));
            }
            if instance.sides.is_empty() {
                return Err(format!("instance `{}` is missing sides", instance.name));
            }
        }

        Ok(Self {
            project_name,
            instances,
        })
    }
}

fn discover_config(start: &Path) -> Result<Option<PathBuf>, String> {
    let mut current = start
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", start.display()))?;

    loop {
        let candidate = current.join("modstage.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

fn project_name(contents: &str) -> Option<String> {
    let mut in_project = false;

    for line in contents.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_project = line == "[project]";
            continue;
        }

        if !in_project {
            continue;
        }

        if let Some(value) = line.strip_prefix("name") {
            let value = value.trim_start();
            let value = value.strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }

    None
}

fn string_value(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
}

fn string_array_value(line: &str, key: &str) -> Option<Vec<String>> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    let value = value.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        value
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_string())
            .filter(|item| !item.is_empty())
            .collect(),
    )
}
