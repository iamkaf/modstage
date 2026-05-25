use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

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
        [command, side, instance, rest @ ..] if command == "run" => {
            run_instance(invocation.config, side, instance, rest)
        }
        [command, subject] if command == "inspect" && subject == "config" => {
            inspect_config(invocation.config)
        }
        [command, subject, run_id] if command == "inspect" && subject == "run" => {
            inspect_run(invocation.config, run_id)
        }
        [command, ..] if command == "inspect" => {
            println!("inspect is not implemented yet");
            Ok(())
        }
        [command, subject, instance, rest @ ..] if command == "clean" && subject == "instance" => {
            clean_instance(invocation.config, instance, rest)
        }
        [command, subject] if command == "clean" && subject == "cache" => {
            println!("clean cache is not implemented yet");
            Ok(())
        }
        [command, ..] if command == "clean" => Err("unknown clean command".to_string()),
        [command, subject] if command == "java" && subject == "list" => java_list(),
        [command, subject, rest @ ..] if command == "java" && subject == "doctor" => {
            java_doctor(rest)
        }
        [command, subject, major] if command == "java" && subject == "install" => {
            java_install(major)
        }
        [command, ..] if command == "java" => Err("unknown java command".to_string()),
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
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;

    println!("config: {}", config_path.display());
    println!("project: {project_name}");
    println!("state-id: {}", dirs.project_id);
    println!("data: {}", dirs.data.display());
    println!("cache: {}", dirs.cache.display());

    Ok(())
}

fn inspect_run(explicit_config: Option<PathBuf>, run_id: &str) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;
    let report_path = dirs
        .data
        .join("runs")
        .join(&dirs.project_id)
        .join(run_id)
        .join("run.toml");
    let report = fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read {}: {error}", report_path.display()))?;

    print!("{report}");

    Ok(())
}

fn clean_instance(
    explicit_config: Option<PathBuf>,
    instance_name: &str,
    args: &[String],
) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;
    let mut target = dirs
        .data
        .join("instances")
        .join(&dirs.project_id)
        .join(instance_name);

    if let Some(side) = parse_side_arg(args)? {
        target = target.join(side);
    }

    if target.exists() {
        fs::remove_dir_all(&target)
            .map_err(|error| format!("failed to remove {}: {error}", target.display()))?;
    }

    println!("removed {}", target.display());

    Ok(())
}

fn parse_side_arg(args: &[String]) -> Result<Option<&str>, String> {
    let mut side = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "--side" {
            side = Some(
                iter.next()
                    .ok_or_else(|| "--side requires client or server".to_string())?
                    .as_str(),
            );
        } else {
            return Err(format!("unknown clean instance option `{arg}`"));
        }
    }

    Ok(side)
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
    let config_root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let lock = format!(
        "# This file is generated by modstage. Do not edit by hand.\n\
version = 1\n\
project = \"{}\"\n\
\n\
[repositories]\n\
{}\
\n\
[[instance]]\n\
instance = \"{}\"\n\
minecraft = \"{}\"\n\
loader = \"{}\"\n\
{}\
sides = [{}]\n",
        config.project_name,
        config
            .repositories
            .iter()
            .map(|(name, url)| format!("{name} = \"{url}\"\n"))
            .collect::<String>(),
        instance.name,
        instance.minecraft,
        instance.loader,
        instance
            .loader_version
            .as_ref()
            .map(|version| format!("loader_version = \"{version}\"\n"))
            .unwrap_or_default(),
        instance
            .sides
            .iter()
            .map(|side| format!("\"{side}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let metadata = resolve_minecraft_metadata(&config, instance, config_root)?;
    let lock = if let Some(metadata) = metadata {
        format!(
            "{lock}\n[minecraft]\nversion = \"{}\"\nmanifest_url = \"{}\"\nmanifest_sha256 = \"{}\"\nversion_url = \"{}\"\nversion_sha256 = \"{}\"\njava_major = {}\nclient_url = \"{}\"\nclient_sha256 = \"{}\"\nserver_url = \"{}\"\nserver_sha256 = \"{}\"\n",
            instance.minecraft,
            metadata.manifest_url,
            metadata.manifest_sha256,
            metadata.version_url,
            metadata.version_sha256,
            metadata.java_major,
            metadata.client_url,
            metadata.client_sha256,
            metadata.server_url,
            metadata.server_sha256
        )
    } else {
        lock
    };
    let loader = resolve_loader_metadata(&config, instance, config_root)?;
    let lock = if let Some(loader) = loader {
        format!(
            "{lock}\n[loader]\nkind = \"{}\"\nversion = \"{}\"\nloader_maven = \"{}\"\nintermediary_maven = \"{}\"\nclient_main_class = \"{}\"\nserver_main_class = \"{}\"\n",
            loader.kind,
            loader.version,
            loader.loader_maven,
            loader.intermediary_maven,
            loader.client_main_class,
            loader.server_main_class
        )
    } else {
        lock
    };
    let mut lock = if instance.mods.is_empty() {
        lock
    } else {
        format!(
            "{lock}mods = [{}]\n",
            instance
                .mods
                .iter()
                .map(|item| format!("\"{item}\""))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    for source in &instance.mods {
        if let Some(path) = local_mod_path(config_root, source) {
            let path = path
                .canonicalize()
                .map_err(|error| format!("failed to resolve local mod {}: {error}", path.display()))?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("failed to read local mod {}: {error}", path.display()))?;
            lock.push_str(&format!(
                "\n[[mod]]\nsource = \"{}\"\npath = \"{}\"\nsha256 = \"{}\"\n",
                source,
                path.display(),
                sha256_hex(&bytes)
            ));
        } else if let Some(coordinates) = MavenCoordinates::parse(source) {
            let Some((repository, path)) = maven_artifact(&config.repositories, &coordinates) else {
                continue;
            };
            let path = path
                .canonicalize()
                .map_err(|error| format!("failed to resolve Maven artifact {}: {error}", path.display()))?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("failed to read Maven artifact {}: {error}", path.display()))?;
            lock.push_str(&format!(
                "\n[[mod]]\nsource = \"{}\"\nrepository = \"{}\"\npath = \"{}\"\nsha256 = \"{}\"\n",
                source,
                repository,
                path.display(),
                sha256_hex(&bytes)
            ));
        }
    }

    fs::write(&lock_path, lock)
        .map_err(|error| format!("failed to write {}: {error}", lock_path.display()))?;
    println!("resolved {} into {}", instance.name, lock_path.display());

    Ok(())
}

fn run_instance(
    explicit_config: Option<PathBuf>,
    side: &str,
    selected: &str,
    _args: &[String],
) -> Result<(), String> {
    if side != "client" && side != "server" {
        return Err(format!("unknown side `{side}`"));
    }

    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let config = Config::parse(&contents)?;
    let instance = config
        .instances
        .iter()
        .find(|instance| instance.name == selected)
        .ok_or_else(|| format!("unknown instance `{selected}`"))?;

    if !instance.sides.iter().any(|configured| configured == side) {
        return Err(format!("instance `{selected}` does not support side `{side}`"));
    }

    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let game_dir = dirs
        .data
        .join("instances")
        .join(&dirs.project_id)
        .join(&instance.name)
        .join(side)
        .join("game");
    let mods_dir = game_dir.join("mods");

    fs::create_dir_all(&mods_dir)
        .map_err(|error| format!("failed to create {}: {error}", mods_dir.display()))?;
    reconcile_mods(root, instance, &mods_dir)?;

    if side == "server" {
        fs::write(game_dir.join("eula.txt"), "eula=true\n")
            .map_err(|error| format!("failed to write server eula.txt: {error}"))?;
    }

    let run_dir = dirs
        .data
        .join("runs")
        .join(&dirs.project_id)
        .join(run_id());
    fs::create_dir_all(&run_dir)
        .map_err(|error| format!("failed to create {}: {error}", run_dir.display()))?;
    fs::write(
        run_dir.join("run.toml"),
        format!(
            "instance = \"{}\"\nside = \"{}\"\nstatus = \"staged\"\ngame_dir = \"{}\"\n",
            instance.name,
            side,
            game_dir.display()
        ),
    )
    .map_err(|error| format!("failed to write run report: {error}"))?;

    Err(format!(
        "launch is not implemented yet; staged instance at {} and report at {}",
        game_dir.display(),
        run_dir.display()
    ))
}

fn reconcile_mods(root: &Path, instance: &Instance, mods_dir: &Path) -> Result<(), String> {
    for entry in fs::read_dir(mods_dir)
        .map_err(|error| format!("failed to read {}: {error}", mods_dir.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read mod directory entry: {error}"))?
            .path();

        if path.is_file() {
            fs::remove_file(&path)
                .map_err(|error| format!("failed to remove stale mod {}: {error}", path.display()))?;
        }
    }

    for source in &instance.mods {
        let Some(path) = resolved_mod_path(root, source)? else {
            continue;
        };
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("resolved mod has no filename: {}", path.display()))?;
        fs::copy(&path, mods_dir.join(file_name))
            .map_err(|error| format!("failed to stage mod {}: {error}", path.display()))?;
    }

    Ok(())
}

fn resolved_mod_path(root: &Path, source: &str) -> Result<Option<PathBuf>, String> {
    if let Some(path) = local_mod_path(root, source) {
        return path
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("failed to resolve local mod {}: {error}", path.display()));
    }

    if let Some(coordinates) = MavenCoordinates::parse(source) {
        return Ok(maven_artifact(&[], &coordinates)
            .map(|(_, path)| path)
            .and_then(|path| path.canonicalize().ok()));
    }

    Ok(None)
}

fn run_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    format!("{millis}")
}

fn config_path(explicit_config: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit_config {
        Some(path) => Ok(path),
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string()),
    }
}

struct MinecraftMetadata {
    manifest_url: String,
    manifest_sha256: String,
    version_url: String,
    version_sha256: String,
    java_major: u32,
    client_url: String,
    client_sha256: String,
    server_url: String,
    server_sha256: String,
}

struct LoaderMetadata {
    kind: String,
    version: String,
    loader_maven: String,
    intermediary_maven: String,
    client_main_class: String,
    server_main_class: String,
}

fn resolve_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    if instance.loader != "fabric" {
        return Ok(None);
    }

    let Some(url) = fabric_meta_url() else {
        return Ok(None);
    };
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("fabric");
    let path = fetch_to_cache(&url, &cache_dir, &format!("{}-loader.json", instance.minecraft))?;
    let metadata = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;

    Ok(Some(LoaderMetadata {
        kind: "fabric".to_string(),
        version: json_string(&metadata, "version")
            .unwrap_or_else(|| instance.loader_version.clone().unwrap_or_else(|| "latest".to_string())),
        loader_maven: json_object_string(&metadata, "loader", "maven")
            .ok_or_else(|| "Fabric metadata did not include loader maven coordinate".to_string())?,
        intermediary_maven: json_object_string(&metadata, "intermediary", "maven")
            .ok_or_else(|| "Fabric metadata did not include intermediary maven coordinate".to_string())?,
        client_main_class: json_object_string(&metadata, "mainClass", "client")
            .ok_or_else(|| "Fabric metadata did not include client main class".to_string())?,
        server_main_class: json_object_string(&metadata, "mainClass", "server")
            .ok_or_else(|| "Fabric metadata did not include server main class".to_string())?,
    }))
}

fn fabric_meta_url() -> Option<String> {
    env::var("MODSTAGE_FABRIC_META_URL").ok()
}

fn resolve_minecraft_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<MinecraftMetadata>, String> {
    let Some(manifest_url) = mojang_manifest_url() else {
        return Ok(None);
    };
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("mojang");
    let manifest_path = fetch_to_cache(&manifest_url, &cache_dir, "version_manifest.json")?;
    let manifest = fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest_text = String::from_utf8_lossy(&manifest);
    let version_url = manifest_version_url(&manifest_text, &instance.minecraft)
        .ok_or_else(|| format!("Minecraft version `{}` not found in manifest", instance.minecraft))?;
    let version_path = fetch_to_cache(&version_url, &cache_dir, &format!("{}.json", instance.minecraft))?;
    let version = fs::read(&version_path)
        .map_err(|error| format!("failed to read {}: {error}", version_path.display()))?;
    let version_text = String::from_utf8_lossy(&version);
    let java_major = json_u32(&version_text, "majorVersion").unwrap_or(8);
    let client_url = json_object_string(&version_text, "client", "url")
        .ok_or_else(|| format!("Minecraft version `{}` has no client download URL", instance.minecraft))?;
    let server_url = json_object_string(&version_text, "server", "url")
        .ok_or_else(|| format!("Minecraft version `{}` has no server download URL", instance.minecraft))?;
    let client_path = fetch_to_cache(&client_url, &cache_dir, &format!("{}-client.jar", instance.minecraft))?;
    let server_path = fetch_to_cache(&server_url, &cache_dir, &format!("{}-server.jar", instance.minecraft))?;
    let client = fs::read(&client_path)
        .map_err(|error| format!("failed to read {}: {error}", client_path.display()))?;
    let server = fs::read(&server_path)
        .map_err(|error| format!("failed to read {}: {error}", server_path.display()))?;

    Ok(Some(MinecraftMetadata {
        manifest_url,
        manifest_sha256: sha256_hex(&manifest),
        version_url,
        version_sha256: sha256_hex(&version),
        java_major,
        client_url,
        client_sha256: sha256_hex(&client),
        server_url,
        server_sha256: sha256_hex(&server),
    }))
}

fn mojang_manifest_url() -> Option<String> {
    env::var("MODSTAGE_MOJANG_MANIFEST_URL").ok()
}

fn fetch_to_cache(url: &str, cache_dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_dir)
        .map_err(|error| format!("failed to create {}: {error}", cache_dir.display()))?;
    let destination = cache_dir.join(file_name);

    if let Some(path) = url.strip_prefix("file://") {
        fs::copy(path, &destination)
            .map_err(|error| format!("failed to copy {url} to {}: {error}", destination.display()))?;
        return Ok(destination);
    }

    if url.starts_with("https://") || url.starts_with("http://") {
        let status = Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--output"])
            .arg(&destination)
            .arg(url)
            .status()
            .map_err(|error| format!("failed to run curl for {url}: {error}"))?;

        if status.success() {
            return Ok(destination);
        }

        return Err(format!("curl failed for {url} with status {status}"));
    }

    Err(format!("unsupported URL `{url}`"))
}

fn manifest_version_url(manifest: &str, version: &str) -> Option<String> {
    let id_key = manifest.find(&format!("\"{version}\""))?;
    let before_id = &manifest[..id_key];
    let id_field = before_id.rfind("\"id\"")?;
    let after_version = &manifest[id_field..];
    json_string(after_version, "url")
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_object_string(text: &str, object_key: &str, value_key: &str) -> Option<String> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    json_string(&text[object_start..], value_key)
}

fn json_u32(text: &str, key: &str) -> Option<u32> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}

struct Config {
    project_name: String,
    repositories: Vec<(String, String)>,
    instances: Vec<Instance>,
}

struct Instance {
    name: String,
    minecraft: String,
    loader: String,
    loader_version: Option<String>,
    sides: Vec<String>,
    mods: Vec<String>,
}

impl Config {
    fn parse(contents: &str) -> Result<Self, String> {
        let project_name = project_name(contents).ok_or_else(|| {
            "modstage.toml must contain [project] with a name".to_string()
        })?;
        let mut section = "";
        let mut repositories = Vec::new();
        let mut instances = Vec::new();
        let mut current: Option<Instance> = None;
        let mut multiline_array: Option<(String, Vec<String>)> = None;

        for line in contents.lines() {
            let line = line.trim();

            if let Some((key, values)) = multiline_array.as_mut() {
                if line == "]" {
                    if key == "mods"
                        && let Some(instance) = current.as_mut()
                    {
                        instance.mods = values.clone();
                    }
                    multiline_array = None;
                    continue;
                }

                values.push(line.trim_end_matches(',').trim_matches('"').to_string());
                continue;
            }

            if line == "[repositories]" {
                section = "repositories";
                continue;
            }

            if line == "[[instance]]" {
                if let Some(instance) = current.take() {
                    instances.push(instance);
                }

                section = "instance";
                current = Some(Instance {
                    name: String::new(),
                    minecraft: String::new(),
                    loader: String::new(),
                    loader_version: None,
                    sides: Vec::new(),
                    mods: Vec::new(),
                });
                continue;
            }

            if line.starts_with('[') {
                section = "";
                continue;
            }

            if section == "repositories" {
                if let Some((name, url)) = key_value(line) {
                    repositories.push((name, url));
                }
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
            } else if let Some(value) = string_value(line, "loader_version") {
                instance.loader_version = Some(value);
            } else if let Some(value) = string_array_value(line, "sides") {
                instance.sides = value;
            } else if let Some(value) = string_array_value(line, "mods") {
                instance.mods = value;
            } else if line == "mods = [" {
                multiline_array = Some(("mods".to_string(), Vec::new()));
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
            repositories,
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

fn key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_string(), value.trim().trim_matches('"').to_string()))
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

fn local_mod_path(root: &Path, source: &str) -> Option<PathBuf> {
    if source.starts_with("maven:") || source.starts_with("modrinth:") {
        return None;
    }

    let path = PathBuf::from(source);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

struct MavenCoordinates<'a> {
    group: &'a str,
    artifact: &'a str,
    version: &'a str,
}

impl<'a> MavenCoordinates<'a> {
    fn parse(source: &'a str) -> Option<Self> {
        let source = source.strip_prefix("maven:")?;
        let mut parts = source.split(':');
        let group = parts.next()?;
        let artifact = parts.next()?;
        let version = parts.next()?;

        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            group,
            artifact,
            version,
        })
    }
}

fn maven_artifact(
    repositories: &[(String, String)],
    coordinates: &MavenCoordinates<'_>,
) -> Option<(String, PathBuf)> {
    for (name, url) in repositories {
        if url == "mavenLocal" {
            if let Some(path) = maven_local_artifact(coordinates) {
                return Some((name.clone(), path));
            }
        } else if let Some(root) = url.strip_prefix("file://")
            && let Some(path) = maven_artifact_under(PathBuf::from(root), coordinates)
        {
            return Some((name.clone(), path));
        }
    }

    maven_local_artifact(coordinates).map(|path| ("mavenLocal".to_string(), path))
}

fn maven_local_artifact(coordinates: &MavenCoordinates<'_>) -> Option<PathBuf> {
    maven_artifact_under(maven_local_root()?, coordinates)
}

fn maven_artifact_under(
    mut path: PathBuf,
    coordinates: &MavenCoordinates<'_>,
) -> Option<PathBuf> {

    for segment in coordinates.group.split('.') {
        path.push(segment);
    }

    path.push(coordinates.artifact);
    path.push(coordinates.version);
    path.push(format!(
        "{}-{}.jar",
        coordinates.artifact, coordinates.version
    ));

    path.is_file().then_some(path)
}

fn maven_local_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MODSTAGE_MAVEN_LOCAL") {
        return Some(PathBuf::from(path));
    }

    Some(home_dir().ok()?.join(".m2").join("repository"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut state = [
        0x6a09e667_u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = bytes.to_vec();
    let bit_len = (message.len() as u64) * 8;
    message.push(0x80);

    while (message.len() % 64) != 56 {
        message.push(0);
    }

    message.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in message.chunks_exact(64) {
        sha256_compress(&mut state, chunk);
    }

    state
        .iter()
        .map(|word| format!("{word:08x}"))
        .collect::<String>()
}

fn sha256_compress(state: &mut [u32; 8], chunk: &[u8]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut w = [0_u32; 64];

    for (i, word) in w.iter_mut().take(16).enumerate() {
        let start = i * 4;
        *word = u32::from_be_bytes([
            chunk[start],
            chunk[start + 1],
            chunk[start + 2],
            chunk[start + 3],
        ]);
    }

    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut a = state[0];
    let mut b = state[1];
    let mut c = state[2];
    let mut d = state[3];
    let mut e = state[4];
    let mut f = state[5];
    let mut g = state[6];
    let mut h = state[7];

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

fn java_list() -> Result<(), String> {
    let runtimes = discover_java_runtimes();

    if runtimes.is_empty() {
        return Err("no Java runtimes found on PATH".to_string());
    }

    for runtime in runtimes {
        let info = inspect_java(&runtime)?;
        print_java_info(&runtime, &info);
    }

    Ok(())
}

fn java_doctor(args: &[String]) -> Result<(), String> {
    let java = parse_java_arg(args)?.unwrap_or_else(|| PathBuf::from(java_bin()));
    let info = inspect_java(&java)?;

    print_java_info(&java, &info);

    Ok(())
}

fn java_install(major: &str) -> Result<(), String> {
    let major: u32 = major
        .parse()
        .map_err(|error| format!("invalid Java major version `{major}`: {error}"))?;
    let metadata_url = azul_metadata_url(major)?;
    let cache_dir = cache_home()?.join("modstage").join("downloads").join("java");
    let metadata_path = fetch_to_cache(&metadata_url, &cache_dir, &format!("azul-{major}.json"))?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let download_url = json_string(&metadata, "download_url")
        .ok_or_else(|| "Azul metadata did not include download_url".to_string())?;
    let archive_name = json_string(&metadata, "name")
        .unwrap_or_else(|| format!("zulu-java-{major}.zip"));
    let archive_path = fetch_to_cache(&download_url, &cache_dir, &archive_name)?;
    let archive = fs::read(&archive_path)
        .map_err(|error| format!("failed to read {}: {error}", archive_path.display()))?;

    println!("java major: {major}");
    println!("java archive: {}", archive_path.display());
    println!("sha256: {}", sha256_hex(&archive));

    Ok(())
}

fn azul_metadata_url(major: u32) -> Result<String, String> {
    if let Ok(url) = env::var("MODSTAGE_AZUL_METADATA_URL") {
        return Ok(url);
    }

    Ok(format!(
        "https://api.azul.com/metadata/v1/zulu/packages?arch={}&java_version={major}&os={}&archive_type=zip&javafx_bundled=false&java_package_type=jre&page_size=1",
        env::consts::ARCH,
        env::consts::OS
    ))
}

fn parse_java_arg(args: &[String]) -> Result<Option<PathBuf>, String> {
    let mut java = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "--java" {
            let path = iter
                .next()
                .ok_or_else(|| "--java requires a path".to_string())?;
            java = Some(PathBuf::from(path));
        } else {
            return Err(format!("unknown java doctor option `{arg}`"));
        }
    }

    Ok(java)
}

fn discover_java_runtimes() -> Vec<PathBuf> {
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };
    let mut runtimes = Vec::new();

    for dir in env::split_paths(&path) {
        let candidate = dir.join(java_bin());
        if candidate.is_file() && !runtimes.contains(&candidate) {
            runtimes.push(candidate);
        }
    }

    runtimes
}

fn inspect_java(java: &Path) -> Result<JavaInfo, String> {
    let output = Command::new(java)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .map_err(|error| format!("failed to run {}: {error}", java.display()))?;

    if !output.status.success() {
        return Err(format!(
            "{} failed Java validation with status {}",
            java.display(),
            output.status
        ));
    }

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = property(&text, "java.version")
        .ok_or_else(|| format!("{} did not report java.version", java.display()))?;
    let arch = property(&text, "os.arch")
        .ok_or_else(|| format!("{} did not report os.arch", java.display()))?;
    let major = java_major(&version)?;

    Ok(JavaInfo {
        version,
        major,
        arch,
    })
}

fn property(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };

        if name.trim() == key {
            return Some(value.trim().to_string());
        }
    }

    None
}

fn java_major(version: &str) -> Result<u32, String> {
    let mut parts = version.split('.');
    let first = parts
        .next()
        .ok_or_else(|| format!("invalid Java version `{version}`"))?;
    let major = if first == "1" {
        parts
            .next()
            .ok_or_else(|| format!("invalid Java version `{version}`"))?
    } else {
        first
    };
    let major = major
        .split_once('-')
        .map_or(major, |(before_dash, _)| before_dash);

    major
        .parse()
        .map_err(|error| format!("invalid Java version `{version}`: {error}"))
}

fn print_java_info(java: &Path, info: &JavaInfo) {
    println!("java: {}", java.display());
    println!("version: {}", info.version);
    println!("major: {}", info.major);
    println!("arch: {}", info.arch);
}

struct JavaInfo {
    version: String,
    major: u32,
    arch: String,
}

fn java_bin() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}

struct StateDirs {
    project_id: String,
    data: PathBuf,
    cache: PathBuf,
}

impl StateDirs {
    fn for_project(project_name: &str, root: &Path) -> Result<Self, String> {
        let project_id = format!("{}-{:08x}", project_name, stable_hash(&root.display().to_string()));
        let data = data_home()?.join("modstage");
        let cache = cache_home()?.join("modstage");

        Ok(Self {
            project_id,
            data,
            cache,
        })
    }
}

#[cfg(target_os = "linux")]
fn data_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".local").join("share"))
}

#[cfg(target_os = "linux")]
fn cache_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".cache"))
}

#[cfg(target_os = "macos")]
fn data_home() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library").join("Application Support"))
}

#[cfg(target_os = "macos")]
fn cache_home() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library").join("Caches"))
}

#[cfg(target_os = "windows")]
fn data_home() -> Result<PathBuf, String> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "APPDATA is not set".to_string())
}

#[cfg(target_os = "windows")]
fn cache_home() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(|path| PathBuf::from(path).join("Cache"))
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn stable_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5_u32;

    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }

    hash
}
