use super::*;

pub(super) fn run_instance(
    explicit_config: Option<PathBuf>,
    side: &str,
    selected: &str,
    args: &[String],
) -> Result<(), String> {
    if side != "client" && side != "server" {
        return Err(format!("unknown side `{side}`"));
    }
    let options = RunOptions::parse(args)?;

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
    let lock_path = root.join("modstage.lock");
    if options.locked && !lock_path.is_file() {
        return Err("locked run requires modstage.lock; run `modstage resolve` first".to_string());
    }
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

    let artifact_key = if side == "server" { "server_url" } else { "client_url" };
    if let Some(artifact_url) = locked_minecraft_url(root, artifact_key)?
    {
        let result = launch_minecraft_instance(
            &config,
            instance,
            side,
            root,
            &game_dir,
            &run_dir,
            &artifact_url,
            &options,
        )?;

        if result.success {
            return Ok(());
        }

        if result.timed_out {
            return Err(format!("{side} run timed out"));
        }

        return Err(format!(
            "{side} run failed with exit code {}",
            result.exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }

    Err(format!(
        "launch is not implemented yet; staged instance at {} and report at {}",
        game_dir.display(),
        run_dir.display()
    ))
}

pub(super) struct RunOptions {
    locked: bool,
    java: Option<PathBuf>,
    timeout: Option<String>,
}

impl RunOptions {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut locked = false;
        let mut java = None;
        let mut timeout = None;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--locked" => {
                    locked = true;
                    index += 1;
                }
                "--java" => {
                    let Some(path) = args.get(index + 1) else {
                        return Err("--java requires a path".to_string());
                    };
                    java = Some(PathBuf::from(path));
                    index += 2;
                }
                "--timeout" => {
                    let Some(value) = args.get(index + 1) else {
                        return Err("--timeout requires a duration".to_string());
                    };
                    timeout = Some(value.clone());
                    index += 2;
                }
                option => return Err(format!("unknown run option `{option}`")),
            }
        }

        Ok(Self {
            locked,
            java,
            timeout,
        })
    }

    fn timeout_duration(&self) -> Result<Option<Duration>, String> {
        self.timeout
            .as_deref()
            .map(parse_duration)
            .transpose()
    }
}

pub(super) struct RunResult {
    success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
}

pub(super) fn launch_minecraft_instance(
    config: &Config,
    instance: &Instance,
    side: &str,
    root: &Path,
    game_dir: &Path,
    run_dir: &Path,
    artifact_url: &str,
    options: &RunOptions,
) -> Result<RunResult, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("mojang");
    let artifact_name = format!("{side}.jar");
    let artifact = fetch_to_cache(artifact_url, &cache_dir, &artifact_name)?;
    let java = options
        .java
        .clone()
        .unwrap_or_else(|| PathBuf::from(java_bin()));
    let mut command = Command::new(&java);
    if side == "client"
        && let Some(main_class) = locked_main_class(root, side)?
    {
        let mut classpath = vec![artifact.clone()];
        classpath.extend(fetch_locked_libraries(root, &cache_dir.join("libraries"))?);
        command
            .arg("-cp")
            .arg(join_classpath(&classpath))
            .arg(main_class);
        if let Some(asset_index) = locked_value(root, "id")? {
            command.arg("--assetIndex").arg(asset_index);
        }
        if locked_value(root, "index_url")?.is_some() {
            command
                .arg("--assetsDir")
                .arg(cache_dir.join("assets").join("objects"));
        }
    } else if side == "server" {
        command.arg("-jar").arg(&artifact);
        command.arg("nogui");
    } else {
        command.arg("-jar").arg(&artifact);
    }
    command.current_dir(game_dir);
    let output = run_process_with_timeout(&mut command, options.timeout_duration()?)
        .map_err(|error| format!("failed to run {}: {error}", java.display()))?;

    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));

    fs::write(run_dir.join("stdout.log"), &output.stdout)
        .map_err(|error| format!("failed to write stdout log: {error}"))?;
    fs::write(run_dir.join("stderr.log"), &output.stderr)
        .map_err(|error| format!("failed to write stderr log: {error}"))?;

    let exit_code = output.status.code();
    let timed_out = output.timed_out;
    let success = output.status.success() && !timed_out;
    let artifacts = collect_run_artifacts(game_dir, run_dir)?;
    let failure_class = if artifacts.crash_report.is_some() {
        "crash_report"
    } else if timed_out {
        "timeout"
    } else if success {
        "none"
    } else {
        "process_exit"
    };
    fs::write(
        run_dir.join("run.toml"),
        format!(
            "instance = \"{}\"\nside = \"{}\"\nstatus = \"{}\"\ngame_dir = \"{}\"\njava = \"{}\"\nartifact = \"{}\"\nexit_code = {}\ntimed_out = {}\ntimeout = \"{}\"\nfailure_class = \"{}\"\nstdout = \"{}\"\nstderr = \"{}\"\nminecraft_log = \"{}\"\ncrash_report = \"{}\"\n",
            instance.name,
            side,
            if timed_out {
                "timed_out"
            } else if success {
                "passed"
            } else {
                "failed"
            },
            game_dir.display(),
            java.display(),
            artifact.display(),
            exit_code.unwrap_or(-1),
            timed_out,
            options.timeout.as_deref().unwrap_or(""),
            failure_class,
            run_dir.join("stdout.log").display(),
            run_dir.join("stderr.log").display(),
            artifacts
                .minecraft_log
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            artifacts
                .crash_report
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default()
        ),
    )
    .map_err(|error| format!("failed to write run report: {error}"))?;

    Ok(RunResult {
        success,
        exit_code,
        timed_out,
    })
}

pub(super) struct RunArtifacts {
    minecraft_log: Option<PathBuf>,
    crash_report: Option<PathBuf>,
}

pub(super) fn collect_run_artifacts(game_dir: &Path, run_dir: &Path) -> Result<RunArtifacts, String> {
    let minecraft_log = copy_if_exists(
        &game_dir.join("logs").join("latest.log"),
        &run_dir.join("minecraft-latest.log"),
    )?;
    let crash_report = newest_crash_report(&game_dir.join("crash-reports"))?
        .map(|path| copy_crash_report(&path, run_dir))
        .transpose()?;

    Ok(RunArtifacts {
        minecraft_log,
        crash_report,
    })
}

pub(super) fn copy_if_exists(source: &Path, destination: &Path) -> Result<Option<PathBuf>, String> {
    if !source.is_file() {
        return Ok(None);
    }

    fs::copy(source, destination)
        .map_err(|error| format!("failed to copy {}: {error}", source.display()))?;
    Ok(Some(destination.to_path_buf()))
}

pub(super) fn newest_crash_report(crash_dir: &Path) -> Result<Option<PathBuf>, String> {
    if !crash_dir.is_dir() {
        return Ok(None);
    }

    let mut newest = None;
    for entry in fs::read_dir(crash_dir)
        .map_err(|error| format!("failed to read {}: {error}", crash_dir.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read crash report entry: {error}"))?
            .path();
        if path.is_file() {
            newest = Some(path);
        }
    }

    Ok(newest)
}

pub(super) fn copy_crash_report(source: &Path, run_dir: &Path) -> Result<PathBuf, String> {
    let file_name = source
        .file_name()
        .ok_or_else(|| format!("crash report has no filename: {}", source.display()))?;
    let destination = run_dir.join(file_name);
    fs::copy(source, &destination)
        .map_err(|error| format!("failed to copy crash report {}: {error}", source.display()))?;
    Ok(destination)
}

pub(super) struct TimedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

pub(super) fn run_process_with_timeout(
    command: &mut Command,
    timeout: Option<Duration>,
) -> Result<TimedOutput, String> {
    let Some(timeout) = timeout else {
        let output = command
            .output()
            .map_err(|error| format!("process execution failed: {error}"))?;
        return Ok(TimedOutput {
            status: output.status,
            stdout: output.stdout,
            stderr: output.stderr,
            timed_out: false,
        });
    };

    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("process spawn failed: {error}"))?;
    let deadline = Instant::now() + timeout;

    loop {
        if child
            .try_wait()
            .map_err(|error| format!("process wait failed: {error}"))?
            .is_some()
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("process output collection failed: {error}"))?;
            return Ok(TimedOutput {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                timed_out: false,
            });
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|error| format!("process output collection failed: {error}"))?;
            return Ok(TimedOutput {
                status: output.status,
                stdout: output.stdout,
                stderr: output.stderr,
                timed_out: true,
            });
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(super) fn parse_duration(value: &str) -> Result<Duration, String> {
    if let Some(ms) = value.strip_suffix("ms") {
        let millis = ms
            .parse()
            .map_err(|error| format!("invalid timeout `{value}`: {error}"))?;
        return Ok(Duration::from_millis(millis));
    }

    if let Some(seconds) = value.strip_suffix('s') {
        let seconds = seconds
            .parse()
            .map_err(|error| format!("invalid timeout `{value}`: {error}"))?;
        return Ok(Duration::from_secs(seconds));
    }

    Err(format!("timeout `{value}` must use `ms` or `s`"))
}

pub(super) fn reconcile_mods(root: &Path, instance: &Instance, mods_dir: &Path) -> Result<(), String> {
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

pub(super) fn resolved_mod_path(root: &Path, source: &str) -> Result<Option<PathBuf>, String> {
    if let Some(path) = local_mod_path(root, source) {
        return path
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("failed to resolve local mod {}: {error}", path.display()));
    }

    if let Some(path) = locked_mod_path(root, source)? {
        return path
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("failed to resolve locked mod {}: {error}", path.display()));
    }

    if let Some(coordinates) = MavenCoordinates::parse(source) {
        return Ok(maven_artifact(&[], &coordinates)
            .map(|(_, path)| path)
            .and_then(|path| path.canonicalize().ok()));
    }

    Ok(None)
}

pub(super) fn locked_mod_path(root: &Path, source: &str) -> Result<Option<PathBuf>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    for block in lock.split("[[mod]]").skip(1) {
        if block_string_value(block, "source").as_deref() == Some(source)
            && let Some(path) = block_string_value(block, "path")
        {
            return Ok(Some(PathBuf::from(path)));
        }
    }

    Ok(None)
}

pub(super) fn locked_minecraft_url(root: &Path, key: &str) -> Result<Option<String>, String> {
    locked_value(root, key)
}

pub(super) fn locked_value(root: &Path, key: &str) -> Result<Option<String>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    Ok(block_string_value(&lock, key))
}

pub(super) fn locked_main_class(root: &Path, side: &str) -> Result<Option<String>, String> {
    let side_key = format!("{side}_main_class");
    if let Some(main_class) = locked_value(root, &side_key)? {
        return Ok(Some(main_class));
    }

    locked_value(root, "main_class")
}

pub(super) fn fetch_locked_libraries(root: &Path, cache_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(Vec::new());
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    let mut libraries = Vec::new();
    for block in lock.split("[[library]]").skip(1) {
        let file_name = block_string_value(block, "path")
            .and_then(|path| path.rsplit('/').next().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "library.jar".to_string());
        if let Some(url) = block_string_value(block, "url") {
            libraries.push(fetch_to_cache(&url, cache_dir, &file_name)?);
        } else if let Some(path) = block_string_value(block, "path") {
            libraries.push(PathBuf::from(path));
        }
    }

    Ok(libraries)
}

pub(super) fn join_classpath(paths: &[PathBuf]) -> String {
    let separator = if cfg!(windows) { ";" } else { ":" };
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

pub(super) fn block_string_value(block: &str, key: &str) -> Option<String> {
    block.lines()
        .map(str::trim)
        .find_map(|line| string_value(line, key))
}

pub(super) fn run_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    format!("{millis}")
}

pub(super) fn config_path(explicit_config: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit_config {
        Some(path) => Ok(path),
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string()),
    }
}
