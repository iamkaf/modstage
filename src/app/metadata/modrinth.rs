use super::*;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
struct ModrinthVersionMetadata {
    id: String,
    version_number: String,
    files: Vec<ModrinthFileMetadata>,
}

#[derive(Deserialize)]
struct ModrinthFileMetadata {
    filename: String,
    url: String,
    primary: Option<bool>,
    hashes: Option<HashMap<String, String>>,
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
}

pub(in crate::app) struct ModrinthSource<'a> {
    pub(in crate::app) project: &'a str,
    pub(in crate::app) version: Option<&'a str>,
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
    let metadata_path = fetch_to_cache(
        &metadata_url,
        &cache_dir,
        &format!(
            "{}-{}-{}.json",
            source.project, instance.minecraft, instance.loader
        ),
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
    })
}

fn select_modrinth_version<'a>(
    versions: &'a [ModrinthVersionMetadata],
    source: &ModrinthSource<'_>,
) -> Option<&'a ModrinthVersionMetadata> {
    let Some(version) = source.version else {
        return versions.first();
    };

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
        return url;
    }

    format!(
        "https://api.modrinth.com/v2/project/{project}/version?loaders=%5B%22{}%22%5D&game_versions=%5B%22{}%22%5D",
        instance.loader, instance.minecraft
    )
}
