use super::*;

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
    verify_locked_artifact_hash(root, side, &artifact)?;
    let scenario = stage_scenario(root, run_dir, options.scenario.as_deref())?;
    let java = options
        .java
        .clone()
        .unwrap_or_else(|| PathBuf::from(java_bin()));
    let mut command = Command::new(&java);
    let mut launch_args = Vec::new();
    if let Some(main_class) = locked_main_class(root, side)? {
        let mut classpath = vec![artifact.clone()];
        classpath.extend(fetch_locked_libraries(root, &cache_dir.join("libraries"))?);
        let classpath = join_classpath(&classpath);
        command.arg("-cp").arg(&classpath).arg(&main_class);
        launch_args.push("-cp".to_string());
        launch_args.push(classpath);
        launch_args.push(main_class);
        if side == "client"
            && let Some(asset_index) = locked_value(root, "id")?
        {
            command.arg("--assetIndex").arg(&asset_index);
            launch_args.push("--assetIndex".to_string());
            launch_args.push(asset_index);
        }
        if side == "client" && locked_value(root, "index_url")?.is_some() {
            let assets_dir = cache_dir.join("assets").join("objects");
            command.arg("--assetsDir").arg(&assets_dir);
            launch_args.push("--assetsDir".to_string());
            launch_args.push(assets_dir.display().to_string());
        }
    } else if side == "server" {
        command.arg("-jar").arg(&artifact);
        command.arg("nogui");
        launch_args.push("-jar".to_string());
        launch_args.push(artifact.display().to_string());
        launch_args.push("nogui".to_string());
    } else {
        command.arg("-jar").arg(&artifact);
        launch_args.push("-jar".to_string());
        launch_args.push(artifact.display().to_string());
    }
    if let Some(scenario) = &scenario {
        command.arg("--modstageScenario").arg(scenario);
        command.env("MODSTAGE_RUN_DIR", run_dir);
        command.env("MODSTAGE_ARTIFACT_DIR", run_dir.join("artifacts"));
        launch_args.push("--modstageScenario".to_string());
        launch_args.push(scenario.display().to_string());
    }
    let launch_plan = write_launch_plan(
        instance,
        side,
        &java,
        &artifact,
        scenario.as_deref(),
        run_dir,
        &launch_args,
    )?;
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
    let failure_class = classify_failure(success, timed_out, &artifacts)?;
    fs::write(
        run_dir.join("run.toml"),
        format!(
            "instance = \"{}\"\nside = \"{}\"\nstatus = \"{}\"\ngame_dir = \"{}\"\njava = \"{}\"\nartifact = \"{}\"\nscenario = \"{}\"\nlaunch_plan = \"{}\"\nexit_code = {}\ntimed_out = {}\ntimeout = \"{}\"\nfailure_class = \"{}\"\nstdout = \"{}\"\nstderr = \"{}\"\nminecraft_log = \"{}\"\ncrash_report = \"{}\"\n",
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
            scenario
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            launch_plan.display(),
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
