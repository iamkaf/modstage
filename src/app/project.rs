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

    pub(super) fn instance_dir(&self, instance: &str) -> Result<PathBuf, String> {
        validate_instance_path_token(instance)?;
        Ok(self
            .dirs
            .data
            .join("instances")
            .join(&self.dirs.project_id)
            .join(instance))
    }

    pub(super) fn lock_path(&self, instance: &str) -> Result<PathBuf, String> {
        Ok(self.instance_dir(instance)?.join("modstage.lock"))
    }
}

fn validate_instance_path_token(instance: &str) -> Result<(), String> {
    if instance.is_empty()
        || instance.contains("..")
        || instance.contains('/')
        || instance.contains('\\')
        || Path::new(instance).is_absolute()
        || !instance
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(format!(
            "invalid instance name `{instance}`; use only letters, numbers, '-', '_', and '.'"
        ));
    }

    Ok(())
}

pub(super) fn project_config_path(explicit_config: Option<PathBuf>) -> Result<PathBuf, String> {
    match explicit_config {
        Some(path) => Ok(path),
        None => discover_config(&env::current_dir().map_err(|error| error.to_string())?)?
            .ok_or_else(|| "no modstage.toml found; run `modstage init`".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_context() -> ProjectContext {
        ProjectContext {
            config_path: PathBuf::from("modstage.toml"),
            root: PathBuf::from("/project"),
            config: Config {
                project_name: "test".to_string(),
                repositories: Vec::new(),
                instances: Vec::new(),
            },
            dirs: StateDirs {
                project_id: "test-00000000".to_string(),
                data: PathBuf::from("/state/modstage"),
                cache: PathBuf::from("/cache/modstage"),
            },
        }
    }

    #[test]
    fn lock_path_accepts_valid_instance_names() {
        let project = project_context();

        for instance in ["client", "server-1", "my_instance"] {
            let path = project
                .lock_path(instance)
                .expect("valid instance name should produce a lock path");
            assert!(
                path.ends_with(Path::new(instance).join("modstage.lock")),
                "lock path should end with the instance directory and lockfile: {}",
                path.display()
            );
        }
    }

    #[test]
    fn lock_path_rejects_traversal_instance_names() {
        let project = project_context();

        for instance in ["..", "../other", "safe/../other", "safe..other"] {
            assert!(
                project.lock_path(instance).is_err(),
                "lock_path should reject traversal input {instance:?}"
            );
        }
    }

    #[test]
    fn lock_path_rejects_absolute_and_separated_instance_names() {
        let project = project_context();

        for instance in ["/tmp/instance", "loader/server", r"loader\server"] {
            assert!(
                project.lock_path(instance).is_err(),
                "lock_path should reject unsafe input {instance:?}"
            );
        }
    }
}
