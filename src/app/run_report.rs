use super::*;

pub(super) struct RunArtifacts {
    pub(super) minecraft_log: Option<PathBuf>,
    pub(super) crash_report: Option<PathBuf>,
}

pub(super) fn classify_failure(
    success: bool,
    timed_out: bool,
    artifacts: &RunArtifacts,
) -> Result<&'static str, String> {
    if artifacts.crash_report.is_some() {
        return Ok("crash_report");
    }

    if timed_out {
        return Ok("timeout");
    }

    if let Some(log_path) = &artifacts.minecraft_log {
        let log = fs::read_to_string(log_path)
            .map_err(|error| format!("failed to read {}: {error}", log_path.display()))?;
        let lower = log.to_ascii_lowercase();
        if lower.contains("failed to start the minecraft server") {
            return Ok("server_start");
        }
        if lower.contains("mixin apply failed")
            || lower.contains("mixintransformererror")
            || lower.contains("mixin transformation")
        {
            return Ok("mixin");
        }
        if lower.contains("missing") && lower.contains("depend") {
            return Ok("missing_dependency");
        }
        if lower.contains("failed to load") && lower.contains("resource") {
            return Ok("resource_load");
        }
        if lower.contains("bootstraplauncher") || lower.contains("knot") {
            return Ok("loader_bootstrap");
        }
    }

    if success {
        return Ok("none");
    }

    Ok("process_exit")
}

pub(super) fn write_launch_plan(
    instance: &Instance,
    side: &str,
    java: &Path,
    artifact: &Path,
    scenario: Option<&Path>,
    run_dir: &Path,
    args: &[String],
) -> Result<PathBuf, String> {
    let path = run_dir.join("launch-plan.toml");
    let mut plan = format!(
        "instance = \"{}\"\nside = \"{}\"\njava = \"{}\"\nartifact = \"{}\"\nscenario = \"{}\"\n",
        instance.name,
        side,
        java.display(),
        artifact.display(),
        scenario
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    );

    for arg in args {
        plan.push_str("\n[[argument]]\n");
        plan.push_str(&format!("arg = \"{}\"\n", toml_escape(arg)));
    }

    plan.push_str("\n[[environment]]\nname = \"MODSTAGE_RUN_DIR\"\nvalue = \"");
    plan.push_str(&toml_escape(&run_dir.display().to_string()));
    plan.push_str("\"\n");
    plan.push_str("\n[[environment]]\nname = \"MODSTAGE_ARTIFACT_DIR\"\nvalue = \"");
    plan.push_str(&toml_escape(
        &run_dir.join("artifacts").display().to_string(),
    ));
    plan.push_str("\"\n");

    fs::write(&path, plan)
        .map_err(|error| format!("failed to write launch plan {}: {error}", path.display()))?;

    Ok(path)
}

pub(super) fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(super) fn collect_run_artifacts(
    game_dir: &Path,
    run_dir: &Path,
) -> Result<RunArtifacts, String> {
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

pub(super) fn stage_scenario(
    root: &Path,
    run_dir: &Path,
    scenario: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    let Some(scenario) = scenario else {
        return Ok(None);
    };

    let source = if scenario.is_absolute() {
        scenario.to_path_buf()
    } else {
        root.join(scenario)
    };
    if !source.is_file() {
        return Err(format!("scenario {} does not exist", source.display()));
    }

    let destination = run_dir.join("scenario.toml");
    fs::copy(&source, &destination)
        .map_err(|error| format!("failed to stage scenario {}: {error}", source.display()))?;
    fs::create_dir_all(run_dir.join("artifacts"))
        .map_err(|error| format!("failed to create run artifact directory: {error}"))?;

    Ok(Some(destination))
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
