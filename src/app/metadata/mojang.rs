use super::*;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct MojangManifest {
    versions: Vec<MojangManifestVersion>,
}

#[derive(Deserialize)]
struct MojangManifestVersion {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct MojangVersion {
    #[serde(rename = "javaVersion")]
    java_version: Option<MojangJavaVersion>,
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    downloads: MojangDownloads,
    libraries: Option<Vec<MojangLibraryEntry>>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<MojangAssetIndex>,
}

#[derive(Deserialize)]
struct MojangJavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

#[derive(Deserialize)]
struct MojangDownloads {
    client: Option<MojangDownload>,
    server: Option<MojangDownload>,
}

#[derive(Deserialize)]
struct MojangDownload {
    url: String,
}

#[derive(Deserialize)]
struct MojangLibraryEntry {
    name: String,
    downloads: Option<MojangLibraryDownloads>,
}

#[derive(Deserialize)]
struct MojangLibraryDownloads {
    artifact: Option<MojangLibraryArtifact>,
}

#[derive(Deserialize)]
struct MojangLibraryArtifact {
    path: String,
    url: String,
}

#[derive(Deserialize)]
struct MojangAssetIndex {
    id: String,
    url: String,
}

#[derive(Deserialize)]
struct MojangAssetIndexObjects {
    objects: BTreeMap<String, MojangAssetObject>,
}

#[derive(Deserialize)]
struct MojangAssetObject {
    hash: String,
    size: u32,
    url: Option<String>,
}

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
    let version_json: MojangVersion = serde_json::from_slice(&version)
        .map_err(|error| format!("failed to parse Minecraft version metadata: {error}"))?;
    let java_major = version_json
        .java_version
        .as_ref()
        .map(|java| java.major_version)
        .unwrap_or(8);
    let client_url = version_json
        .downloads
        .client
        .as_ref()
        .map(|download| download.url.clone())
        .ok_or_else(|| {
            format!(
                "Minecraft version `{}` has no client download URL",
                instance.minecraft
            )
        })?;
    let server_url = version_json
        .downloads
        .server
        .as_ref()
        .map(|download| download.url.clone())
        .ok_or_else(|| {
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
    let libraries = resolve_minecraft_libraries_from_version(&version_json, &cache_dir)?;
    let assets = resolve_minecraft_assets_from_version(&version_json, &cache_dir)?;

    Ok(Some(MinecraftMetadata {
        manifest_url,
        manifest_sha256: sha256_hex(&manifest),
        version_url,
        version_sha256: sha256_hex(&version),
        java_major,
        client_url,
        client_sha256: sha256_hex(&client),
        server_url,
        server_sha256: sha256_hex(&server),
        main_class: version_json.main_class,
        libraries,
        assets,
    }))
}

fn resolve_minecraft_assets_from_version(
    version_json: &MojangVersion,
    cache_dir: &Path,
) -> Result<Option<MinecraftAssets>, String> {
    let Some(asset_index) = &version_json.asset_index else {
        return Ok(None);
    };
    let id = &asset_index.id;
    let index_url = &asset_index.url;

    let index_path = fetch_to_cache(
        index_url,
        &cache_dir.join("assets").join("indexes"),
        &format!("{}.json", id),
    )?;
    let index = fs::read(&index_path)
        .map_err(|error| format!("failed to read {}: {error}", index_path.display()))?;
    let index_json: MojangAssetIndexObjects = serde_json::from_slice(&index)
        .map_err(|error| format!("failed to parse Minecraft asset index: {error}"))?;
    let objects = index_json
        .objects
        .into_iter()
        .map(|(name, object)| {
            let url = object
                .url
                .unwrap_or_else(|| minecraft_asset_url(&object.hash));
            MinecraftAsset {
                name,
                hash: object.hash,
                size: object.size,
                url,
            }
        })
        .collect();

    Ok(Some(MinecraftAssets {
        id: id.clone(),
        index_url: index_url.clone(),
        index_sha256: sha256_hex(&index),
        objects,
    }))
}

pub(in crate::app) fn asset_object_dir(cache_dir: &Path, hash: &str) -> PathBuf {
    let prefix = hash.get(..2).unwrap_or(hash);
    cache_dir.join("assets").join("objects").join(prefix)
}

pub(in crate::app) fn minecraft_asset_url(hash: &str) -> String {
    let base = std::env::var("MODSTAGE_MOJANG_ASSET_BASE_URL")
        .unwrap_or_else(|_| "https://resources.download.minecraft.net".to_string());
    let base = base.trim_end_matches('/');
    let prefix = hash.get(..2).unwrap_or(hash);

    format!("{base}/{prefix}/{hash}")
}

fn resolve_minecraft_libraries_from_version(
    version_json: &MojangVersion,
    cache_dir: &Path,
) -> Result<Vec<MinecraftLibrary>, String> {
    let mut libraries = Vec::new();

    for library in version_json.libraries.as_deref().unwrap_or_default() {
        let Some(artifact) = library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
        else {
            continue;
        };
        let file_name = artifact
            .path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("library.jar");
        let library_path = fetch_to_cache(&artifact.url, &cache_dir.join("libraries"), file_name)?;
        let bytes = fs::read(&library_path)
            .map_err(|error| format!("failed to read {}: {error}", library_path.display()))?;

        libraries.push(MinecraftLibrary {
            name: library.name.clone(),
            path: artifact.path.clone(),
            url: artifact.url.clone(),
            sha256: sha256_hex(&bytes),
        });
    }

    Ok(libraries)
}

pub(in crate::app) fn mojang_manifest_url() -> Option<String> {
    Some(
        env::var("MODSTAGE_MOJANG_MANIFEST_URL").unwrap_or_else(|_| {
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".to_string()
        }),
    )
}

pub(in crate::app) fn manifest_version_url(manifest: &str, version: &str) -> Option<String> {
    let manifest: MojangManifest = serde_json::from_str(manifest).ok()?;
    manifest
        .versions
        .into_iter()
        .find(|candidate| candidate.id == version)
        .map(|candidate| candidate.url)
}
