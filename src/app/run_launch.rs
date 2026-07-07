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
    let artifact_name = format!("{side}.jar");
    let artifact = fetch_to_cache(artifact_url, &cache_dir, &artifact_name)?;
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
    } else if let Some(main_class) = main_class {
        if side == "server" {
            launch_artifact = loader_server_artifact(&artifact, &cache_dir)?;
        }
        for arg in locked_arguments(lock_path, &instance.name, "jvm")? {
            let arg = expand_launch_argument(&arg, &cache_dir, game_dir);
            launch_plan.arg(&mut command, arg);
        }
        let mut classpath = vec![launch_artifact.clone()];
        if side == "client"
            && let Some(patched) =
                installer_runtime.prepare_client_artifact(game_dir, &java, &launch_artifact)?
        {
            classpath.push(patched);
        }
        if side == "client"
            && let Some(runtime) = installer_runtime.neoforge_client_runtime()?
        {
            classpath.push(runtime);
        }
        classpath.extend(fetch_locked_libraries(
            lock_path,
            &instance.name,
            &cache_dir.join("libraries"),
            side,
        )?);
        let classpath = join_classpath(&classpath);
        launch_plan.arg_pair(&mut command, "-cp", classpath);
        launch_plan.arg(&mut command, main_class);
        let game_args = launch_game_arguments(
            instance,
            side,
            locked_arguments(lock_path, &instance.name, "game")?,
        )
        .into_iter()
        .map(|arg| expand_launch_argument(&arg, &cache_dir, game_dir))
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
        run_server_process_with_timeout(&mut command, timeout)
    } else if side == "client" {
        run_client_process_with_timeout(&mut command, timeout)
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
    fs::write(&destination, bytes)
        .map_err(|error| format!("failed to write {}: {error}", destination.display()))?;

    Ok(destination)
}

fn expand_launch_argument(arg: &str, cache_dir: &Path, game_dir: &Path) -> String {
    let classpath_separator = if cfg!(windows) { ";" } else { ":" };
    arg.replace(
        "${library_directory}",
        &cache_dir.join("libraries").display().to_string(),
    )
    .replace("${game_directory}", &game_dir.display().to_string())
    .replace("${classpath_separator}", classpath_separator)
    .replace("${version_name}", "modstage")
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
        return Ok(java.clone());
    }

    if let Some(major) = locked_java_major(lock_path, &instance.name)?
        && let Some(java) = managed_java_for_major(major)?
    {
        return Ok(java);
    }

    Ok(PathBuf::from(java_bin()))
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
