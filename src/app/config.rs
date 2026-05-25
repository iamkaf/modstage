struct Config {
    project_name: String,
    repositories: Vec<(String, String)>,
    instances: Vec<Instance>,
}

struct Instance {
    name: String,
    minecraft: String,
    loader: String,
    loader_version: Option<String>,
    sides: Vec<String>,
    mods: Vec<String>,
}

impl Config {
    fn parse(contents: &str) -> Result<Self, String> {
        let project_name = project_name(contents).ok_or_else(|| {
            "modstage.toml must contain [project] with a name".to_string()
        })?;
        let mut section = "";
        let mut repositories = Vec::new();
        let mut instances = Vec::new();
        let mut current: Option<Instance> = None;
        let mut multiline_array: Option<(String, Vec<String>)> = None;

        for line in contents.lines() {
            let line = line.trim();

            if let Some((key, values)) = multiline_array.as_mut() {
                if line == "]" {
                    if key == "mods"
                        && let Some(instance) = current.as_mut()
                    {
                        instance.mods = values.clone();
                    }
                    multiline_array = None;
                    continue;
                }

                values.push(line.trim_end_matches(',').trim_matches('"').to_string());
                continue;
            }

            if line == "[repositories]" {
                section = "repositories";
                continue;
            }

            if line == "[[instance]]" {
                if let Some(instance) = current.take() {
                    instances.push(instance);
                }

                section = "instance";
                current = Some(Instance {
                    name: String::new(),
                    minecraft: String::new(),
                    loader: String::new(),
                    loader_version: None,
                    sides: Vec::new(),
                    mods: Vec::new(),
                });
                continue;
            }

            if line.starts_with('[') {
                section = "";
                continue;
            }

            if section == "repositories" {
                if let Some((name, url)) = key_value(line) {
                    repositories.push((name, url));
                }
                continue;
            }

            let Some(instance) = current.as_mut() else {
                continue;
            };

            if let Some(value) = string_value(line, "name") {
                instance.name = value;
            } else if let Some(value) = string_value(line, "minecraft") {
                instance.minecraft = value;
            } else if let Some(value) = string_value(line, "loader") {
                instance.loader = value;
            } else if let Some(value) = string_value(line, "loader_version") {
                instance.loader_version = Some(value);
            } else if let Some(value) = string_array_value(line, "sides") {
                instance.sides = value;
            } else if let Some(value) = string_array_value(line, "mods") {
                instance.mods = value;
            } else if line == "mods = [" {
                multiline_array = Some(("mods".to_string(), Vec::new()));
            }
        }

        if let Some(instance) = current.take() {
            instances.push(instance);
        }

        for instance in &instances {
            if instance.name.is_empty() {
                return Err("instance is missing name".to_string());
            }
            if instance.minecraft.is_empty() {
                return Err(format!("instance `{}` is missing minecraft", instance.name));
            }
            if instance.loader.is_empty() {
                return Err(format!("instance `{}` is missing loader", instance.name));
            }
            if instance.sides.is_empty() {
                return Err(format!("instance `{}` is missing sides", instance.name));
            }
        }

        Ok(Self {
            project_name,
            repositories,
            instances,
        })
    }
}

fn discover_config(start: &Path) -> Result<Option<PathBuf>, String> {
    let mut current = start
        .canonicalize()
        .map_err(|error| format!("failed to resolve {}: {error}", start.display()))?;

    loop {
        let candidate = current.join("modstage.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !current.pop() {
            return Ok(None);
        }
    }
}

fn project_name(contents: &str) -> Option<String> {
    let mut in_project = false;

    for line in contents.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            in_project = line == "[project]";
            continue;
        }

        if !in_project {
            continue;
        }

        if let Some(value) = line.strip_prefix("name") {
            let value = value.trim_start();
            let value = value.strip_prefix('=')?.trim();
            return Some(value.trim_matches('"').to_string());
        }
    }

    None
}

fn string_value(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
}

fn key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_string(), value.trim().trim_matches('"').to_string()))
}

fn string_array_value(line: &str, key: &str) -> Option<Vec<String>> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    let value = value.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        value
            .split(',')
            .map(|item| item.trim().trim_matches('"').to_string())
            .filter(|item| !item.is_empty())
            .collect(),
    )
}

fn local_mod_path(root: &Path, source: &str) -> Option<PathBuf> {
    if source.starts_with("maven:") || source.starts_with("modrinth:") {
        return None;
    }

    let path = PathBuf::from(source);
    Some(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

struct MavenCoordinates<'a> {
    group: &'a str,
    artifact: &'a str,
    version: &'a str,
}

impl<'a> MavenCoordinates<'a> {
    fn parse(source: &'a str) -> Option<Self> {
        let source = source.strip_prefix("maven:")?;
        let mut parts = source.split(':');
        let group = parts.next()?;
        let artifact = parts.next()?;
        let version = parts.next()?;

        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            group,
            artifact,
            version,
        })
    }
}

fn maven_artifact(
    repositories: &[(String, String)],
    coordinates: &MavenCoordinates<'_>,
) -> Option<(String, PathBuf)> {
    for (name, url) in repositories {
        if url == "mavenLocal" {
            if let Some(path) = maven_local_artifact(coordinates) {
                return Some((name.clone(), path));
            }
        } else if let Some(root) = url.strip_prefix("file://")
            && let Some(path) = maven_artifact_under(PathBuf::from(root), coordinates)
        {
            return Some((name.clone(), path));
        }
    }

    maven_local_artifact(coordinates).map(|path| ("mavenLocal".to_string(), path))
}

fn maven_local_artifact(coordinates: &MavenCoordinates<'_>) -> Option<PathBuf> {
    maven_artifact_under(maven_local_root()?, coordinates)
}

fn maven_artifact_under(
    mut path: PathBuf,
    coordinates: &MavenCoordinates<'_>,
) -> Option<PathBuf> {

    for segment in coordinates.group.split('.') {
        path.push(segment);
    }

    path.push(coordinates.artifact);
    path.push(coordinates.version);
    path.push(format!(
        "{}-{}.jar",
        coordinates.artifact, coordinates.version
    ));

    path.is_file().then_some(path)
}

fn maven_local_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MODSTAGE_MAVEN_LOCAL") {
        return Some(PathBuf::from(path));
    }

    Some(home_dir().ok()?.join(".m2").join("repository"))
}

