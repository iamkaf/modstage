fn inspect_config(explicit_config: Option<PathBuf>) -> Result<(), String> {
    let config_path = match explicit_config {
        Some(path) => path,
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string())?,
    };

    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;

    println!("config: {}", config_path.display());
    println!("project: {project_name}");
    println!("state-id: {}", dirs.project_id);
    println!("data: {}", dirs.data.display());
    println!("cache: {}", dirs.cache.display());

    Ok(())
}

fn inspect_lock(explicit_config: Option<PathBuf>, instance: Option<&str>) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let lock_path = root.join("modstage.lock");
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

fn inspect_run(explicit_config: Option<PathBuf>, run_id: &str) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;
    let report_path = dirs
        .data
        .join("runs")
        .join(&dirs.project_id)
        .join(run_id)
        .join("run.toml");
    let report = fs::read_to_string(&report_path)
        .map_err(|error| format!("failed to read {}: {error}", report_path.display()))?;

    print!("{report}");

    Ok(())
}

fn clean_instance(
    explicit_config: Option<PathBuf>,
    instance_name: &str,
    args: &[String],
) -> Result<(), String> {
    let config_path = config_path(explicit_config)?;
    let contents = fs::read_to_string(&config_path)
        .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
    let project_name = project_name(&contents)
        .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
    let root = config_path.parent().unwrap_or_else(|| Path::new("."));
    let dirs = StateDirs::for_project(&project_name, root)?;
    let mut target = dirs
        .data
        .join("instances")
        .join(&dirs.project_id)
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

fn parse_side_arg(args: &[String]) -> Result<Option<&str>, String> {
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

fn init_project() -> Result<(), String> {
    let current_dir = env::current_dir().map_err(|error| error.to_string())?;
    let config_path = current_dir.join("modstage.toml");

    if config_path.exists() {
        return Err(format!("{} already exists", config_path.display()));
    }

    let project_name = current_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("modstage-project");
    let template = format!(
        r#"[project]
name = "{project_name}"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#
    );

    fs::write(&config_path, template)
        .map_err(|error| format!("failed to write {}: {error}", config_path.display()))?;
    println!("created {}", config_path.display());

    Ok(())
}

