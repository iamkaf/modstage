use super::*;

pub(super) struct LaunchRequest<'a> {
    pub(super) config: &'a Config,
    pub(super) instance: &'a Instance,
    pub(super) side: &'a str,
    pub(super) root: &'a Path,
    pub(super) lock_path: &'a Path,
    pub(super) game_dir: &'a Path,
    pub(super) run_dir: &'a Path,
    pub(super) artifact_url: &'a str,
    pub(super) options: &'a RunOptions,
}

pub(super) fn launch_minecraft_instance(request: LaunchRequest<'_>) -> Result<RunResult, String> {
    let LaunchRequest {
        config,
        instance,
        side,
        root,
        lock_path,
        game_dir,
        run_dir,
        artifact_url,
        options,
    } = request;
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("mojang");
    // One file per version, so runs of different versions can't overwrite each other's jar.
    let artifact_name = format!("{}-{side}.jar", instance.minecraft);
    let cached = cache_dir.join(&artifact_name);
    let artifact = if locked_artifact_is_cached(lock_path, &instance.name, side, &cached)? {
        cached
    } else {
        fetch_to_cache(artifact_url, &cache_dir, &artifact_name)?
    };
    verify_locked_artifact_hash(lock_path, &instance.name, side, &artifact)?;
    let main_class = locked_main_class(lock_path, &instance.name, side)?;
    let java = selected_java(lock_path, instance, options)?;
    let installer_runtime = InstallerRuntime::new(config, instance, root, lock_path, &dirs);
    let mut command = Command::new(&java);
    let mut launch_plan = LaunchPlanBuilder::new();
    let mut launch_artifact = artifact.clone();
    if side == "server"
        && let Some(forge_launch) = installer_runtime.prepare_server_launch(game_dir, &java)?
    {
        launch_artifact = forge_launch.artifact;
        for arg in forge_launch.args {
            launch_plan.arg(&mut command, arg);
        }
    } else if let Some(mut main_class) = main_class {
        if side == "server" {
            launch_artifact = loader_server_artifact(&artifact, &cache_dir)?;
            // Vanilla locks only record the version's client main class. The server jar
            // names its own entry point.
            if locked_side_main_class(lock_path, &instance.name, side)?.is_none()
                && let Some(server_main_class) = jar_main_class(&launch_artifact)?
            {
                main_class = server_main_class;
            }
        }
        let libraries = restore_locked_libraries(
            lock_path,
            &instance.name,
            &cache_dir.join("libraries"),
            side,
        )?;
        // Loader arguments name the vanilla jar after the version, as the ignore list does.
        let version_name = launch_artifact
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("minecraft")
            .to_string();
        for arg in locked_arguments(lock_path, &instance.name, "jvm")? {
            let arg = expand_launch_argument(&arg, &cache_dir, game_dir, &version_name);
            launch_plan.arg(&mut command, arg);
        }
        // FML looks for its libraries relative to the working directory unless told otherwise. The
        // vanilla launcher runs from the directory that holds them; Modstage keeps them in its cache.
        if side == "client"
            && matches!(instance.loader.as_str(), "forge" | "neoforge")
            && !launch_plan
                .args()
                .iter()
                .any(|arg| arg.starts_with("-DlibraryDirectory="))
        {
            launch_plan.arg(
                &mut command,
                format!(
                    "-DlibraryDirectory={}",
                    cache_dir.join("libraries").display()
                ),
            );
        }
        if side == "client" && !libraries.natives.is_empty() {
            let natives_dir = extract_natives(&libraries.natives, &game_dir.join("natives"))?;
            launch_plan.arg(
                &mut command,
                format!("-Djava.library.path={}", natives_dir.display()),
            );
        }
        // Run the installer's client processors. The jars they generate sit in the libraries
        // directory: on the classpath where the profile lists them, otherwise found by the loader.
        if side == "client" {
            installer_runtime.prepare_client_artifact(game_dir, &java, &launch_artifact)?;
        }
        let mut classpath = vec![launch_artifact.clone()];
        classpath.extend(libraries.classpath);
        let classpath = join_classpath(&classpath);
        launch_plan.arg_pair(&mut command, "-cp", classpath);
        launch_plan.arg(&mut command, main_class);
        let game_args = launch_game_arguments(
            instance,
            side,
            locked_arguments(lock_path, &instance.name, "game")?,
        )
        .into_iter()
        .map(|arg| expand_launch_argument(&arg, &cache_dir, game_dir, &version_name))
        .collect::<Vec<_>>();
        let has_nogui = game_args.iter().any(|arg| arg == "nogui");
        for arg in game_args {
            launch_plan.arg(&mut command, arg);
        }
        if side == "client" {
            append_client_argument(
                &mut command,
                &mut launch_plan,
                "--version",
                &instance.minecraft,
            );
            append_client_argument(&mut command, &mut launch_plan, "--accessToken", "0");
            append_client_argument(&mut command, &mut launch_plan, "--username", "Player");
            append_client_argument(
                &mut command,
                &mut launch_plan,
                "--uuid",
                "00000000000000000000000000000000",
            );
            append_client_argument(&mut command, &mut launch_plan, "--userType", "legacy");
            append_client_argument(
                &mut command,
                &mut launch_plan,
                "--gameDir",
                &game_dir.display().to_string(),
            );
        }
        if side == "client"
            && let Some(address) = &options.join
        {
            for arg in join_arguments(&instance.minecraft, address)? {
                launch_plan.arg(&mut command, arg);
            }
        }
        if side == "server" && !has_nogui {
            launch_plan.arg(&mut command, "nogui");
        }
        if side == "client"
            && let Some(asset_index) = locked_value(lock_path, &instance.name, "id")?
        {
            launch_plan.arg_pair(&mut command, "--assetIndex", asset_index);
        }
        if side == "client" && locked_value(lock_path, &instance.name, "index_url")?.is_some() {
            let assets_dir = fetch_locked_assets(lock_path, &instance.name, &cache_dir)?;
            launch_plan.arg_pair(
                &mut command,
                "--assetsDir",
                assets_dir.display().to_string(),
            );
        }
    } else if side == "server" {
        launch_plan.arg_pair(&mut command, "-jar", artifact.display().to_string());
        launch_plan.arg(&mut command, "nogui");
    } else {
        launch_plan.arg_pair(&mut command, "-jar", artifact.display().to_string());
    }
    let launch_plan_path = write_launch_plan(
        instance,
        side,
        &java,
        &launch_artifact,
        run_dir,
        launch_plan.args(),
    )?;
    command.current_dir(game_dir);
    let timeout = options.timeout_duration()?;
    let run_started = SystemTime::now();
    let output = if side == "server" {
        run_server_process_with_timeout(&mut command, timeout, options.keep_alive)
    } else if side == "client" {
        run_client_process_with_timeout(&mut command, timeout, options.join.is_some())
    } else {
        run_process_with_timeout(&mut command, timeout)
    }
    .map_err(|error| format!("failed to run {}: {error}", java.display()))?;

    if !output.streamed {
        print!("{}", String::from_utf8_lossy(&output.stdout));
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }

    fs::write(run_dir.join("stdout.log"), &output.stdout)
        .map_err(|error| format!("failed to write stdout log: {error}"))?;
    fs::write(run_dir.join("stderr.log"), &output.stderr)
        .map_err(|error| format!("failed to write stderr log: {error}"))?;

    let exit_code = output.status.code();
    let timed_out = output.timed_out;
    let process_success = (output.status.success() || output.graceful_stop) && !timed_out;
    let artifacts = collect_run_artifacts(game_dir, run_dir, run_started)?;
    let failure_class = classify_failure(process_success, timed_out, &artifacts)?;
    let success = process_success && failure_class == "none";
    let status = if timed_out {
        "timed_out"
    } else if success {
        "passed"
    } else {
        "failed"
    };
    let report_path =
        RunReport::new(instance, side, game_dir, run_dir).finalize(FinalRunReport {
            status,
            java: &java,
            artifact: &artifact,
            launch_plan: &launch_plan_path,
            exit_code,
            timed_out,
            timeout: options.timeout.as_deref(),
            failure_class,
            artifacts: &artifacts,
        })?;
    print_run_summary(
        instance,
        side,
        success,
        exit_code,
        timed_out,
        failure_class,
        &report_path,
    );

    Ok(RunResult {
        success,
        exit_code,
        timed_out,
    })
}

/// The `Main-Class` a jar's manifest declares, if any.
fn jar_main_class(jar: &Path) -> Result<Option<String>, String> {
    Ok(jar_entry_text(jar, &["META-INF/MANIFEST.MF"])?
        .as_deref()
        .and_then(manifest_main_class))
}

fn manifest_main_class(manifest: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        line.strip_prefix("Main-Class:")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

pub(super) fn loader_server_artifact(artifact: &Path, cache_dir: &Path) -> Result<PathBuf, String> {
    let Some(versions_list) = jar_entry_text(artifact, &["META-INF/versions.list"])? else {
        return Ok(artifact.to_path_buf());
    };
    let Some(entry) = versions_list.lines().find_map(|line| {
        let mut columns = line.split('\t');
        let _hash = columns.next()?;
        let _version = columns.next()?;
        columns.next()
    }) else {
        return Ok(artifact.to_path_buf());
    };
    let entry = format!("META-INF/versions/{entry}");
    let Some(bytes) = jar_entry_bytes(artifact, &[&entry])? else {
        return Ok(artifact.to_path_buf());
    };
    let destination = cache_dir
        .join("versions")
        .join(entry.rsplit('/').next().unwrap_or("server.jar"));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    // Runs of the same version extract the same jar; write it whole so none reads a partial one.
    let staging = destination.with_extension(format!("jar.{}", std::process::id()));
    fs::write(&staging, bytes)
        .map_err(|error| format!("failed to write {}: {error}", staging.display()))?;
    fs::rename(&staging, &destination)
        .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;

    Ok(destination)
}

fn expand_launch_argument(
    arg: &str,
    cache_dir: &Path,
    game_dir: &Path,
    version_name: &str,
) -> String {
    let classpath_separator = if cfg!(windows) { ";" } else { ":" };
    arg.replace(
        "${library_directory}",
        &cache_dir.join("libraries").display().to_string(),
    )
    .replace("${game_directory}", &game_dir.display().to_string())
    .replace("${classpath_separator}", classpath_separator)
    .replace("${version_name}", version_name)
}

/// Extracts native libraries from `jars` into `dir`, replacing what an earlier launch left there.
fn extract_natives(jars: &[PathBuf], dir: &Path) -> Result<PathBuf, String> {
    if dir.exists() {
        fs::remove_dir_all(dir)
            .map_err(|error| format!("failed to clear {}: {error}", dir.display()))?;
    }
    fs::create_dir_all(dir)
        .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
    for jar in jars {
        let file = fs::File::open(jar)
            .map_err(|error| format!("failed to open {}: {error}", jar.display()))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|error| format!("failed to read natives jar {}: {error}", jar.display()))?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|error| {
                format!("failed to read natives jar {}: {error}", jar.display())
            })?;
            let Some(name) = entry.enclosed_name() else {
                continue;
            };
            if entry.is_dir() || name.starts_with("META-INF") {
                continue;
            }
            let destination = dir.join(name);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
            }
            let mut output = fs::File::create(&destination)
                .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
            std::io::copy(&mut entry, &mut output)
                .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;
        }
    }
    Ok(dir.to_path_buf())
}

/// Client arguments that connect straight to `address` once the game has loaded.
/// Quick Play replaced `--server` and `--port` in 1.20.
fn join_arguments(minecraft: &str, address: &str) -> Result<Vec<String>, String> {
    let (host, port) = address
        .rsplit_once(':')
        .filter(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok())
        .ok_or_else(|| format!("--join expects host:port, got `{address}`"))?;
    if minecraft == "1.17" {
        return Err(
            "--join can't work on Minecraft 1.17: it connects before resources load and crashes \
             rendering the connect screen (fixed in 1.17.1)"
                .to_string(),
        );
    }
    let mut parts = minecraft
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0));
    let has_quick_play = match (parts.next(), parts.next()) {
        (Some(1), Some(minor)) => minor >= 20,
        _ => true,
    };
    Ok(if has_quick_play {
        vec!["--quickPlayMultiplayer".to_string(), address.to_string()]
    } else {
        vec![
            "--server".to_string(),
            host.to_string(),
            "--port".to_string(),
            port.to_string(),
        ]
    })
}

fn append_client_argument(
    command: &mut Command,
    launch_plan: &mut LaunchPlanBuilder,
    key: &str,
    value: &str,
) {
    if launch_plan.contains(key) {
        return;
    }
    launch_plan.arg_pair(command, key, value);
}

pub(super) fn print_run_summary(
    instance: &Instance,
    side: &str,
    success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
    failure_class: &str,
    report_path: &Path,
) {
    println!("run summary:");
    println!("instance = \"{}\"", instance.name);
    println!("side = \"{side}\"");
    println!("minecraft = \"{}\"", instance.minecraft);
    println!("loader = \"{}\"", instance.loader);
    println!(
        "status = \"{}\"",
        if timed_out {
            "timed_out"
        } else if success {
            "passed"
        } else {
            "failed"
        }
    );
    println!("exit_code = {}", exit_code.unwrap_or(-1));
    println!("timed_out = {timed_out}");
    println!("failure_class = \"{failure_class}\"");
    println!("report = \"{}\"", report_path.display());
}

pub(super) fn selected_java(
    lock_path: &Path,
    instance: &Instance,
    options: &RunOptions,
) -> Result<PathBuf, String> {
    if let Some(java) = &options.java {
        // `--java 17` names a Java version rather than an executable.
        return match java.to_str().and_then(|value| value.parse::<u32>().ok()) {
            Some(major) => java_for_major(major, instance),
            None => Ok(java.clone()),
        };
    }

    match locked_java_major(lock_path, &instance.name)? {
        Some(major) => java_for_major(major, instance),
        None => Ok(PathBuf::from(java_bin())),
    }
}

/// The Java runtime for `major`: the managed one, the one on `PATH` if it is that exact version,
/// or a newly installed one. Like the vanilla launcher, never a newer major: old loaders bundle
/// class readers that reject newer class files.
fn java_for_major(major: u32, instance: &Instance) -> Result<PathBuf, String> {
    // Java 16 is end-of-life and no longer published; the versions that ask for it run on 17.
    let major = if major == 16 { 17 } else { major };
    if let Some(java) = managed_java_for_major(major)? {
        return Ok(java);
    }
    let system = PathBuf::from(java_bin());
    if inspect_java(&system).is_ok_and(|info| info.major == major) {
        return Ok(system);
    }
    println!(
        "installing Java {major}, which Minecraft {} needs",
        instance.minecraft
    );
    Ok(install_managed_java(major)?.java)
}

pub(super) fn launch_game_arguments(
    instance: &Instance,
    side: &str,
    args: Vec<String>,
) -> Vec<String> {
    args.into_iter()
        .map(|arg| {
            if instance.loader == "forge" && side == "server" && arg == "forge_client" {
                "forge_server".to_string()
            } else {
                arg
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{join_arguments, manifest_main_class};

    #[test]
    fn join_uses_quick_play_from_1_20_and_server_arguments_before_it() {
        assert_eq!(
            join_arguments("1.19.4", "127.0.0.1:25611").unwrap(),
            ["--server", "127.0.0.1", "--port", "25611"]
        );
        for minecraft in ["1.20", "1.21.11", "26.3"] {
            assert_eq!(
                join_arguments(minecraft, "127.0.0.1:25611").unwrap(),
                ["--quickPlayMultiplayer", "127.0.0.1:25611"]
            );
        }
        assert!(join_arguments("1.21.1", "localhost").is_err());
        assert!(join_arguments("1.17", "127.0.0.1:25611").is_err());
        assert!(join_arguments("1.17.1", "127.0.0.1:25611").is_ok());
    }

    #[test]
    fn reads_the_main_class_a_manifest_declares() {
        let manifest = "Manifest-Version: 1.0\r\nMain-Class: net.minecraft.server.Main\r\n\r\n";
        assert_eq!(
            manifest_main_class(manifest).as_deref(),
            Some("net.minecraft.server.Main")
        );
        assert_eq!(manifest_main_class("Manifest-Version: 1.0\n"), None);
    }
}
