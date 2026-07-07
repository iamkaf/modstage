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

    let project = ProjectContext::load(explicit_config)?;
    let instance = project.instance(selected)?;

    if !instance.sides.iter().any(|configured| configured == side) {
        return Err(format!(
            "instance `{selected}` does not support side `{side}`"
        ));
    }

    let lock_path = project.lock_path(selected)?;
    if options.locked && !lock_path.is_file() {
        return Err("locked run requires modstage.lock; run `modstage resolve` first".to_string());
    }
    if options.locked && lock_is_stale_for_instance(&lock_path, selected)? {
        return Err(format!(
            "locked run requires modstage.lock for instance `{selected}`; run `modstage resolve {selected}` first"
        ));
    }
    if !options.locked && lock_is_stale_for_instance(&lock_path, selected)? {
        resolve_instance(Some(project.config_path.clone()), Some(selected))?;
    }
    let mod_cache = project.dirs.cache.join("downloads").join("mods");
    if options.locked {
        verify_locked_mod_hashes(&lock_path, instance, &mod_cache)?;
    }
    let game_dir = project
        .dirs
        .data
        .join("instances")
        .join(&project.dirs.project_id)
        .join(&instance.name)
        .join(side)
        .join("game");
    let mods_dir = game_dir.join("mods");

    fs::create_dir_all(&mods_dir)
        .map_err(|error| format!("failed to create {}: {error}", mods_dir.display()))?;
    reconcile_mods(&project.root, &lock_path, instance, &mods_dir, &mod_cache)?;
    apply_fixtures(&project.root, side, instance, &game_dir)?;

    if side == "server" {
        fs::write(game_dir.join("eula.txt"), "eula=true\n")
            .map_err(|error| format!("failed to write server eula.txt: {error}"))?;
    }
    write_side_launcher_metadata(instance, side, &game_dir)?;

    let run_dir = project
        .dirs
        .data
        .join("runs")
        .join(&project.dirs.project_id)
        .join(run_id());
    fs::create_dir_all(&run_dir)
        .map_err(|error| format!("failed to create {}: {error}", run_dir.display()))?;
    RunReport::new(instance, side, &game_dir, &run_dir).write_staged()?;

    let artifact_key = if side == "server" {
        "server_url"
    } else {
        "client_url"
    };
    if let Some(artifact_url) = locked_minecraft_url(&lock_path, &instance.name, artifact_key)? {
        let result = launch_minecraft_instance(LaunchRequest {
            config: &project.config,
            instance,
            side,
            root: &project.root,
            lock_path: &lock_path,
            game_dir: &game_dir,
            run_dir: &run_dir,
            artifact_url: &artifact_url,
            options: &options,
        })?;

        if result.success {
            return Ok(());
        }

        if result.timed_out {
            return Err(format!("{side} run timed out"));
        }

        return Err(format!(
            "{side} run failed with exit code {}",
            result
                .exit_code
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
    pub(super) locked: bool,
    pub(super) java: Option<PathBuf>,
    pub(super) timeout: Option<String>,
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

    pub(super) fn timeout_duration(&self) -> Result<Option<Duration>, String> {
        self.timeout.as_deref().map(parse_duration).transpose()
    }
}

pub(super) struct RunResult {
    pub(super) success: bool,
    pub(super) exit_code: Option<i32>,
    pub(super) timed_out: bool,
}

pub(super) fn run_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);

    format!("{millis}")
}
