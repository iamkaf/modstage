use super::*;
use toml_edit::{DocumentMut, Item};

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
        let document = contents
            .parse::<DocumentMut>()
            .map_err(|error| format!("failed to parse modstage.toml: {error}"))?;
        let project_name = project_name(contents)
            .ok_or_else(|| "modstage.toml must contain [project] with a name".to_string())?;
        let repositories = repositories_from_document(&document);
        let instances = instances_from_document(&document);

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
            if !is_supported_loader(&instance.loader) {
                return Err(format!(
                    "instance `{}` uses unsupported loader `{}`; supported loaders: vanilla, fabric, forge, neoforge",
                    instance.name, instance.loader
                ));
            }
            if instance.sides.is_empty() {
                return Err(format!("instance `{}` is missing sides", instance.name));
            }
            for side in &instance.sides {
                if !is_supported_side(side) {
                    return Err(format!(
                        "instance `{}` uses unsupported side `{side}`; supported sides: client, server",
                        instance.name
                    ));
                }
            }
            for fixture in &instance.fixtures {
                if fixture.from.is_empty() {
                    return Err(format!(
                        "instance `{}` has a fixture missing from",
                        instance.name
                    ));
                }
                if fixture.to.is_empty() {
                    return Err(format!(
                        "instance `{}` has a fixture missing to",
                        instance.name
                    ));
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

fn repositories_from_document(document: &DocumentMut) -> Vec<(String, String)> {
    document
        .get("repositories")
        .and_then(Item::as_table)
        .map(|table| {
            table
                .iter()
                .filter_map(|(name, item)| {
                    item.as_str().map(|url| (name.to_string(), url.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn instances_from_document(document: &DocumentMut) -> Vec<Instance> {
    document
        .get("instance")
        .and_then(Item::as_array_of_tables)
        .map(|instances| {
            instances
                .iter()
                .map(|table| Instance {
                    name: table_string(table.get("name")).unwrap_or_default(),
                    minecraft: table_string(table.get("minecraft")).unwrap_or_default(),
                    loader: table_string(table.get("loader")).unwrap_or_default(),
                    loader_version: table_string(table.get("loader_version")),
                    sides: table_string_array(table.get("sides")),
                    mods: table_string_array(table.get("mods")),
                    fixtures: table
                        .get("fixture")
                        .and_then(Item::as_array_of_tables)
                        .map(|fixtures| {
                            fixtures
                                .iter()
                                .map(|fixture| Fixture {
                                    from: table_string(fixture.get("from")).unwrap_or_default(),
                                    to: table_string(fixture.get("to"))
                                        .unwrap_or_else(|| ".".to_string()),
                                    side: table_string(fixture.get("side")),
                                    replace: fixture
                                        .get("replace")
                                        .and_then(Item::as_bool)
                                        .unwrap_or(false),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn table_string(item: Option<&Item>) -> Option<String> {
    item.and_then(Item::as_str).map(str::to_string)
}

fn table_string_array(item: Option<&Item>) -> Vec<String> {
    item.and_then(Item::as_array)
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn is_supported_loader(loader: &str) -> bool {
    matches!(loader, "vanilla" | "fabric" | "forge" | "neoforge")
}

pub(super) fn is_supported_side(side: &str) -> bool {
    matches!(side, "client" | "server")
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
    contents
        .parse::<DocumentMut>()
        .ok()?
        .get("project")?
        .get("name")?
        .as_str()
        .map(str::to_string)
}

pub(super) fn string_value(line: &str, key: &str) -> Option<String> {
    let value = line.strip_prefix(key)?.trim_start();
    let value = value.strip_prefix('=')?.trim();
    Some(value.trim_matches('"').to_string())
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
    pub(super) classifier: Option<&'a str>,
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
        let classifier = parts.next();

        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            group,
            artifact,
            version,
            classifier,
        })
    }

    pub(super) fn artifact_relative_path(&self) -> String {
        let classifier = self
            .classifier
            .map(|classifier| format!("-{classifier}"))
            .unwrap_or_default();
        format!(
            "{}/{}/{}/{}-{}{}.jar",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            classifier
        )
    }

    pub(super) fn file_name(&self) -> String {
        let classifier = self
            .classifier
            .map(|classifier| format!("-{classifier}"))
            .unwrap_or_default();
        format!("{}-{}{}.jar", self.artifact, self.version, classifier)
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
    for segment in coordinates.artifact_relative_path().split('/') {
        path.push(segment);
    }

    path.is_file().then_some(path)
}

pub(super) fn maven_local_root() -> Option<PathBuf> {
    if let Some(path) = env::var_os("MODSTAGE_MAVEN_LOCAL") {
        return Some(PathBuf::from(path));
    }

    Some(home_dir().ok()?.join(".m2").join("repository"))
}
