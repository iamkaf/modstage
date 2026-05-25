use super::*;

pub(in crate::app) struct MinecraftMetadata {
    pub(in crate::app) manifest_url: String,
    pub(in crate::app) manifest_sha256: String,
    pub(in crate::app) version_url: String,
    pub(in crate::app) version_sha256: String,
    pub(in crate::app) java_major: u32,
    pub(in crate::app) main_class: Option<String>,
    pub(in crate::app) client_url: String,
    pub(in crate::app) client_sha256: String,
    pub(in crate::app) server_url: String,
    pub(in crate::app) server_sha256: String,
    pub(in crate::app) libraries: Vec<MinecraftLibrary>,
    pub(in crate::app) assets: Option<MinecraftAssets>,
}

pub(in crate::app) struct MinecraftLibrary {
    pub(in crate::app) name: String,
    pub(in crate::app) path: String,
    pub(in crate::app) url: String,
    pub(in crate::app) sha256: String,
}

pub(in crate::app) struct MinecraftAssets {
    pub(in crate::app) id: String,
    pub(in crate::app) index_url: String,
    pub(in crate::app) index_sha256: String,
    pub(in crate::app) objects: Vec<MinecraftAsset>,
}

pub(in crate::app) struct MinecraftAsset {
    pub(in crate::app) name: String,
    pub(in crate::app) hash: String,
    pub(in crate::app) size: u32,
    pub(in crate::app) url: String,
    pub(in crate::app) sha256: String,
}

pub(in crate::app) fn resolve_minecraft_metadata(
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
            return Err(format!(
                "Minecraft version `{}` not found in manifest",
                instance.minecraft
            ));
        }

        return Ok(None);
    };
    let version_path = fetch_to_cache(
        &version_url,
        &cache_dir,
        &format!("{}.json", instance.minecraft),
    )?;
    let version = fs::read(&version_path)
        .map_err(|error| format!("failed to read {}: {error}", version_path.display()))?;
    let version_text = String::from_utf8_lossy(&version);
    let java_major = json_u32(&version_text, "majorVersion").unwrap_or(8);
    let client_url = json_object_string(&version_text, "client", "url").ok_or_else(|| {
        format!(
            "Minecraft version `{}` has no client download URL",
            instance.minecraft
        )
    })?;
    let server_url = json_object_string(&version_text, "server", "url").ok_or_else(|| {
        format!(
            "Minecraft version `{}` has no server download URL",
            instance.minecraft
        )
    })?;
    let client_path = fetch_to_cache(
        &client_url,
        &cache_dir,
        &format!("{}-client.jar", instance.minecraft),
    )?;
    let server_path = fetch_to_cache(
        &server_url,
        &cache_dir,
        &format!("{}-server.jar", instance.minecraft),
    )?;
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

pub(in crate::app) fn resolve_minecraft_assets(
    version_text: &str,
    cache_dir: &Path,
) -> Result<Option<MinecraftAssets>, String> {
    let Some(asset_index) = json_object_after(version_text, "assetIndex") else {
        return Ok(None);
    };
    let Some(id) = json_string(asset_index, "id") else {
        return Ok(None);
    };
    let Some(index_url) = json_string(asset_index, "url") else {
        return Ok(None);
    };

    let index_path = fetch_to_cache(
        &index_url,
        &cache_dir.join("assets").join("indexes"),
        &format!("{id}.json"),
    )?;
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

pub(in crate::app) fn asset_object_dir(cache_dir: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or(hash);
    cache_dir.join("assets").join("objects").join(prefix)
}

pub(in crate::app) fn minecraft_asset_blocks(index_text: &str) -> Vec<&str> {
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

pub(in crate::app) fn asset_name(block: &str) -> Option<String> {
    let first = block.strip_prefix('"')?;
    let end = first.find('"')?;
    Some(first[..end].to_string())
}

pub(in crate::app) fn resolve_minecraft_libraries(
    version_text: &str,
    cache_dir: &Path,
) -> Result<Vec<MinecraftLibrary>, String> {
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

pub(in crate::app) fn minecraft_library_blocks(version_text: &str) -> Vec<&str> {
    let Some(libraries_start) = version_text.find("\"libraries\"") else {
        return Vec::new();
    };
    let libraries = &version_text[libraries_start..];
    let Some(array_start) = libraries.find('[') else {
        return Vec::new();
    };
    let array = &libraries[array_start + 1..];
    let mut depth = 1_i32;

    for (index, character) in array.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return json_object_blocks(&array[..index]);
                }
            }
            _ => {}
        }
    }

    Vec::new()
}

pub(in crate::app) fn mojang_manifest_url() -> Option<String> {
    Some(
        env::var("MODSTAGE_MOJANG_MANIFEST_URL").unwrap_or_else(|_| {
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".to_string()
        }),
    )
}

pub(in crate::app) fn manifest_version_url(manifest: &str, version: &str) -> Option<String> {
    let mut rest = manifest;

    while let Some(id_position) = rest.find("\"id\"") {
        let candidate = &rest[id_position..];
        if json_string(candidate, "id").as_deref() == Some(version) {
            return json_string(candidate, "url");
        }
        rest = &candidate["\"id\"".len()..];
    }

    None
}
