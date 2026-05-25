use super::*;

pub(super) struct ProjectContext {
    pub(super) config_path: PathBuf,
    pub(super) root: PathBuf,
    pub(super) config: Config,
    pub(super) dirs: StateDirs,
}

impl ProjectContext {
    pub(super) fn load(explicit_config: Option<PathBuf>) -> Result<Self, String> {
        let config_path = project_config_path(explicit_config)?;
        let root = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let contents = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
        let config = Config::parse(&contents)?;
        let dirs = StateDirs::for_project(&config.project_name, &root)?;

        Ok(Self {
            config_path,
            root,
            config,
            dirs,
        })
    }

    pub(super) fn load_summary(explicit_config: Option<PathBuf>) -> Result<Self, String> {
        let config_path = project_config_path(explicit_config)?;
        let root = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        let contents = fs::read_to_string(&config_path)
            .map_err(|error| format!("failed to read {}: {error}", config_path.display()))?;
        let project_name = project_name(&contents)
            .ok_or_else(|| format!("missing [project] name in {}", config_path.display()))?;
        let dirs = StateDirs::for_project(&project_name, &root)?;

        Ok(Self {
            config_path,
            root,
            config: Config {
                project_name,
                repositories: Vec::new(),
                instances: Vec::new(),
            },
            dirs,
        })
    }

    pub(super) fn instance(&self, selected: &str) -> Result<&Instance, String> {
        self.config
            .instances
            .iter()
            .find(|instance| instance.name == selected)
            .ok_or_else(|| format!("unknown instance `{selected}`"))
    }

    pub(super) fn lock_path(&self) -> PathBuf {
        self.root.join("modstage.lock")
    }
}

pub(super) fn project_config_path(explicit_config: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit_config {
        Some(path) => Ok(path),
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string()),
    }
}
