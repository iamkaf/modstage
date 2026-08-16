use super::*;

pub(super) struct RunArtifacts {
    pub(super) minecraft_log: Option<PathBuf>,
    pub(super) crash_report: Option<PathBuf>,
}

pub(super) struct RunReport<'a> {
    pub(super) instance: &'a Instance,
    pub(super) side: &'a str,
    pub(super) game_dir: &'a Path,
    pub(super) run_dir: &'a Path,
}

impl<'a> RunReport<'a> {
    pub(super) fn new(
        instance: &'a Instance,
        side: &'a str,
        game_dir: &'a Path,
        run_dir: &'a Path,
    ) -> Self {
        Self {
            instance,
            side,
            game_dir,
            run_dir,
        }
    }

    pub(super) fn path(&self) -> PathBuf {
        self.run_dir.join("run.toml")
    }

    pub(super) fn write_staged(&self) -> Result<(), String> {
        let mut report = TomlDocument::new();
        report.string("instance", &self.instance.name);
        report.string("side", self.side);
        report.string("status", "staged");
        report.string("game_dir", self.game_dir.display().to_string());

        fs::write(self.path(), report.finish())
            .map_err(|error| format!("failed to write run report: {error}"))
    }

    pub(super) fn finalize(&self, final_report: FinalRunReport<'_>) -> Result<PathBuf, String> {
        let report_path = self.path();
        let mut report = TomlDocument::new();
        report.string("instance", &self.instance.name);
        report.string("side", self.side);
        report.string("status", final_report.status);
        report.string("game_dir", self.game_dir.display().to_string());
        report.string("java", final_report.java.display().to_string());
        report.string("artifact", final_report.artifact.display().to_string());
        report.string(
            "launch_plan",
            final_report.launch_plan.display().to_string(),
        );
        report.integer("exit_code", final_report.exit_code.unwrap_or(-1));
        report.line(format!("timed_out = {}", final_report.timed_out));
        report.string("timeout", final_report.timeout.unwrap_or(""));
        report.string("failure_class", final_report.failure_class);
        report.string(
            "stdout",
            self.run_dir.join("stdout.log").display().to_string(),
        );
        report.string(
            "stderr",
            self.run_dir.join("stderr.log").display().to_string(),
        );
        report.string(
            "minecraft_log",
            final_report
                .artifacts
                .minecraft_log
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        );
        report.string(
            "crash_report",
            final_report
                .artifacts
                .crash_report
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
        );

        fs::write(&report_path, report.finish())
            .map_err(|error| format!("failed to write run report: {error}"))?;

        Ok(report_path)
    }
}

pub(super) struct FinalRunReport<'a> {
    pub(super) status: &'a str,
    pub(super) java: &'a Path,
    pub(super) artifact: &'a Path,
    pub(super) launch_plan: &'a Path,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
    pub(super) timeout: Option<&'a str>,
    pub(super) failure_class: &'a str,
    pub(super) artifacts: &'a RunArtifacts,
}

pub(super) fn classify_failure(
    success: bool,
    timed_out: bool,
    artifacts: &RunArtifacts,
) -> Result<&'static str, String> {
    if timed_out {
        return Ok("timeout");
    }
    if artifacts.crash_report.is_some() {
        return Ok("crash_report");
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
        if !success && looks_like_missing_dependency(&lower) {
            return Ok("missing_dependency");
        }
        if !success
            && lower
                .lines()
                .any(|line| line.contains("failed to load") && line.contains("resource"))
        {
            return Ok("resource_load");
        }
        if !success && (lower.contains("bootstraplauncher") || lower.contains("knot")) {
            return Ok("loader_bootstrap");
        }
    }

    if success {
        return Ok("none");
    }

    Ok("process_exit")
}

fn looks_like_missing_dependency(log: &str) -> bool {
    log.lines().any(|line| {
        line.contains("missing or unsupported mandatory depend")
            || line.contains("missing mods:")
            || line.contains("mod resolution failed")
            || line.contains("incompatible mods found")
    })
}

pub(super) fn write_launch_plan(
    instance: &Instance,
    side: &str,
    java: &Path,
    artifact: &Path,
    run_dir: &Path,
    args: &[String],
) -> Result<PathBuf, String> {
    let path = run_dir.join("launch-plan.toml");
    let mut plan = TomlDocument::new();
    plan.string("instance", &instance.name);
    plan.string("side", side);
    plan.string("java", java.display().to_string());
    plan.string("artifact", artifact.display().to_string());

    for arg in args {
        plan.blank();
        plan.array_table("argument");
        plan.string("arg", arg);
    }

    fs::write(&path, plan.finish())
        .map_err(|error| format!("failed to write launch plan {}: {error}", path.display()))?;

    Ok(path)
}

pub(super) fn collect_run_artifacts(
    game_dir: &Path,
    run_dir: &Path,
    run_started: SystemTime,
) -> Result<RunArtifacts, String> {
    let minecraft_log = copy_if_exists(
        &game_dir.join("logs").join("latest.log"),
        &run_dir.join("minecraft-latest.log"),
    )?;
    let crash_report = newest_crash_report(&game_dir.join("crash-reports"), run_started)?
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

pub(super) fn newest_crash_report(
    crash_dir: &Path,
    run_started: SystemTime,
) -> Result<Option<PathBuf>, String> {
    if !crash_dir.is_dir() {
        return Ok(None);
    }

    let cutoff = run_started
        .checked_sub(Duration::from_secs(2))
        .unwrap_or(UNIX_EPOCH);
    let mut newest = None;
    for entry in fs::read_dir(crash_dir)
        .map_err(|error| format!("failed to read {}: {error}", crash_dir.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read crash report entry: {error}"))?
            .path();
        let modified = path
            .metadata()
            .and_then(|metadata| metadata.modified())
            .map_err(|error| format!("failed to read crash report metadata: {error}"))?;
        if path.is_file() && modified >= cutoff {
            newest = match newest {
                Some((newest_modified, newest_path)) if newest_modified > modified => {
                    Some((newest_modified, newest_path))
                }
                _ => Some((modified, path)),
            };
        }
    }

    Ok(newest.map(|(_, path)| path))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_log(success: bool, contents: &str) -> &'static str {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before UNIX_EPOCH")
            .as_nanos();
        let log = env::temp_dir().join(format!(
            "modstage-log-classification-{}-{success}-{nanos}",
            std::process::id()
        ));
        fs::write(&log, contents).expect("failed to write classification fixture");
        let artifacts = RunArtifacts {
            minecraft_log: Some(log.clone()),
            crash_report: None,
        };
        let class = classify_failure(success, false, &artifacts).unwrap();
        fs::remove_file(log).expect("failed to remove classification fixture");
        class
    }

    #[test]
    fn successful_process_is_not_failed_by_incidental_log_language() {
        assert_eq!(
            classify_log(true, "Optional integration has a missing dependency\n"),
            "none"
        );
    }

    #[test]
    fn vanilla_asset_and_discovery_logs_are_not_missing_dependencies() {
        assert_eq!(
            classify_log(
                false,
                "Missing metadata in pack minecraft:icons\n[net.minecraftforge.fml.loading.moddiscovery.ModDiscoverer/SCAN] Found 5 dependencies\n"
            ),
            "process_exit"
        );
        assert_eq!(
            classify_log(false, "Optional integration has a missing dependency\n"),
            "process_exit"
        );
    }

    #[test]
    fn loader_missing_mod_phrases_are_classified() {
        assert_eq!(
            classify_log(false, "Missing or unsupported mandatory dependencies:\n"),
            "missing_dependency"
        );
        assert_eq!(
            classify_log(false, "Missing Mods:\n  fabric-api\n"),
            "missing_dependency"
        );
        assert_eq!(
            classify_log(false, "Mod resolution failed!\n"),
            "missing_dependency"
        );
    }
}
