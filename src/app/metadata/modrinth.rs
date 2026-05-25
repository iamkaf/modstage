use super::*;

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
    let version_metadata = select_modrinth_version(&metadata, source).ok_or_else(|| {
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
    let file_metadata = primary_modrinth_file(&metadata)
        .filter(|_| source.version.is_none())
        .or_else(|| primary_modrinth_file(version_metadata))
        .ok_or_else(|| {
            format!(
                "Modrinth project `{}` did not include a primary file",
                source.project
            )
        })?;
    let filename = json_string(file_metadata, "filename").ok_or_else(|| {
        format!(
            "Modrinth project `{}` primary file had no filename",
            source.project
        )
    })?;
    let url = json_string(file_metadata, "url").ok_or_else(|| {
        format!(
            "Modrinth project `{}` primary file had no URL",
            source.project
        )
    })?;
    let path = fetch_to_cache(&url, &cache_dir, &filename)?;
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read Modrinth file {}: {error}", path.display()))?;

    Ok(ModrinthMod {
        project: source.project.to_string(),
        version_id: json_string(version_metadata, "id").ok_or_else(|| {
            format!(
                "Modrinth project `{}` metadata had no version id",
                source.project
            )
        })?,
        version_number: json_string(version_metadata, "version_number").ok_or_else(|| {
            format!(
                "Modrinth project `{}` metadata had no version number",
                source.project
            )
        })?,
        filename,
        url,
        path,
        sha1: json_object_string(file_metadata, "hashes", "sha1").unwrap_or_default(),
        sha512: json_object_string(file_metadata, "hashes", "sha512").unwrap_or_default(),
        sha256: sha256_hex(&bytes),
    })
}

pub(in crate::app) fn select_modrinth_version<'a>(
    metadata: &'a str,
    source: &ModrinthSource<'_>,
) -> Option<&'a str> {
    let Some(version) = source.version else {
        return Some(metadata);
    };

    for block in modrinth_version_blocks(metadata) {
        if json_string(block, "version_number").as_deref() == Some(version)
            || json_string(block, "id").as_deref() == Some(version)
        {
            return Some(block);
        }
    }

    None
}

pub(in crate::app) fn modrinth_version_blocks(metadata: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = metadata;

    while let Some(version_position) = rest.find("\"version_number\"") {
        let before_version = &rest[..version_position];
        let Some(block_start) = before_version.rfind('{') else {
            break;
        };
        let block = &rest[block_start..];
        blocks.push(block);
        rest = &rest[version_position + "\"version_number\"".len()..];
    }

    blocks
}

pub(in crate::app) fn primary_modrinth_file(metadata: &str) -> Option<&str> {
    let primary = metadata.find("\"primary\"")?;
    let file_start = metadata[..primary].rfind('{')?;
    Some(&metadata[file_start..])
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
