use super::*;

pub(super) struct Config {
    pub(super) project_name: String,
    pub(super) repositories: Vec<(String, String)>,
    pub(super) instances: Vec<Instance>,
}

pub(super) struct Instance {
    pub(super) name: String,
    pub(super) minecraft: String,
    pub(super) loader: String,
    pub(super) loader_version: Option<String>,
    pub(super) sides: Vec<String>,
    pub(super) mods: Vec<String>,
    pub(super) fixtures: Vec<Fixture>,
}

pub(super) struct Fixture {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) side: Option<String>,
    pub(super) replace: bool,
}

impl Config {
    pub(super) fn parse(contents: &str) -> Result<Self, String> {
        let project_name = project_name(contents).ok_or_else(|| {
            "modstage.toml must contain [project] with a name".to_string()
        })?;
        let mut section = "";
        let mut repositories = Vec::new();
        let mut instances = Vec::new();
        let mut current: Option<Instance> = None;
        let mut current_fixture: Option<Fixture> = None;
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
                if let Some(fixture) = current_fixture.take()
                    && let Some(instance) = current.as_mut()
                {
                    instance.fixtures.push(fixture);
                }
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
                    fixtures: Vec::new(),
                });
                continue;
            }

            if line == "[[instance.fixture]]" {
                if let Some(fixture) = current_fixture.take()
                    && let Some(instance) = current.as_mut()
                {
                    instance.fixtures.push(fixture);
                }

                section = "fixture";
                current_fixture = Some(Fixture {
                    from: String::new(),
                    to: ".".to_string(),
                    side: None,
                    replace: false,
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

            if section == "fixture" {
                let Some(fixture) = current_fixture.as_mut() else {
                    continue;
                };

                if let Some(value) = string_value(line, "from") {
                    fixture.from = value;
                } else if let Some(value) = string_value(line, "to") {
                    fixture.to = value;
                } else if let Some(value) = string_value(line, "side") {
                    fixture.side = Some(value);
                } else if let Some(value) = bool_value(line, "replace") {
                    fixture.replace = value;
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

        if let Some(fixture) = current_fixture.take()
            && let Some(instance) = current.as_mut()
        {
            instance.fixtures.push(fixture);
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
            for fixture in &instance.fixtures {
                if fixture.from.is_empty() {
                    return Err(format!("instance `{}` has a fixture missing from", instance.name));
                }
                if fixture.to.is_empty() {
                    return Err(format!("instance `{}` has a fixture missing to", instance.name));
                }
            }
        }

        Ok(Self {
            project_name,
            repositories,
            instances,
        })
    }
}

pub(super) fn discover_config(start: &Path) -> Result<Option<PathBuf>, String> {
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

pub(super) fn project_name(contents: &str) -> Option<String> {
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

pub(super) fn string_value(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
}

pub(super) fn key_value(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_string(), value.trim().trim_matches('"').to_string()))
}

pub(super) fn bool_value(line: &str, key: &str) -> Option<bool> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

pub(super) fn string_array_value(line: &str, key: &str) -> Option<Vec<String>> {
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

pub(super) fn local_mod_path(root: &Path, source: &str) -> Option<PathBuf> {
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

pub(super) struct MavenCoordinates<'a> {
    pub(super) group: &'a str,
    pub(super) artifact: &'a str,
    pub(super) version: &'a str,
}

impl<'a> MavenCoordinates<'a> {
    pub(super) fn parse(source: &'a str) -> Option<Self> {
        let source = source.strip_prefix("maven:")?;
        Self::parse_coordinate(source)
    }

    pub(super) fn parse_coordinate(source: &'a str) -> Option<Self> {
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

pub(super) fn maven_artifact(
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

pub(super) fn maven_local_artifact(coordinates: &MavenCoordinates<'_>) -> Option<PathBuf> {
    maven_artifact_under(maven_local_root()?, coordinates)
}

pub(super) fn maven_artifact_under(
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

pub(super) fn maven_local_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MODSTAGE_MAVEN_LOCAL") {
        return Some(PathBuf::from(path));
    }

    Some(home_dir().ok()?.join(".m2").join("repository"))
}
