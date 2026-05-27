use super::*;

pub(super) fn inspect_config(explicit_config: Option<PathBuf>) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;

    println!("config: {}", project.config_path.display());
    println!("project: {}", project.config.project_name);
    println!("state-id: {}", project.dirs.project_id);
    println!("data: {}", project.dirs.data.display());
    println!("cache: {}", project.dirs.cache.display());

    Ok(())
}

pub(super) fn inspect_lock(
    explicit_config: Option<PathBuf>,
    instance: Option<&str>,
) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;
    let lock_path = project.lock_path();
    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;

    if let Some(instance) = instance
        && !lock.contains(&format!("instance = \"{instance}\""))
    {
        return Err(format!("lockfile does not contain instance `{instance}`"));
    }

    print!("{lock}");
    Ok(())
}

pub(super) fn inspect_run(explicit_config: Option<PathBuf>, run_id: &str) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;
    let report_path = project
        .dirs
        .data
        .join("runs")
        .join(&project.dirs.project_id)
        .join(run_id)
        .join("run.toml");
    let report = fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read {}: {error}", report_path.display()))?;

    print!("{report}");

    Ok(())
}

pub(super) fn inspect_instance(
    explicit_config: Option<PathBuf>,
    instance_name: &str,
    args: &[String],
) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;
    let instance_dir = project
        .dirs
        .data
        .join("instances")
        .join(&project.dirs.project_id)
        .join(instance_name);

    if let Some(side) = parse_side_arg(args)? {
        validate_side(side)?;
        print_instance_side(instance_name, side, &instance_dir.join(side))
    } else {
        print_instance_summary(instance_name, &instance_dir)
    }
}

pub(super) fn print_instance_summary(
    instance_name: &str,
    instance_dir: &Path,
) -> Result<(), String> {
    if !instance_dir.exists() {
        return Err(format!(
            "instance `{instance_name}` has no staged state at {}",
            instance_dir.display()
        ));
    }

    let mut sides = Vec::new();
    for side in ["client", "server"] {
        if instance_dir.join(side).join("game").is_dir() {
            sides.push(side);
        }
    }

    println!(r#"instance = "{instance_name}""#);
    println!(r#"instance_dir = "{}""#, instance_dir.display());
    println!(
        "sides = [{}]",
        sides
            .iter()
            .map(|side| format!(r#""{side}""#))
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(())
}

pub(super) fn print_instance_side(
    instance_name: &str,
    side: &str,
    side_dir: &Path,
) -> Result<(), String> {
    let game_dir = side_dir.join("game");
    if !game_dir.is_dir() {
        return Err(format!(
            "instance `{instance_name}` has no staged {side} game directory at {}",
            game_dir.display()
        ));
    }

    let mods_dir = game_dir.join("mods");
    let mut mods = Vec::new();
    if mods_dir.is_dir() {
        for entry in fs::read_dir(&mods_dir)
            .map_err(|error| format!("failed to read {}: {error}", mods_dir.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
                && let Some(name) = entry.file_name().to_str()
            {
                mods.push(name.to_string());
            }
        }
    }
    mods.sort();

    println!(r#"instance = "{instance_name}""#);
    println!(r#"side = "{side}""#);
    println!(r#"game_dir = "{}""#, game_dir.display());
    println!(r#"mods_dir = "{}""#, mods_dir.display());
    println!("eula = {}", game_dir.join("eula.txt").is_file());
    println!(
        "mods = [{}]",
        mods.iter()
            .map(|name| format!(r#""{name}""#))
            .collect::<Vec<_>>()
            .join(", ")
    );

    Ok(())
}

pub(super) fn clean_instance(
    explicit_config: Option<PathBuf>,
    instance_name: &str,
    args: &[String],
) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;
    let mut target = project
        .dirs
        .data
        .join("instances")
        .join(&project.dirs.project_id)
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

pub(super) fn clean_cache(explicit_config: Option<PathBuf>) -> Result<(), String> {
    let project = ProjectContext::load_summary(explicit_config)?;

    if project.dirs.cache.exists() {
        fs::remove_dir_all(&project.dirs.cache).map_err(|error| {
            format!("failed to remove {}: {error}", project.dirs.cache.display())
        })?;
    }

    println!("removed {}", project.dirs.cache.display());

    Ok(())
}

pub(super) fn parse_side_arg(args: &[String]) -> Result<Option<&str>, String> {
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

pub(super) fn validate_side(side: &str) -> Result<(), String> {
    if side == "client" || side == "server" {
        Ok(())
    } else {
        Err(format!("side must be client or server, got `{side}`"))
    }
}

pub(super) fn init_project() -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|error| error.to_string())?;
    let config_path = current_dir.join("modstage.toml");

    if config_path.exists() {
        return Err(format!("{} already exists", config_path.display()));
    }

    let detected = DetectedProject::detect(&current_dir)?;
    let template = detected.to_config();

    fs::write(&config_path, template)
        .map_err(|error| format!("failed to write {}: {error}", config_path.display()))?;
    println!("created {}", config_path.display());

    Ok(())
}
