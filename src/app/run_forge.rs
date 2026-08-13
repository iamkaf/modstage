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
            self.root,
            self.lock_path,
            self.dirs,
            game_dir,
            java,
            minecraft_artifact,
        )
    }

    pub(super) fn neoforge_client_runtime(&self) -> Result<Option<PathBuf>, String> {
        neoforge_client_runtime(self.config, self.instance, self.lock_path, self.dirs)
    }
}

pub(super) fn prepare_forge_client_artifact(
    config: &Config,
    instance: &Instance,
    _root: &Path,
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
    let Some(processor) = forge_client_processor(&profile) else {
        return Ok(None);
    };
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
    let Some(processor_coordinate) = json_string(processor, "jar") else {
        return Ok(None);
    };
    let processor_path =
        resolve_processor_artifact(&repositories, &maven_cache, &processor_coordinate)?;
    let mut classpath = Vec::new();
    for coordinate in json_string_array(processor, "classpath").unwrap_or_default() {
        classpath.push(resolve_processor_artifact(
            &repositories,
            &maven_cache,
            &coordinate,
        )?);
    }
    classpath.push(processor_path.clone());
    let main_class = jar_manifest_main_class(&processor_path)?;
    if let Some(parent) = patched.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let processor_args = forge_processor_args(ForgeProcessorArgs {
        processor,
        repositories: &repositories,
        maven_cache: &maven_cache,
        library_dir: &library_dir,
        profile: &profile,
        game_dir,
        minecraft_artifact,
        patched: &patched,
        binpatch: &binpatch,
        installer: &installer.path,
        minecraft_version: &instance.minecraft,
    })?;
    let mut processor_command = Command::new(java);
    processor_command
        .arg("-cp")
        .arg(join_classpath(&classpath))
        .arg(main_class)
        .args(processor_args);
    let processor_output =
        run_process_with_timeout(&mut processor_command, None).map_err(|error| {
            format!(
                "failed to run Forge client processor {}: {error}",
                java.display()
            )
        })?;
    print!("{}", String::from_utf8_lossy(&processor_output.stdout));
    eprint!("{}", String::from_utf8_lossy(&processor_output.stderr));
    if !processor_output.status.success() {
        return Err(format!(
            "Forge client processor failed with exit code {}",
            processor_output
                .status
                .code()
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ));
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

pub(super) fn neoforge_client_runtime(
    config: &Config,
    instance: &Instance,
    lock_path: &Path,
    dirs: &StateDirs,
) -> Result<Option<PathBuf>, String> {
    if instance.loader != "neoforge" {
        return Ok(None);
    }
    let Some(version) = neoforge_runtime_version(lock_path, instance)? else {
        return Ok(None);
    };
    let coordinate = format!("net.neoforged:neoforge:{version}:universal");
    let coordinates = MavenCoordinates::parse_coordinate(&coordinate)
        .ok_or_else(|| format!("invalid NeoForge runtime coordinate `{coordinate}`"))?;
    let repositories = repositories_with_builtins(&config.repositories);
    resolve_maven_artifact(
        &repositories,
        &coordinates,
        &dirs.cache.join("downloads").join("maven"),
    )?
    .map(|artifact| Some(artifact.path))
    .ok_or_else(|| format!("failed to resolve NeoForge runtime `{coordinate}`"))
}

fn neoforge_runtime_version(
    lock_path: &Path,
    instance: &Instance,
) -> Result<Option<String>, String> {
    Ok(
        locked_table_value(lock_path, &instance.name, "loader", "version")?
            .filter(|version| version != "latest")
            .or_else(|| {
                instance
                    .loader_version
                    .clone()
                    .filter(|version| version != "latest")
            }),
    )
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
    let loader_args = installer_server_args_file(&instance.loader, coordinates.version);
    let loader_args_path = game_dir.join(loader_args.trim_start_matches('@'));
    if loader_args_path.is_file() {
        return Ok(Some(ForgeServerLaunch {
            artifact: loader_args_path,
            args: forge_server_launch_args(game_dir, loader_args),
        }));
    }

    let repositories = repositories_with_builtins(&config.repositories);
    let installer = resolve_maven_artifact(
        &repositories,
        &coordinates,
        &dirs.cache.join("downloads").join("maven"),
    )?
    .ok_or_else(|| format!("failed to resolve Forge installer `{installer_maven}`"))?;

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

    if !loader_args_path.is_file() {
        return Err(format!(
            "Forge installer did not create {}",
            loader_args_path.display()
        ));
    }

    Ok(Some(ForgeServerLaunch {
        artifact: installer.path,
        args: forge_server_launch_args(game_dir, loader_args),
    }))
}

fn forge_client_processor(profile: &str) -> Option<&str> {
    let processors = json_object_after(profile, "processors")?;
    json_object_blocks(processors).into_iter().find(|block| {
        (!block.contains("\"sides\"") || block.contains("\"client\""))
            && json_string(block, "jar").is_some()
            && json_string_array(block, "args").is_some()
    })
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

struct ForgeProcessorArgs<'a> {
    processor: &'a str,
    repositories: &'a [(String, String)],
    maven_cache: &'a Path,
    library_dir: &'a Path,
    profile: &'a str,
    game_dir: &'a Path,
    minecraft_artifact: &'a Path,
    patched: &'a Path,
    binpatch: &'a Path,
    installer: &'a Path,
    minecraft_version: &'a str,
}

fn forge_processor_args(request: ForgeProcessorArgs<'_>) -> Result<Vec<String>, String> {
    let mut data = vec![
        (
            "MINECRAFT_JAR".to_string(),
            request.minecraft_artifact.display().to_string(),
        ),
        ("PATCHED".to_string(), request.patched.display().to_string()),
        (
            "BINPATCH".to_string(),
            request.binpatch.display().to_string(),
        ),
        ("ROOT".to_string(), request.game_dir.display().to_string()),
        (
            "LIBRARY_DIR".to_string(),
            request.library_dir.display().to_string(),
        ),
        (
            "INSTALLER".to_string(),
            request.installer.display().to_string(),
        ),
        ("SIDE".to_string(), "client".to_string()),
        (
            "MINECRAFT_VERSION".to_string(),
            request.minecraft_version.to_string(),
        ),
    ];
    for key in [
        "MC_UNPACKED",
        "MC_UNPACKED_SHA",
        "PATCHED_SHA",
        "MCP_VERSION",
    ] {
        if let Some(value) = profile_data_client_value(request.profile, key) {
            data.push((
                key.to_string(),
                profile_data_arg(&value, request.library_dir)?,
            ));
        }
    }

    let mut args = Vec::new();
    for arg in json_string_array(request.processor, "args").unwrap_or_default() {
        let arg =
            if let Some(coordinate) = arg.strip_prefix('[').and_then(|arg| arg.strip_suffix(']')) {
                resolve_processor_artifact(request.repositories, request.maven_cache, coordinate)?
                    .display()
                    .to_string()
            } else {
                let mut arg = arg;
                for (key, value) in &data {
                    arg = arg.replace(&format!("{{{key}}}"), value);
                }
                arg
            };
        args.push(arg);
    }

    Ok(args)
}

fn profile_data_client_value(profile: &str, key: &str) -> Option<String> {
    let block = json_object_after(profile, key)?;
    json_string(block, "client")
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

    #[test]
    fn neoforge_client_runtime_uses_locked_version_before_configured_latest() {
        let root = env::temp_dir().join(format!(
            "modstage-neoforge-runtime-test-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("failed to create temp root");
        let lock_path = root.join("modstage.lock");
        fs::write(
            &lock_path,
            r#"[[instance]]
instance = "neoforge-client"
loader = "neoforge"

[minecraft]
version = "26.1.2"

[loader]
version = "26.1.2.66-beta"
"#,
        )
        .expect("failed to write lockfile");
        let instance = Instance {
            name: "neoforge-client".to_string(),
            minecraft: "26.1.2".to_string(),
            loader: "neoforge".to_string(),
            loader_version: Some("latest".to_string()),
            sides: vec!["client".to_string()],
            modrinth_pack: None,
            server_properties: Vec::new(),
            mods: Vec::new(),
            fixtures: Vec::new(),
        };

        assert_eq!(
            neoforge_runtime_version(&lock_path, &instance)
                .expect("runtime version should resolve")
                .as_deref(),
            Some("26.1.2.66-beta")
        );

        fs::remove_dir_all(root).expect("failed to remove temp root");
    }
}
