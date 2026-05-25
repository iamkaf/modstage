use super::*;

pub(super) struct MinecraftMetadata {
    pub(super) manifest_url: String,
    pub(super) manifest_sha256: String,
    pub(super) version_url: String,
    pub(super) version_sha256: String,
    pub(super) java_major: u32,
    pub(super) main_class: Option<String>,
    pub(super) client_url: String,
    pub(super) client_sha256: String,
    pub(super) server_url: String,
    pub(super) server_sha256: String,
    pub(super) libraries: Vec<MinecraftLibrary>,
    pub(super) assets: Option<MinecraftAssets>,
}

pub(super) struct MinecraftLibrary {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) url: String,
    pub(super) sha256: String,
}

pub(super) struct MinecraftAssets {
    pub(super) id: String,
    pub(super) index_url: String,
    pub(super) index_sha256: String,
    pub(super) objects: Vec<MinecraftAsset>,
}

pub(super) struct MinecraftAsset {
    pub(super) name: String,
    pub(super) hash: String,
    pub(super) size: u32,
    pub(super) url: String,
    pub(super) sha256: String,
}

pub(super) struct LoaderMetadata {
    pub(super) kind: String,
    pub(super) version: String,
    pub(super) loader_maven: Option<String>,
    pub(super) intermediary_maven: Option<String>,
    pub(super) installer_maven: Option<String>,
    pub(super) client_main_class: String,
    pub(super) server_main_class: String,
}

pub(super) struct ModrinthMod {
    pub(super) project: String,
    pub(super) version_id: String,
    pub(super) version_number: String,
    pub(super) filename: String,
    pub(super) url: String,
    pub(super) path: PathBuf,
    pub(super) sha1: String,
    pub(super) sha512: String,
    pub(super) sha256: String,
}

pub(super) struct ModrinthSource<'a> {
    pub(super) project: &'a str,
    pub(super) version: Option<&'a str>,
}

pub(super) fn resolve_modrinth_mod(
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
        &format!("{}-{}-{}.json", source.project, instance.minecraft, instance.loader),
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
            format!("Modrinth project `{}` did not include any versions", source.project)
        }
    })?;
    let file_metadata = primary_modrinth_file(&metadata)
        .filter(|_| source.version.is_none())
        .or_else(|| primary_modrinth_file(version_metadata))
        .ok_or_else(|| format!("Modrinth project `{}` did not include a primary file", source.project))?;
    let filename = json_string(file_metadata, "filename")
        .ok_or_else(|| format!("Modrinth project `{}` primary file had no filename", source.project))?;
    let url = json_string(file_metadata, "url")
        .ok_or_else(|| format!("Modrinth project `{}` primary file had no URL", source.project))?;
    let path = fetch_to_cache(&url, &cache_dir, &filename)?;
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read Modrinth file {}: {error}", path.display()))?;

    Ok(ModrinthMod {
        project: source.project.to_string(),
        version_id: json_string(version_metadata, "id")
            .ok_or_else(|| format!("Modrinth project `{}` metadata had no version id", source.project))?,
        version_number: json_string(version_metadata, "version_number")
            .ok_or_else(|| format!("Modrinth project `{}` metadata had no version number", source.project))?,
        filename,
        url,
        path,
        sha1: json_object_string(file_metadata, "hashes", "sha1").unwrap_or_default(),
        sha512: json_object_string(file_metadata, "hashes", "sha512").unwrap_or_default(),
        sha256: sha256_hex(&bytes),
    })
}

pub(super) fn select_modrinth_version<'a>(
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

pub(super) fn modrinth_version_blocks(metadata: &str) -> Vec<&str> {
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

pub(super) fn primary_modrinth_file(metadata: &str) -> Option<&str> {
    let primary = metadata.find("\"primary\"")?;
    let file_start = metadata[..primary].rfind('{')?;
    Some(&metadata[file_start..])
}

pub(super) fn modrinth_source(source: &str) -> Option<ModrinthSource<'_>> {
    let source = source.strip_prefix("modrinth:")?;
    let mut parts = source.split(':');
    let project = parts.next().filter(|project| !project.is_empty())?;
    let version = parts.next().filter(|version| !version.is_empty());
    if parts.next().is_some() {
        return None;
    }

    Some(ModrinthSource { project, version })
}

pub(super) fn modrinth_versions_url(project: &str, instance: &Instance) -> String {
    if let Ok(url) = env::var("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL") {
        return url;
    }

    format!(
        "https://api.modrinth.com/v2/project/{project}/version?loaders=%5B%22{}%22%5D&game_versions=%5B%22{}%22%5D",
        instance.loader, instance.minecraft
    )
}

pub(super) fn resolve_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    match instance.loader.as_str() {
        "fabric" => resolve_fabric_loader_metadata(config, instance, root),
        "forge" => resolve_installer_loader_metadata(config, instance, root, "forge"),
        "neoforge" => resolve_neoforge_loader_metadata(config, instance, root),
        _ => Ok(None),
    }
}

pub(super) fn resolve_fabric_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    let Some(url) = fabric_meta_url() else {
        return Ok(None);
    };
    let metadata = loader_metadata_text(config, root, "fabric", &url, instance)?;

    Ok(Some(LoaderMetadata {
        kind: "fabric".to_string(),
        version: json_string(&metadata, "version")
            .unwrap_or_else(|| instance.loader_version.clone().unwrap_or_else(|| "latest".to_string())),
        loader_maven: Some(
            json_object_string(&metadata, "loader", "maven")
                .ok_or_else(|| "Fabric metadata did not include loader maven coordinate".to_string())?,
        ),
        intermediary_maven: Some(
            json_object_string(&metadata, "intermediary", "maven")
                .ok_or_else(|| "Fabric metadata did not include intermediary maven coordinate".to_string())?,
        ),
        installer_maven: None,
        client_main_class: json_object_string(&metadata, "mainClass", "client")
            .ok_or_else(|| "Fabric metadata did not include client main class".to_string())?,
        server_main_class: json_object_string(&metadata, "mainClass", "server")
            .ok_or_else(|| "Fabric metadata did not include server main class".to_string())?,
    }))
}

pub(super) fn resolve_neoforge_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    resolve_installer_loader_metadata(config, instance, root, "neoforge")
}

pub(super) fn resolve_installer_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
    loader: &str,
) -> Result<Option<LoaderMetadata>, String> {
    let Some(url) = installer_loader_meta_url(loader) else {
        return Ok(None);
    };
    let metadata = loader_metadata_text(config, root, loader, &url, instance)?;

    Ok(Some(LoaderMetadata {
        kind: loader.to_string(),
        version: json_string(&metadata, "version")
            .unwrap_or_else(|| instance.loader_version.clone().unwrap_or_else(|| "latest".to_string())),
        loader_maven: None,
        intermediary_maven: None,
        installer_maven: Some(
            json_string(&metadata, "installer_maven")
                .ok_or_else(|| format!("{loader} metadata did not include installer maven coordinate"))?,
        ),
        client_main_class: json_string(&metadata, "client_main_class")
            .ok_or_else(|| format!("{loader} metadata did not include client main class"))?,
        server_main_class: json_string(&metadata, "server_main_class")
            .ok_or_else(|| format!("{loader} metadata did not include server main class"))?,
    }))
}

pub(super) fn loader_metadata_text(
    config: &Config,
    root: &Path,
    loader: &str,
    url: &str,
    instance: &Instance,
) -> Result<String, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join(loader);
    let path = fetch_to_cache(url, &cache_dir, &format!("{}-loader.json", instance.minecraft))?;
    fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))
}

pub(super) fn fabric_meta_url() -> Option<String> {
    env::var("MODSTAGE_FABRIC_META_URL").ok()
}

pub(super) fn installer_loader_meta_url(loader: &str) -> Option<String> {
    match loader {
        "forge" => env::var("MODSTAGE_FORGE_META_URL").ok(),
        "neoforge" => env::var("MODSTAGE_NEOFORGE_META_URL").ok(),
        _ => None,
    }
}

pub(super) fn resolve_minecraft_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<MinecraftMetadata>, String> {
    let Some(manifest_url) = mojang_manifest_url() else {
        return Ok(None);
    };
    let manifest_is_override = env::var("MODSTAGE_MOJANG_MANIFEST_URL").is_ok();
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("mojang");
    let manifest_path = fetch_to_cache(&manifest_url, &cache_dir, "version_manifest.json")?;
    let manifest = fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest_text = String::from_utf8_lossy(&manifest);
    let Some(version_url) = manifest_version_url(&manifest_text, &instance.minecraft) else {
        if manifest_is_override {
            return Err(format!("Minecraft version `{}` not found in manifest", instance.minecraft));
        }

        return Ok(None);
    };
    let version_path = fetch_to_cache(&version_url, &cache_dir, &format!("{}.json", instance.minecraft))?;
    let version = fs::read(&version_path)
        .map_err(|error| format!("failed to read {}: {error}", version_path.display()))?;
    let version_text = String::from_utf8_lossy(&version);
    let java_major = json_u32(&version_text, "majorVersion").unwrap_or(8);
    let client_url = json_object_string(&version_text, "client", "url")
        .ok_or_else(|| format!("Minecraft version `{}` has no client download URL", instance.minecraft))?;
    let server_url = json_object_string(&version_text, "server", "url")
        .ok_or_else(|| format!("Minecraft version `{}` has no server download URL", instance.minecraft))?;
    let client_path = fetch_to_cache(&client_url, &cache_dir, &format!("{}-client.jar", instance.minecraft))?;
    let server_path = fetch_to_cache(&server_url, &cache_dir, &format!("{}-server.jar", instance.minecraft))?;
    let client = fs::read(&client_path)
        .map_err(|error| format!("failed to read {}: {error}", client_path.display()))?;
    let server = fs::read(&server_path)
        .map_err(|error| format!("failed to read {}: {error}", server_path.display()))?;
    let libraries = resolve_minecraft_libraries(&version_text, &cache_dir)?;
    let assets = resolve_minecraft_assets(&version_text, &cache_dir)?;

    Ok(Some(MinecraftMetadata {
        manifest_url,
        manifest_sha256: sha256_hex(&manifest),
        version_url,
        version_sha256: sha256_hex(&version),
        java_major,
        main_class: json_string(&version_text, "mainClass"),
        client_url,
        client_sha256: sha256_hex(&client),
        server_url,
        server_sha256: sha256_hex(&server),
        libraries,
        assets,
    }))
}

pub(super) fn resolve_minecraft_assets(version_text: &str, cache_dir: &Path) -> Result<Option<MinecraftAssets>, String> {
    let Some(asset_index) = json_object_after(version_text, "assetIndex") else {
        return Ok(None);
    };
    let Some(id) = json_string(asset_index, "id") else {
        return Ok(None);
    };
    let Some(index_url) = json_string(asset_index, "url") else {
        return Ok(None);
    };

    let index_path = fetch_to_cache(&index_url, &cache_dir.join("assets").join("indexes"), &format!("{id}.json"))?;
    let index = fs::read(&index_path)
        .map_err(|error| format!("failed to read {}: {error}", index_path.display()))?;
    let index_text = String::from_utf8_lossy(&index);
    let mut objects = Vec::new();

    for block in minecraft_asset_blocks(&index_text) {
        let Some(name) = asset_name(block) else {
            continue;
        };
        let Some(hash) = json_string(block, "hash") else {
            continue;
        };
        let size = json_u32(block, "size").unwrap_or(0);
        let Some(url) = json_string(block, "url") else {
            continue;
        };
        let object_path = fetch_to_cache(&url, &asset_object_dir(cache_dir, &hash), &hash)?;
        let bytes = fs::read(&object_path)
            .map_err(|error| format!("failed to read {}: {error}", object_path.display()))?;

        objects.push(MinecraftAsset {
            name,
            hash,
            size,
            url,
            sha256: sha256_hex(&bytes),
        });
    }

    Ok(Some(MinecraftAssets {
        id,
        index_url,
        index_sha256: sha256_hex(&index),
        objects,
    }))
}

pub(super) fn asset_object_dir(cache_dir: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or(hash);
    cache_dir.join("assets").join("objects").join(prefix)
}

pub(super) fn minecraft_asset_blocks(index_text: &str) -> Vec<&str> {
    let Some(objects_start) = index_text.find("\"objects\"") else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    let mut rest = &index_text[objects_start..];

    while let Some(hash_position) = rest.find("\"hash\"") {
        let before_hash = &rest[..hash_position];
        let Some(object_start) = before_hash.rfind('{') else {
            break;
        };
        let Some(name_end) = before_hash[..object_start].rfind('"') else {
            break;
        };
        let Some(name_start) = before_hash[..name_end].rfind('"') else {
            break;
        };
        let block = &rest[name_start..];
        blocks.push(block);
        rest = &rest[hash_position + "\"hash\"".len()..];
    }

    blocks
}

pub(super) fn asset_name(block: &str) -> Option<String> {
    let first = block.strip_prefix('"')?;
    let end = first.find('"')?;
    Some(first[..end].to_string())
}

pub(super) fn resolve_minecraft_libraries(version_text: &str, cache_dir: &Path) -> Result<Vec<MinecraftLibrary>, String> {
    let mut libraries = Vec::new();

    for block in minecraft_library_blocks(version_text) {
        let Some(name) = json_string(block, "name") else {
            continue;
        };
        let Some(artifact) = json_object_after(block, "artifact") else {
            continue;
        };
        let Some(path) = json_string(artifact, "path") else {
            continue;
        };
        let Some(url) = json_string(artifact, "url") else {
            continue;
        };
        let file_name = path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("library.jar");
        let library_path = fetch_to_cache(&url, &cache_dir.join("libraries"), file_name)?;
        let bytes = fs::read(&library_path)
            .map_err(|error| format!("failed to read {}: {error}", library_path.display()))?;

        libraries.push(MinecraftLibrary {
            name,
            path,
            url,
            sha256: sha256_hex(&bytes),
        });
    }

    Ok(libraries)
}

pub(super) fn minecraft_library_blocks(version_text: &str) -> Vec<&str> {
    let Some(libraries_start) = version_text.find("\"libraries\"") else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    let mut rest = &version_text[libraries_start..];

    while let Some(name_position) = rest.find("\"name\"") {
        let before_name = &rest[..name_position];
        let Some(block_start) = before_name.rfind('{') else {
            break;
        };
        let block = &rest[block_start..];
        blocks.push(block);
        rest = &rest[name_position + "\"name\"".len()..];
    }

    blocks
}

pub(super) fn json_object_after<'a>(text: &'a str, object_key: &str) -> Option<&'a str> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    Some(&text[object_start..])
}

pub(super) fn mojang_manifest_url() -> Option<String> {
    Some(env::var("MODSTAGE_MOJANG_MANIFEST_URL").unwrap_or_else(|_| {
        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".to_string()
    }))
}

pub(super) fn fetch_to_cache(url: &str, cache_dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_dir)
        .map_err(|error| format!("failed to create {}: {error}", cache_dir.display()))?;
    let destination = cache_dir.join(file_name);

    if let Some(path) = url.strip_prefix("file://") {
        fs::copy(path, &destination)
            .map_err(|error| format!("failed to copy {url} to {}: {error}", destination.display()))?;
        return Ok(destination);
    }

    if url.starts_with("https://") || url.starts_with("http://") {
        let status = Command::new("curl")
            .args(["--fail", "--location", "--silent", "--show-error", "--output"])
            .arg(&destination)
            .arg(url)
            .status()
            .map_err(|error| format!("failed to run curl for {url}: {error}"))?;

        if status.success() {
            return Ok(destination);
        }

        return Err(format!("curl failed for {url} with status {status}"));
    }

    Err(format!("unsupported URL `{url}`"))
}

pub(super) fn manifest_version_url(manifest: &str, version: &str) -> Option<String> {
    let id_key = manifest.find(&format!("\"{version}\""))?;
    let before_id = &manifest[..id_key];
    let id_field = before_id.rfind("\"id\"")?;
    let after_version = &manifest[id_field..];
    json_string(after_version, "url")
}

pub(super) fn json_string(text: &str, key: &str) -> Option<String> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub(super) fn json_object_string(text: &str, object_key: &str, value_key: &str) -> Option<String> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    json_string(&text[object_start..], value_key)
}

pub(super) fn json_u32(text: &str, key: &str) -> Option<u32> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}
