use super::*;
use rayon::prelude::*;
use serde::Deserialize;
use std::collections::HashMap;

const DEFAULT_MOJANG_ASSET_BASE: &str = "https://resources.download.minecraft.net";

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
    #[serde(default)]
    rules: Vec<LibraryRule>,
    /// Operating system to classifier, for versions that ship natives as classifier jars.
    #[serde(default)]
    natives: HashMap<String, String>,
}

#[derive(Deserialize)]
struct MojangLibraryDownloads {
    artifact: Option<MojangLibraryArtifact>,
    #[serde(default)]
    classifiers: HashMap<String, MojangLibraryArtifact>,
}

#[derive(Deserialize)]
pub(in crate::app) struct LibraryRule {
    action: String,
    os: Option<LibraryRuleOs>,
    features: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct LibraryRuleOs {
    name: Option<String>,
    arch: Option<String>,
}

#[derive(Deserialize)]
struct MojangLibraryArtifact {
    path: String,
    url: String,
    sha1: Option<String>,
}

#[derive(Deserialize)]
struct MojangAssetIndex {
    id: String,
    url: String,
}

#[derive(Clone, Deserialize)]
struct AssetIndexFile {
    objects: HashMap<String, AssetObject>,
}

#[derive(Clone, Deserialize)]
struct AssetObject {
    hash: String,
    size: Option<u64>,
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
    /// A jar of native libraries the client extracts before launch, rather than a classpath entry.
    pub(in crate::app) natives: bool,
}

pub(in crate::app) struct MinecraftAssets {
    pub(in crate::app) id: String,
    pub(in crate::app) index_url: String,
    pub(in crate::app) index_sha256: String,
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
    hydrate_asset_objects(&index_path, &cache_dir.join("assets"))?;

    Ok(Some(MinecraftAssets {
        id: id.clone(),
        index_url: index_url.clone(),
        index_sha256: sha256_hex(&index),
    }))
}

pub(in crate::app) fn hydrate_asset_objects(
    index_path: &Path,
    assets_dir: &Path,
) -> Result<usize, String> {
    let index = fs::read(index_path)
        .map_err(|error| format!("failed to read {}: {error}", index_path.display()))?;
    let parsed: AssetIndexFile = serde_json::from_slice(&index).map_err(|error| {
        format!(
            "failed to parse asset index {}: {error}",
            index_path.display()
        )
    })?;
    let count = parsed.objects.len();
    let objects_dir = assets_dir.join("objects");
    parsed
        .objects
        .into_values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .try_for_each(|object| restore_asset_object(&object, &objects_dir))?;
    Ok(count)
}

fn restore_asset_object(object: &AssetObject, objects_dir: &Path) -> Result<(), String> {
    let hash = object.hash.to_ascii_lowercase();
    if hash.len() < 2 || !hash.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(format!("invalid Minecraft asset hash `{hash}`"));
    }
    let destination = objects_dir.join(&hash[..2]).join(&hash);
    if asset_object_is_current(&destination, &hash, object.size)? {
        return Ok(());
    }
    let url = asset_object_url(object, &hash);
    fetch_to_cache(&url, destination.parent().unwrap_or(objects_dir), &hash)?;
    if !asset_object_is_current(&destination, &hash, object.size)? {
        return Err(format!(
            "Minecraft asset `{hash}` hash mismatch after download from {url}"
        ));
    }
    Ok(())
}

fn asset_object_is_current(
    path: &Path,
    hash: &str,
    expected_size: Option<u64>,
) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if expected_size.is_some_and(|size| bytes.len() as u64 != size) {
        return Ok(false);
    }
    Ok(sha1_hex(&bytes) == hash)
}

fn asset_object_url(object: &AssetObject, hash: &str) -> String {
    if let Some(url) = &object.url
        && !url.is_empty()
    {
        return url.clone();
    }
    let base = env::var("MODSTAGE_MOJANG_ASSET_BASE")
        .unwrap_or_else(|_| DEFAULT_MOJANG_ASSET_BASE.to_string());
    format!("{}/{}/{}", base.trim_end_matches('/'), &hash[..2], hash)
}

fn resolve_minecraft_libraries_from_version(
    version_json: &MojangVersion,
    cache_dir: &Path,
) -> Result<Vec<MinecraftLibrary>, String> {
    let downloads = library_downloads(
        version_json.libraries.as_deref().unwrap_or_default(),
        current_os(),
    );
    let libraries_dir = cache_dir.join("libraries");
    downloads
        .par_iter()
        .map(|(library, artifact, natives)| {
            let library_path = fetch_library(
                &artifact.url,
                &artifact.path,
                artifact.sha1.as_deref(),
                &libraries_dir,
            )?;
            let bytes = fs::read(&library_path)
                .map_err(|error| format!("failed to read {}: {error}", library_path.display()))?;

            Ok(MinecraftLibrary {
                name: library.name.clone(),
                path: artifact.path.clone(),
                url: artifact.url.clone(),
                sha256: sha256_hex(&bytes),
                natives: *natives,
            })
        })
        .collect()
}

/// The jars a launcher on `os` downloads for these libraries, in order and without repeats.
/// Versions before 1.19 list some libraries twice and ship natives as classifier jars.
fn library_downloads<'a>(
    libraries: &'a [MojangLibraryEntry],
    os: &str,
) -> Vec<(&'a MojangLibraryEntry, &'a MojangLibraryArtifact, bool)> {
    let mut seen = std::collections::HashSet::new();
    let mut downloads = Vec::new();
    for library in libraries {
        if !rules_allow(&library.rules, os) {
            continue;
        }
        let Some(library_downloads) = &library.downloads else {
            continue;
        };
        let natives = library
            .natives
            .get(os)
            .map(|classifier| classifier.replace("${arch}", "64"))
            .and_then(|classifier| library_downloads.classifiers.get(&classifier));
        for (artifact, is_natives) in [
            (library_downloads.artifact.as_ref(), false),
            (natives, true),
        ] {
            if let Some(artifact) = artifact
                && seen.insert(artifact.path.as_str())
            {
                downloads.push((library, artifact, is_natives));
            }
        }
    }
    downloads
}

/// Whether a library's launcher rules include it on `os`. Without rules a library always applies.
pub(in crate::app) fn rules_allow(rules: &[LibraryRule], os: &str) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let matches = rule.features.is_none()
            && rule.os.as_ref().is_none_or(|rule_os| {
                rule_os.name.as_deref().is_none_or(|name| name == os)
                    && rule_os
                        .arch
                        .as_deref()
                        .is_none_or(|arch| arch == env::consts::ARCH)
            });
        if matches {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

/// The operating system name launcher rules use for this machine.
pub(in crate::app) fn current_os() -> &'static str {
    match env::consts::OS {
        "macos" => "osx",
        os => os,
    }
}

/// Downloads a library into `libraries_dir` at its Maven path, the layout loader arguments expect.
/// A file already there is kept when it matches the SHA-1 the metadata declares.
pub(in crate::app) fn fetch_library(
    url: &str,
    maven_path: &str,
    sha1: Option<&str>,
    libraries_dir: &Path,
) -> Result<PathBuf, String> {
    let relative = Path::new(maven_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!(
            "library path `{maven_path}` leaves the libraries directory"
        ));
    }
    let destination = libraries_dir.join(relative);
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("library path `{maven_path}` has no file name"))?;
    if let Some(sha1) = sha1
        && fs::read(&destination).is_ok_and(|bytes| sha1_hex(&bytes).eq_ignore_ascii_case(sha1))
    {
        return Ok(destination);
    }
    fetch_to_cache(
        url,
        destination.parent().unwrap_or(libraries_dir),
        file_name,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The library shapes 1.18.2 uses: a macOS-only LWJGL, the same LWJGL listed again with
    /// natives, and a natives classifier per operating system.
    const LEGACY_LIBRARIES: &str = r#"[
      { "name": "org.lwjgl:lwjgl:3.2.1",
        "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.2.1/lwjgl-3.2.1.jar", "url": "u" } },
        "rules": [{ "action": "allow", "os": { "name": "osx" } }] },
      { "name": "org.lwjgl:lwjgl:3.2.2",
        "downloads": { "artifact": { "path": "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2.jar", "url": "u" } },
        "rules": [{ "action": "allow" }, { "action": "disallow", "os": { "name": "osx" } }] },
      { "name": "org.lwjgl:lwjgl:3.2.2",
        "downloads": {
          "artifact": { "path": "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2.jar", "url": "u" },
          "classifiers": {
            "natives-linux": { "path": "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2-natives-linux.jar", "url": "u" },
            "natives-windows": { "path": "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2-natives-windows.jar", "url": "u" }
          }
        },
        "natives": { "linux": "natives-linux", "windows": "natives-windows" },
        "rules": [{ "action": "allow" }, { "action": "disallow", "os": { "name": "osx" } }] },
      { "name": "com.mojang:text2speech:1.12.4",
        "downloads": { "classifiers": {
          "natives-linux": { "path": "com/mojang/text2speech/1.12.4/text2speech-1.12.4-natives-linux.jar", "url": "u" }
        } },
        "natives": { "linux": "natives-linux" } }
    ]"#;

    fn downloads(os: &str) -> Vec<(String, bool)> {
        let libraries: Vec<MojangLibraryEntry> =
            serde_json::from_str(LEGACY_LIBRARIES).expect("libraries should parse");
        library_downloads(&libraries, os)
            .into_iter()
            .map(|(_, artifact, natives)| (artifact.path.clone(), natives))
            .collect()
    }

    #[test]
    fn linux_gets_its_own_libraries_once_and_its_natives_as_natives() {
        assert_eq!(
            downloads("linux"),
            [
                ("org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2.jar".to_string(), false),
                (
                    "org/lwjgl/lwjgl/3.2.2/lwjgl-3.2.2-natives-linux.jar".to_string(),
                    true
                ),
                (
                    "com/mojang/text2speech/1.12.4/text2speech-1.12.4-natives-linux.jar"
                        .to_string(),
                    true
                ),
            ]
        );
    }

    #[test]
    fn macos_gets_only_the_macos_lwjgl() {
        assert_eq!(
            downloads("osx"),
            [("org/lwjgl/lwjgl/3.2.1/lwjgl-3.2.1.jar".to_string(), false)]
        );
    }

    #[test]
    fn library_paths_cannot_leave_the_libraries_directory() {
        let error = fetch_library(
            "file:///x.jar",
            "../escape.jar",
            None,
            Path::new("/libraries"),
        )
        .expect_err("a path outside the libraries directory must fail");
        assert!(error.contains("leaves the libraries directory"), "{error}");
    }
}
