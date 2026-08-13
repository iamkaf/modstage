use super::*;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Read;
use zip::ZipArchive;

const MODRINTH_METADATA_TTL: Duration = Duration::from_secs(300);

#[derive(Deserialize)]
struct ModrinthVersionMetadata {
    id: String,
    version_number: String,
    files: Vec<ModrinthFileMetadata>,
    #[serde(default)]
    dependencies: Vec<ModrinthDependencyMetadata>,
}

#[derive(Deserialize)]
struct ModrinthDependencyMetadata {
    project_id: Option<String>,
    dependency_type: String,
}

#[derive(Deserialize)]
struct ModrinthFileMetadata {
    filename: String,
    url: String,
    primary: Option<bool>,
    hashes: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModrinthPackIndex {
    format_version: u32,
    game: String,
    name: String,
    version_id: String,
    files: Vec<ModrinthPackIndexFile>,
    dependencies: HashMap<String, String>,
}

#[derive(Deserialize)]
struct ModrinthPackIndexFile {
    path: String,
    hashes: HashMap<String, String>,
    env: Option<HashMap<String, String>>,
    downloads: Vec<String>,
}

pub(in crate::app) struct ModrinthMod {
    pub(in crate::app) project: String,
    pub(in crate::app) version_id: String,
    pub(in crate::app) version_number: String,
    pub(in crate::app) filename: String,
    pub(in crate::app) url: String,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) sha1: String,
    pub(in crate::app) sha512: String,
    pub(in crate::app) sha256: String,
    pub(in crate::app) required_projects: Vec<String>,
}

pub(in crate::app) struct ModrinthSource<'a> {
    pub(in crate::app) project: &'a str,
    pub(in crate::app) version: Option<&'a str>,
}

pub(in crate::app) struct ModrinthPack {
    pub(in crate::app) source: String,
    pub(in crate::app) project: String,
    pub(in crate::app) version_id: String,
    pub(in crate::app) version_number: String,
    pub(in crate::app) name: String,
    pub(in crate::app) index_version: String,
    pub(in crate::app) archive_url: String,
    pub(in crate::app) archive_path: PathBuf,
    pub(in crate::app) archive_sha256: String,
    pub(in crate::app) files: Vec<ModrinthPackFile>,
}

pub(in crate::app) struct ModrinthPackFile {
    pub(in crate::app) destination: String,
    pub(in crate::app) sides: Vec<String>,
    pub(in crate::app) url: Option<String>,
    pub(in crate::app) path: PathBuf,
    pub(in crate::app) sha1: String,
    pub(in crate::app) sha512: String,
    pub(in crate::app) sha256: String,
    pub(in crate::app) archive_entry: Option<String>,
}

pub(in crate::app) fn resolve_modrinth_mod(
    config: &Config,
    instance: &Instance,
    root: &Path,
    source: &ModrinthSource<'_>,
) -> Result<ModrinthMod, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs
        .cache
        .join("downloads")
        .join("modrinth")
        .join(source.project);
    let metadata_url = modrinth_versions_url(source.project, instance);
    let metadata_path = fetch_to_cache_with_ttl(
        &metadata_url,
        &cache_dir,
        &format!(
            "{}-{}-{}.json",
            source.project, instance.minecraft, instance.loader
        ),
        MODRINTH_METADATA_TTL,
    )?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let versions: Vec<ModrinthVersionMetadata> = serde_json::from_str(&metadata)
        .map_err(|error| format!("failed to parse Modrinth metadata: {error}"))?;
    let version_metadata = select_modrinth_version(&versions, source).ok_or_else(|| {
        if let Some(version) = source.version {
            format!(
                "Modrinth project `{}` did not include requested version `{version}`",
                source.project
            )
        } else {
            format!(
                "Modrinth project `{}` did not include any versions",
                source.project
            )
        }
    })?;
    let file_metadata = primary_modrinth_file(version_metadata).ok_or_else(|| {
        format!(
            "Modrinth project `{}` did not include a primary file",
            source.project
        )
    })?;
    let filename = file_metadata.filename.clone();
    let url = file_metadata.url.clone();
    let path = fetch_to_cache(&url, &cache_dir, &filename)?;
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read Modrinth file {}: {error}", path.display()))?;
    let hashes = file_metadata.hashes.as_ref();

    Ok(ModrinthMod {
        project: source.project.to_string(),
        version_id: version_metadata.id.clone(),
        version_number: version_metadata.version_number.clone(),
        filename,
        url,
        path,
        sha1: hashes
            .and_then(|hashes| hashes.get("sha1"))
            .cloned()
            .unwrap_or_default(),
        sha512: hashes
            .and_then(|hashes| hashes.get("sha512"))
            .cloned()
            .unwrap_or_default(),
        sha256: sha256_hex(&bytes),
        required_projects: version_metadata
            .dependencies
            .iter()
            .filter(|dependency| dependency.dependency_type == "required")
            .filter_map(|dependency| dependency.project_id.clone())
            .collect(),
    })
}

pub(in crate::app) fn resolve_modrinth_mod_tree(
    config: &Config,
    instance: &Instance,
    root: &Path,
    source: &ModrinthSource<'_>,
    provided_projects: &std::collections::HashSet<String>,
) -> Result<Vec<(String, ModrinthMod)>, String> {
    let root_source = format!(
        "modrinth:{}{}",
        source.project,
        source
            .version
            .map(|version| format!(":{version}"))
            .unwrap_or_default()
    );
    let mut queue = vec![(
        root_source,
        source.project.to_string(),
        source.version.map(str::to_string),
    )];
    let mut resolved = Vec::new();
    let mut seen = std::collections::HashSet::new();
    while let Some((declared_source, project, version)) = queue.pop() {
        if !seen.insert(project.clone()) {
            continue;
        }
        let dependency_source = ModrinthSource {
            project: &project,
            version: version.as_deref(),
        };
        let artifact = resolve_modrinth_mod(config, instance, root, &dependency_source)?;
        for dependency in &artifact.required_projects {
            if !seen.contains(dependency) && !provided_projects.contains(dependency) {
                queue.push((format!("modrinth:{dependency}"), dependency.clone(), None));
            }
        }
        resolved.push((declared_source, artifact));
    }
    Ok(resolved)
}

pub(in crate::app) fn resolve_modrinth_pack(
    config: &Config,
    instance: &Instance,
    root: &Path,
    source: &ModrinthSource<'_>,
) -> Result<ModrinthPack, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs
        .cache
        .join("downloads")
        .join("modrinth-packs")
        .join(source.project);
    let metadata_url = modrinth_versions_url(source.project, instance);
    let metadata_path = fetch_to_cache_with_ttl(
        &metadata_url,
        &cache_dir,
        &format!(
            "{}-{}-{}.json",
            source.project, instance.minecraft, instance.loader
        ),
        MODRINTH_METADATA_TTL,
    )?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let versions: Vec<ModrinthVersionMetadata> = serde_json::from_str(&metadata)
        .map_err(|error| format!("failed to parse Modrinth metadata: {error}"))?;
    let version_metadata = select_modrinth_version(&versions, source).ok_or_else(|| {
        format!(
            "Modrinth pack `{}` did not include requested version `{}`",
            source.project,
            source.version.unwrap_or("latest")
        )
    })?;
    let file_metadata = primary_modrinth_file(version_metadata).ok_or_else(|| {
        format!(
            "Modrinth pack `{}` did not include a primary file",
            source.project
        )
    })?;
    if !file_metadata.filename.ends_with(".mrpack") {
        return Err(format!(
            "Modrinth project `{}` version `{}` does not provide an .mrpack primary file",
            source.project, version_metadata.version_number
        ));
    }

    let archive_url = file_metadata.url.clone();
    let archive_path = fetch_to_cache(&archive_url, &cache_dir, &file_metadata.filename)?;
    let archive_bytes = fs::read(&archive_path).map_err(|error| {
        format!(
            "failed to read Modrinth pack {}: {error}",
            archive_path.display()
        )
    })?;
    let archive_sha256 = sha256_hex(&archive_bytes);
    let mut archive = ZipArchive::new(std::io::Cursor::new(&archive_bytes))
        .map_err(|error| format!("failed to open Modrinth pack archive: {error}"))?;
    let mut index_contents = String::new();
    archive
        .by_name("modrinth.index.json")
        .map_err(|error| format!("Modrinth pack is missing modrinth.index.json: {error}"))?
        .read_to_string(&mut index_contents)
        .map_err(|error| format!("failed to read modrinth.index.json: {error}"))?;
    let index: ModrinthPackIndex = serde_json::from_str(&index_contents)
        .map_err(|error| format!("failed to parse modrinth.index.json: {error}"))?;
    validate_pack_index(instance, &index)?;

    let mut files = Vec::new();
    for file in &index.files {
        let destination = safe_pack_destination(&file.path)?;
        let sides = pack_file_sides(file.env.as_ref(), &instance.sides);
        if sides.is_empty() {
            continue;
        }
        let url = file
            .downloads
            .first()
            .ok_or_else(|| format!("Modrinth pack file `{}` has no download URL", file.path))?;
        let file_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("Modrinth pack file `{}` has no filename", file.path))?;
        let cache_key = file
            .hashes
            .get("sha512")
            .or_else(|| file.hashes.get("sha1"))
            .cloned()
            .unwrap_or_else(|| stable_cache_key(&file.path));
        let file_cache = cache_dir.join("files").join(cache_key);
        let cached = file_cache.join(file_name);
        let path = if cached.is_file() {
            cached
        } else {
            fetch_to_cache(url, &file_cache, file_name)?
        };
        let bytes = fs::read(&path).map_err(|error| {
            format!(
                "failed to read Modrinth pack file {}: {error}",
                path.display()
            )
        })?;
        files.push(ModrinthPackFile {
            destination: normalized_pack_path(&destination),
            sides,
            url: Some(url.clone()),
            path,
            sha1: file.hashes.get("sha1").cloned().unwrap_or_default(),
            sha512: file.hashes.get("sha512").cloned().unwrap_or_default(),
            sha256: sha256_hex(&bytes),
            archive_entry: None,
        });
    }

    let override_root = cache_dir.join("overrides").join(&version_metadata.id);
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("failed to read Modrinth pack entry {index}: {error}"))?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            return Err(format!(
                "Modrinth pack contains unsafe entry `{}`",
                entry.name()
            ));
        };
        let Some((destination, sides)) = override_destination(&enclosed, &instance.sides) else {
            continue;
        };
        let destination = safe_pack_destination(&normalized_pack_path(&destination))?;
        let output = override_root.join(&destination);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|error| {
            format!(
                "failed to extract Modrinth pack entry `{}`: {error}",
                entry.name()
            )
        })?;
        fs::write(&output, &bytes)
            .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
        files.push(ModrinthPackFile {
            destination: normalized_pack_path(&destination),
            sides,
            url: None,
            path: output,
            sha1: String::new(),
            sha512: String::new(),
            sha256: sha256_hex(&bytes),
            archive_entry: Some(normalized_pack_path(&enclosed)),
        });
    }

    Ok(ModrinthPack {
        source: format!(
            "modrinth:{}:{}",
            source.project,
            source.version.unwrap_or("latest")
        ),
        project: source.project.to_string(),
        version_id: version_metadata.id.clone(),
        version_number: version_metadata.version_number.clone(),
        name: index.name,
        index_version: index.version_id,
        archive_url,
        archive_path,
        archive_sha256,
        files,
    })
}

fn validate_pack_index(instance: &Instance, index: &ModrinthPackIndex) -> Result<(), String> {
    if index.format_version != 1 || index.game != "minecraft" {
        return Err(format!(
            "unsupported Modrinth pack format: game `{}`, formatVersion {}",
            index.game, index.format_version
        ));
    }
    let minecraft = index
        .dependencies
        .get("minecraft")
        .ok_or_else(|| "Modrinth pack does not declare a Minecraft version".to_string())?;
    if minecraft != &instance.minecraft {
        return Err(format!(
            "Modrinth pack requires Minecraft {minecraft}, but instance `{}` requests {}",
            instance.name, instance.minecraft
        ));
    }
    let loader_key = match instance.loader.as_str() {
        "fabric" => Some("fabric-loader"),
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        "vanilla" => None,
        _ => None,
    };
    if let Some(loader_key) = loader_key {
        let pack_loader = index.dependencies.get(loader_key).ok_or_else(|| {
            format!(
                "Modrinth pack does not declare `{loader_key}` for {}",
                instance.loader
            )
        })?;
        if let Some(configured) = instance
            .loader_version
            .as_deref()
            .filter(|version| *version != "latest")
            && configured != pack_loader
        {
            return Err(format!(
                "Modrinth pack requires {} {pack_loader}, but instance `{}` requests {configured}",
                instance.loader, instance.name
            ));
        }
    }
    Ok(())
}

fn pack_file_sides(
    environment: Option<&HashMap<String, String>>,
    configured_sides: &[String],
) -> Vec<String> {
    configured_sides
        .iter()
        .filter(|side| {
            environment
                .and_then(|values| values.get(side.as_str()))
                .is_none_or(|value| value != "unsupported")
        })
        .cloned()
        .collect()
}

fn override_destination(
    entry: &Path,
    configured_sides: &[String],
) -> Option<(PathBuf, Vec<String>)> {
    for (prefix, sides) in [
        ("overrides", configured_sides.to_vec()),
        ("client-overrides", vec!["client".to_string()]),
        ("server-overrides", vec!["server".to_string()]),
    ] {
        let Ok(destination) = entry.strip_prefix(prefix) else {
            continue;
        };
        if destination.as_os_str().is_empty() {
            return None;
        }
        let sides = sides
            .into_iter()
            .filter(|side| configured_sides.contains(side))
            .collect::<Vec<_>>();
        if !sides.is_empty() {
            return Some((destination.to_path_buf(), sides));
        }
    }
    None
}

pub(in crate::app) fn safe_pack_destination(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.contains('\\') {
        return Err(format!("unsafe Modrinth pack path `{value}`"));
    }
    let path = PathBuf::from(value);
    if path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("unsafe Modrinth pack path `{value}`"));
    }
    Ok(path)
}

fn normalized_pack_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn stable_cache_key(value: &str) -> String {
    format!(
        "{:08x}",
        value.bytes().fold(0x811c9dc5_u32, |hash, byte| {
            (hash ^ u32::from(byte)).wrapping_mul(0x01000193)
        })
    )
}

fn select_modrinth_version<'a>(
    versions: &'a [ModrinthVersionMetadata],
    source: &ModrinthSource<'_>,
) -> Option<&'a ModrinthVersionMetadata> {
    let Some(version) = source.version else {
        return versions.first();
    };
    if version == "latest" {
        return versions.first();
    }

    versions
        .iter()
        .find(|candidate| candidate.version_number == version || candidate.id == version)
}

fn primary_modrinth_file(version: &ModrinthVersionMetadata) -> Option<&ModrinthFileMetadata> {
    version
        .files
        .iter()
        .find(|file| file.primary.unwrap_or(false))
        .or_else(|| version.files.first())
}

pub(in crate::app) fn modrinth_source(source: &str) -> Option<ModrinthSource<'_>> {
    let source = source.strip_prefix("modrinth:")?;
    let mut parts = source.split(':');
    let project = parts.next().filter(|project| !project.is_empty())?;
    let version = parts.next().filter(|version| !version.is_empty());
    if parts.next().is_some() {
        return None;
    }

    Some(ModrinthSource { project, version })
}

pub(in crate::app) fn modrinth_versions_url(project: &str, instance: &Instance) -> String {
    if let Ok(url) = env::var("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL") {
        return url.replace("{project}", project);
    }

    format!(
        "https://api.modrinth.com/v2/project/{project}/version?loaders=%5B%22{}%22%5D&game_versions=%5B%22{}%22%5D",
        instance.loader, instance.minecraft
    )
}
