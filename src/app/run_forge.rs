use super::*;

pub(super) struct ForgeServerLaunch {
    pub(super) artifact: PathBuf,
    pub(super) args: Vec<String>,
}

pub(super) struct InstallerRuntime<'a> {
    config: &'a Config,
    instance: &'a Instance,
    root: &'a Path,
    lock_path: &'a Path,
    dirs: &'a StateDirs,
}

impl<'a> InstallerRuntime<'a> {
    pub(super) fn new(
        config: &'a Config,
        instance: &'a Instance,
        root: &'a Path,
        lock_path: &'a Path,
        dirs: &'a StateDirs,
    ) -> Self {
        Self {
            config,
            instance,
            root,
            lock_path,
            dirs,
        }
    }

    pub(super) fn prepare_server_launch(
        &self,
        game_dir: &Path,
        java: &Path,
    ) -> Result<Option<ForgeServerLaunch>, String> {
        prepare_forge_server_launch(
            self.config,
            self.instance,
            self.root,
            self.lock_path,
            self.dirs,
            game_dir,
            java,
        )
    }

    pub(super) fn prepare_client_artifact(
        &self,
        game_dir: &Path,
        java: &Path,
        minecraft_artifact: &Path,
    ) -> Result<Option<PathBuf>, String> {
        prepare_forge_client_artifact(
            self.config,
            self.instance,
            self.lock_path,
            self.dirs,
            game_dir,
            java,
            minecraft_artifact,
        )
    }
}

pub(super) fn prepare_forge_client_artifact(
    config: &Config,
    instance: &Instance,
    lock_path: &Path,
    dirs: &StateDirs,
    game_dir: &Path,
    java: &Path,
    minecraft_artifact: &Path,
) -> Result<Option<PathBuf>, String> {
    if !matches!(instance.loader.as_str(), "forge" | "neoforge") {
        return Ok(None);
    }

    let Some(installer_maven) = locked_value(lock_path, &instance.name, "installer_maven")? else {
        return Ok(None);
    };
    let coordinates = MavenCoordinates::parse_coordinate(&installer_maven)
        .ok_or_else(|| format!("invalid Forge installer coordinate `{installer_maven}`"))?;
    let cache_dir = dirs.cache.join("downloads");
    let patched = cache_dir.join("mojang").join("libraries").join(format!(
        "{}-{}-client.jar",
        coordinates.artifact, coordinates.version
    ));
    if patched.is_file() {
        return Ok(Some(patched));
    }

    let repositories = repositories_with_builtins(&config.repositories);
    let maven_cache = cache_dir.join("maven");
    let installer = resolve_maven_artifact(&repositories, &coordinates, &maven_cache)?
        .ok_or_else(|| format!("failed to resolve Forge installer `{installer_maven}`"))?;
    let Some(profile) = jar_entry_text(&installer.path, &["install_profile.json"])? else {
        return Ok(None);
    };
    if !has_client_processor(&profile)? {
        return Ok(None);
    }
    let library_dir = cache_dir.join("mojang").join("libraries");
    let binpatch = cache_dir.join("mojang").join("forge").join(format!(
        "{}-{}-client.lzma",
        coordinates.artifact, coordinates.version
    ));
    if !binpatch.is_file() {
        let bytes =
            jar_entry_bytes(&installer.path, &["data/client.lzma", "/data/client.lzma"])?
                .ok_or_else(|| "Forge installer did not contain data/client.lzma".to_string())?;
        if let Some(parent) = binpatch.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::write(&binpatch, bytes)
            .map_err(|error| format!("failed to write {}: {error}", binpatch.display()))?;
    }
    let patched = profile_data_client_value(&profile, "PATCHED")
        .and_then(|value| profile_data_path(&value, &library_dir).ok())
        .unwrap_or_else(|| {
            library_dir.join(format!(
                "{}-{}-client.jar",
                coordinates.artifact, coordinates.version
            ))
        });
    if patched.is_file() {
        ensure_neoforge_patched_manifest(&instance.loader, java, &patched)?;
        return Ok(Some(patched));
    }
    if let Some(parent) = patched.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    let data = processor_data(ProcessorPaths {
        profile: &profile,
        library_dir: &library_dir,
        game_dir,
        minecraft_artifact,
        patched: &patched,
        binpatch: &binpatch,
        installer: &installer.path,
        minecraft_version: &instance.minecraft,
    })?;
    // Run every client-side processor in order. Obfuscated lines chain several: 1.21.11 downloads the
    // Mojang mappings, renames the jar with them, and only then patches it.
    for planned in client_processor_plan(&profile, &data)? {
        let processor_path = resolve_processor_artifact(&repositories, &maven_cache, &planned.jar)?;
        let mut classpath = Vec::new();
        for coordinate in &planned.classpath {
            classpath.push(resolve_processor_artifact(
                &repositories,
                &maven_cache,
                coordinate,
            )?);
        }
        classpath.push(processor_path.clone());
        let main_class = jar_manifest_main_class(&processor_path)?;
        let mut args = Vec::new();
        for arg in planned.args {
            args.push(match arg {
                ProcessorArg::Literal(value) => value,
                ProcessorArg::Artifact(coordinate) => {
                    resolve_processor_artifact(&repositories, &maven_cache, &coordinate)?
                        .display()
                        .to_string()
                }
            });
        }
        let mut processor_command = Command::new(java);
        processor_command
            .arg("-cp")
            .arg(join_classpath(&classpath))
            .arg(main_class)
            .args(args);
        let processor_output =
            run_process_with_timeout(&mut processor_command, None).map_err(|error| {
                format!(
                    "failed to run Forge client processor {} ({}): {error}",
                    planned.jar,
                    java.display()
                )
            })?;
        print!("{}", String::from_utf8_lossy(&processor_output.stdout));
        eprint!("{}", String::from_utf8_lossy(&processor_output.stderr));
        if !processor_output.status.success() {
            return Err(format!(
                "Forge client processor {} failed with exit code {}",
                planned.jar,
                processor_output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
        }
    }
    if !patched.is_file() {
        return Err(format!(
            "Forge client processor did not create {}",
            patched.display()
        ));
    }
    ensure_neoforge_patched_manifest(&instance.loader, java, &patched)?;

    Ok(Some(patched))
}

pub(super) fn prepare_forge_server_launch(
    config: &Config,
    instance: &Instance,
    _root: &Path,
    lock_path: &Path,
    dirs: &StateDirs,
    game_dir: &Path,
    java: &Path,
) -> Result<Option<ForgeServerLaunch>, String> {
    if !matches!(instance.loader.as_str(), "forge" | "neoforge") {
        return Ok(None);
    }

    let Some(installer_maven) = locked_value(lock_path, &instance.name, "installer_maven")? else {
        return Ok(None);
    };
    let coordinates = MavenCoordinates::parse_coordinate(&installer_maven)
        .ok_or_else(|| format!("invalid Forge installer coordinate `{installer_maven}`"))?;
    let marker = game_dir.join(".modstage-installed");
    if fs::read_to_string(&marker).is_ok_and(|installed| installed.trim() == installer_maven)
        && let Some(launch) =
            installed_server_launch(&instance.loader, game_dir, coordinates.version)
    {
        return Ok(Some(launch));
    }

    let repositories = repositories_with_builtins(&config.repositories);
    let installer = resolve_maven_artifact(
        &repositories,
        &coordinates,
        &dirs.cache.join("downloads").join("maven"),
    )?
    .ok_or_else(|| format!("failed to resolve Forge installer `{installer_maven}`"))?;

    // Install from scratch: an install that didn't finish can leave truncated downloads, which
    // the installer would reuse.
    let _ = fs::remove_file(&marker);
    let libraries = game_dir.join("libraries");
    if libraries.exists() {
        fs::remove_dir_all(&libraries)
            .map_err(|error| format!("failed to clear {}: {error}", libraries.display()))?;
    }
    let mut installer_command = Command::new(java);
    installer_command
        .arg("-jar")
        .arg(&installer.path)
        .arg("--installServer")
        .arg(".")
        .current_dir(game_dir);
    let installer_output = run_process_with_timeout(&mut installer_command, None)
        .map_err(|error| format!("failed to run Forge installer {}: {error}", java.display()))?;
    print!("{}", String::from_utf8_lossy(&installer_output.stdout));
    eprint!("{}", String::from_utf8_lossy(&installer_output.stderr));

    if !installer_output.status.success() {
        return Err(format!(
            "Forge installer failed with exit code {}",
            installer_output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }
    let launch = installed_server_launch(&instance.loader, game_dir, coordinates.version)
        .ok_or_else(|| {
            format!(
                "Forge installer created neither {} nor a server shim jar",
                installer_server_args_file(&instance.loader, coordinates.version)
                    .trim_start_matches('@')
            )
        })?;
    fs::write(&marker, &installer_maven)
        .map_err(|error| format!("failed to write {}: {error}", marker.display()))?;

    Ok(Some(launch))
}

/// How an installed server starts: from the arguments file the installer writes, or, for Forge
/// 1.20.3, whose installer writes none, from the shim jar.
fn installed_server_launch(
    loader: &str,
    game_dir: &Path,
    version: &str,
) -> Option<ForgeServerLaunch> {
    let loader_args = installer_server_args_file(loader, version);
    let loader_args_path = game_dir.join(loader_args.trim_start_matches('@'));
    if loader_args_path.is_file() {
        return Some(ForgeServerLaunch {
            artifact: loader_args_path,
            args: forge_server_launch_args(game_dir, loader_args),
        });
    }
    let shim = format!("forge-{version}-shim.jar");
    game_dir.join(&shim).is_file().then(|| ForgeServerLaunch {
        artifact: game_dir.join(&shim),
        args: vec!["-jar".to_string(), shim, "nogui".to_string()],
    })
}

/// One installer processor to run for the client, with its arguments already substituted.
#[derive(Debug, PartialEq)]
struct PlannedProcessor {
    jar: String,
    classpath: Vec<String>,
    args: Vec<ProcessorArg>,
}

#[derive(Debug, PartialEq)]
enum ProcessorArg {
    Literal(String),
    /// A `[group:artifact:version]` argument, resolved from Maven when the processor runs.
    Artifact(String),
}

fn has_client_processor(profile: &str) -> Result<bool, String> {
    Ok(!client_processors(&parse_profile(profile)?).is_empty())
}

fn parse_profile(profile: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(profile)
        .map_err(|error| format!("invalid Forge install_profile.json: {error}"))
}

/// Processors without `sides`, or whose `sides` include the client, in installer order.
fn client_processors(profile: &serde_json::Value) -> Vec<&serde_json::Value> {
    profile["processors"]
        .as_array()
        .map(|processors| {
            processors
                .iter()
                .filter(|processor| match processor["sides"].as_array() {
                    Some(sides) => sides.iter().any(|side| side.as_str() == Some("client")),
                    None => true,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn client_processor_plan(
    profile: &str,
    data: &[(String, String)],
) -> Result<Vec<PlannedProcessor>, String> {
    let profile = parse_profile(profile)?;
    let strings = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut plan = Vec::new();
    for processor in client_processors(&profile) {
        let jar = processor["jar"]
            .as_str()
            .ok_or_else(|| "Forge processor has no jar".to_string())?
            .to_string();
        let mut args = Vec::new();
        for arg in strings(&processor["args"]) {
            if let Some(coordinate) = arg.strip_prefix('[').and_then(|arg| arg.strip_suffix(']')) {
                args.push(ProcessorArg::Artifact(coordinate.to_string()));
                continue;
            }
            let mut value = arg;
            for (key, replacement) in data {
                value = value.replace(&format!("{{{key}}}"), replacement);
            }
            if let Some(unknown) = unknown_placeholder(&value) {
                return Err(format!(
                    "Forge processor {jar} uses unknown installer data `{{{unknown}}}`"
                ));
            }
            args.push(ProcessorArg::Literal(value));
        }
        plan.push(PlannedProcessor {
            jar,
            classpath: strings(&processor["classpath"]),
            args,
        });
    }
    Ok(plan)
}

/// An unsubstituted `{UPPER_SNAKE}` placeholder. Running a processor with one writes to a literal path.
fn unknown_placeholder(value: &str) -> Option<&str> {
    value.split('{').skip(1).find_map(|rest| {
        let name = rest.split('}').next()?;
        (rest.contains('}')
            && !name.is_empty()
            && name.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .then_some(name)
    })
}

struct ProcessorPaths<'a> {
    profile: &'a str,
    library_dir: &'a Path,
    game_dir: &'a Path,
    minecraft_artifact: &'a Path,
    patched: &'a Path,
    binpatch: &'a Path,
    installer: &'a Path,
    minecraft_version: &'a str,
}

/// The installer's built-in placeholders plus every client value from the profile's `data`.
fn processor_data(paths: ProcessorPaths<'_>) -> Result<Vec<(String, String)>, String> {
    let mut data = vec![
        (
            "MINECRAFT_JAR".to_string(),
            paths.minecraft_artifact.display().to_string(),
        ),
        ("PATCHED".to_string(), paths.patched.display().to_string()),
        ("BINPATCH".to_string(), paths.binpatch.display().to_string()),
        ("ROOT".to_string(), paths.game_dir.display().to_string()),
        (
            "LIBRARY_DIR".to_string(),
            paths.library_dir.display().to_string(),
        ),
        (
            "INSTALLER".to_string(),
            paths.installer.display().to_string(),
        ),
        ("SIDE".to_string(), "client".to_string()),
        (
            "MINECRAFT_VERSION".to_string(),
            paths.minecraft_version.to_string(),
        ),
    ];
    let profile = parse_profile(paths.profile)?;
    if let Some(entries) = profile["data"].as_object() {
        for (key, entry) in entries {
            if data.iter().any(|(known, _)| known == key) {
                continue;
            }
            if let Some(value) = entry["client"].as_str() {
                data.push((key.clone(), profile_data_arg(value, paths.library_dir)?));
            }
        }
    }
    Ok(data)
}

fn resolve_processor_artifact(
    repositories: &[(String, String)],
    maven_cache: &Path,
    coordinate: &str,
) -> Result<PathBuf, String> {
    let coordinates = MavenCoordinates::parse_coordinate(coordinate)
        .ok_or_else(|| format!("invalid Forge processor coordinate `{coordinate}`"))?;
    resolve_maven_artifact(repositories, &coordinates, maven_cache)?
        .map(|artifact| artifact.path)
        .ok_or_else(|| format!("failed to resolve Forge processor `{coordinate}`"))
}

fn jar_manifest_main_class(jar: &Path) -> Result<String, String> {
    let manifest = jar_entry_text(jar, &["META-INF/MANIFEST.MF"])?
        .ok_or_else(|| format!("processor jar {} has no manifest", jar.display()))?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("Main-Class: ").map(str::trim))
        .map(str::to_string)
        .ok_or_else(|| format!("processor jar {} has no Main-Class", jar.display()))
}

fn profile_data_client_value(profile: &str, key: &str) -> Option<String> {
    parse_profile(profile).ok()?["data"][key]["client"]
        .as_str()
        .map(str::to_string)
}

fn profile_data_arg(value: &str, library_dir: &Path) -> Result<String, String> {
    if let Ok(path) = profile_data_path(value, library_dir) {
        return Ok(path.display().to_string());
    }
    Ok(value.trim_matches('\'').to_string())
}

fn profile_data_path(value: &str, library_dir: &Path) -> Result<PathBuf, String> {
    let coordinate = value
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .ok_or_else(|| format!("profile data value `{value}` is not a Maven path"))?;
    let coordinates = MavenCoordinates::parse_coordinate(coordinate)
        .ok_or_else(|| format!("invalid profile data Maven coordinate `{coordinate}`"))?;
    Ok(library_dir.join(coordinates.artifact_relative_path()))
}

fn ensure_neoforge_patched_manifest(
    loader: &str,
    java: &Path,
    patched: &Path,
) -> Result<(), String> {
    if loader != "neoforge" {
        return Ok(());
    }
    if jar_entry_text(patched, &["META-INF/MANIFEST.MF"])?
        .is_some_and(|manifest| manifest.contains("Minecraft-Dists: client"))
    {
        return Ok(());
    }
    let manifest = patched.with_extension("modstage-manifest.mf");
    fs::write(&manifest, "Minecraft-Dists: client\n\n")
        .map_err(|error| format!("failed to write {}: {error}", manifest.display()))?;
    let jar = java
        .parent()
        .map(|bin| bin.join(if cfg!(windows) { "jar.exe" } else { "jar" }))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("jar"));
    let status = Command::new(&jar)
        .arg("ufm")
        .arg(patched)
        .arg(&manifest)
        .status()
        .map_err(|error| format!("failed to run {}: {error}", jar.display()))?;
    let _ = fs::remove_file(&manifest);
    if !status.success() {
        return Err(format!(
            "failed to update NeoForge patched jar manifest with status {status}"
        ));
    }

    Ok(())
}

fn forge_server_launch_args(game_dir: &Path, loader_args: String) -> Vec<String> {
    let mut args = Vec::new();
    if game_dir.join("user_jvm_args.txt").is_file() {
        args.push("@user_jvm_args.txt".to_string());
    }
    args.push(loader_args);
    args.push("nogui".to_string());
    args
}

fn installer_server_args_file(loader: &str, version: &str) -> String {
    let (group_path, artifact) = match loader {
        "neoforge" => ("net/neoforged", "neoforge"),
        _ => ("net/minecraftforge", "forge"),
    };
    let file_name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    format!("@libraries/{group_path}/{artifact}/{version}/{file_name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of Forge 61.2.0's install_profile.json for 1.21.11, trimmed to what matters.
    const FORGE_1_21_11_PROFILE: &str = r#"{
      "data": {
        "MOJMAPS": { "client": "[net.minecraft:client:1.21.11:mappings@tsrg]", "server": "[net.minecraft:server:1.21.11:mappings@tsrg]" },
        "MOJMAPS_SHA": { "client": "'f9240aaa'", "server": "'x'" },
        "MC_OFF": { "client": "[net.minecraft:client:1.21.11:official]", "server": "[net.minecraft:server:1.21.11:official]" },
        "BINPATCH": { "client": "/data/client.lzma", "server": "/data/server.lzma" },
        "PATCHED": { "client": "[net.minecraftforge:forge:1.21.11-61.2.0:client]", "server": "[net.minecraftforge:forge:1.21.11-61.2.0:server]" }
      },
      "processors": [
        { "sides": ["server"], "jar": "net.minecraftforge:installertools:1.4.3", "args": ["--task", "EXTRACT_FILES", "--archive", "{INSTALLER}"] },
        { "jar": "net.minecraftforge:installertools:1.4.3", "classpath": ["net.sf.jopt-simple:jopt-simple:6.0-alpha-3"],
          "args": ["--task", "DOWNLOAD_MOJMAPS", "--version", "1.21.11", "--side", "{SIDE}", "--output", "{MOJMAPS}"] },
        { "sides": ["server"], "jar": "net.minecraftforge:ForgeAutoRenamingTool:1.0.6", "args": ["--input", "{MC_UNPACKED}"] },
        { "sides": ["client"], "jar": "net.minecraftforge:ForgeAutoRenamingTool:1.0.6",
          "args": ["--input", "{MINECRAFT_JAR}", "--output", "{MC_OFF}", "--names", "{MOJMAPS}", "--reverse"] },
        { "jar": "net.minecraftforge:binarypatcher:1.1.1",
          "args": ["--clean", "{MC_OFF}", "--output", "{PATCHED}", "--apply", "{BINPATCH}", "--extra", "[de.oceanlabs.mcp:mcp_config:1.21.11:mappings@txt]"] }
      ]
    }"#;

    fn paths<'a>(profile: &'a str, library: &'a Path) -> ProcessorPaths<'a> {
        ProcessorPaths {
            profile,
            library_dir: library,
            game_dir: Path::new("/game"),
            minecraft_artifact: Path::new("/mc/client.jar"),
            patched: Path::new("/lib/patched.jar"),
            binpatch: Path::new("/cache/client.lzma"),
            installer: Path::new("/cache/installer.jar"),
            minecraft_version: "1.21.11",
        }
    }

    #[test]
    fn obfuscated_profiles_run_every_client_processor_in_order_with_their_data() {
        let library = Path::new("/lib");
        let data =
            processor_data(paths(FORGE_1_21_11_PROFILE, library)).expect("data should build");
        let plan = client_processor_plan(FORGE_1_21_11_PROFILE, &data).expect("plan should build");

        let jars: Vec<&str> = plan.iter().map(|p| p.jar.as_str()).collect();
        assert_eq!(
            jars,
            [
                "net.minecraftforge:installertools:1.4.3",
                "net.minecraftforge:ForgeAutoRenamingTool:1.0.6",
                "net.minecraftforge:binarypatcher:1.1.1",
            ]
        );
        let mojmaps = library
            .join("net/minecraft/client/1.21.11/client-1.21.11-mappings.tsrg")
            .display()
            .to_string();
        assert!(
            plan[0]
                .args
                .contains(&ProcessorArg::Literal(mojmaps.clone())),
            "{:?}",
            plan[0].args
        );
        assert!(
            plan[0]
                .args
                .contains(&ProcessorArg::Literal("client".to_string()))
        );
        assert_eq!(
            plan[0].classpath,
            ["net.sf.jopt-simple:jopt-simple:6.0-alpha-3"]
        );
        assert!(plan[1].args.contains(&ProcessorArg::Literal(mojmaps)));
        assert!(
            plan[2]
                .args
                .contains(&ProcessorArg::Literal("/cache/client.lzma".to_string()))
        );
        assert!(
            plan[2]
                .args
                .contains(&ProcessorArg::Literal("/lib/patched.jar".to_string()))
        );
        assert!(plan[2].args.contains(&ProcessorArg::Artifact(
            "de.oceanlabs.mcp:mcp_config:1.21.11:mappings@txt".to_string()
        )));
    }

    #[test]
    fn unknown_installer_data_fails_instead_of_writing_a_literal_path() {
        let profile =
            r#"{"data": {}, "processors": [{"jar": "a:b:1", "args": ["--output", "{MOJMAPS}"]}]}"#;
        let data = processor_data(paths(profile, Path::new("/lib"))).expect("data should build");
        let error =
            client_processor_plan(profile, &data).expect_err("an unknown placeholder must fail");
        assert!(error.contains("{MOJMAPS}"), "{error}");
    }
}
