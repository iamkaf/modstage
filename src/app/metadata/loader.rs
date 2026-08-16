use super::*;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;

const LOADER_METADATA_TTL: Duration = Duration::from_secs(300);
const FORGE_MAVEN_METADATA_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/maven-metadata.json";
const NEOFORGE_MAVEN_METADATA_URL: &str =
    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml";
const NEOFORGE_LEGACY_MAVEN_METADATA_URL: &str =
    "https://maven.neoforged.net/releases/net/neoforged/forge/maven-metadata.xml";

#[derive(Deserialize)]
struct FabricMetadata {
    loader: FabricComponent,
    intermediary: FabricComponent,
    #[serde(rename = "launcherMeta")]
    launcher_meta: FabricLauncherMeta,
}

#[derive(Deserialize)]
struct FabricComponent {
    maven: String,
    version: Option<String>,
}

#[derive(Deserialize)]
struct FabricLauncherMeta {
    libraries: Option<FabricLibraries>,
    #[serde(rename = "mainClass")]
    main_class: FabricMainClass,
}

#[derive(Deserialize)]
struct FabricLibraries {
    common: Option<Vec<FabricLibrary>>,
    client: Option<Vec<FabricLibrary>>,
    server: Option<Vec<FabricLibrary>>,
}

#[derive(Deserialize)]
struct FabricLibrary {
    name: String,
    url: String,
}

#[derive(Deserialize)]
struct FabricMainClass {
    client: String,
    server: String,
}

#[derive(Deserialize)]
struct InstallerLoaderMetadata {
    version: Option<String>,
    installer_maven: String,
    client_main_class: String,
    server_main_class: String,
}

pub(in crate::app) struct LoaderMetadata {
    pub(in crate::app) kind: String,
    pub(in crate::app) version: String,
    pub(in crate::app) loader_maven: Option<String>,
    pub(in crate::app) intermediary_maven: Option<String>,
    pub(in crate::app) installer_maven: Option<String>,
    pub(in crate::app) libraries: Vec<LoaderLibrary>,
    pub(in crate::app) client_main_class: String,
    pub(in crate::app) server_main_class: String,
}

pub(in crate::app) struct LoaderLibrary {
    pub(in crate::app) side: String,
    pub(in crate::app) name: String,
    pub(in crate::app) url: String,
}

pub(in crate::app) fn resolve_loader_metadata(
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

pub(in crate::app) fn resolve_fabric_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    let url = fabric_meta_url(instance);
    let metadata = loader_metadata_text(config, root, "fabric", &url, instance)?;
    let parsed = parse_fabric_metadata(&metadata)?;

    Ok(Some(LoaderMetadata {
        kind: "fabric".to_string(),
        version: parsed.loader.version.clone().unwrap_or_else(|| {
            instance
                .loader_version
                .clone()
                .unwrap_or_else(|| "latest".to_string())
        }),
        loader_maven: Some(parsed.loader.maven),
        intermediary_maven: Some(parsed.intermediary.maven),
        installer_maven: None,
        libraries: fabric_launcher_libraries(parsed.launcher_meta.libraries),
        client_main_class: parsed.launcher_meta.main_class.client,
        server_main_class: parsed.launcher_meta.main_class.server,
    }))
}

pub(in crate::app) fn resolve_neoforge_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    resolve_installer_loader_metadata(config, instance, root, "neoforge")
}

pub(in crate::app) fn resolve_installer_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
    loader: &str,
) -> Result<Option<LoaderMetadata>, String> {
    if let Some(url) = installer_loader_meta_url(loader) {
        let metadata = loader_metadata_text(config, root, loader, &url, instance)?;
        let parsed: InstallerLoaderMetadata = serde_json::from_str(&metadata)
            .map_err(|error| format!("failed to parse {loader} metadata: {error}"))?;

        return Ok(Some(LoaderMetadata {
            kind: loader.to_string(),
            version: parsed.version.unwrap_or_else(|| {
                instance
                    .loader_version
                    .clone()
                    .unwrap_or_else(|| "latest".to_string())
            }),
            loader_maven: None,
            intermediary_maven: None,
            installer_maven: Some(parsed.installer_maven),
            libraries: Vec::new(),
            client_main_class: parsed.client_main_class,
            server_main_class: parsed.server_main_class,
        }));
    }

    if instance.loader_version.as_deref().unwrap_or("latest") == "latest" {
        return first_party_latest_installer_loader_metadata(config, instance, root, loader);
    }

    pinned_installer_loader_metadata(loader, instance)
}

pub(in crate::app) fn loader_metadata_text(
    config: &Config,
    root: &Path,
    loader: &str,
    url: &str,
    instance: &Instance,
) -> Result<String, String> {
    cached_loader_metadata(
        config,
        root,
        loader,
        url,
        &loader_metadata_cache_name(instance),
    )
}

fn loader_metadata_cache_name(instance: &Instance) -> String {
    let version = requested_loader_version(instance)
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("{}-{}-loader.json", instance.minecraft, version)
}

pub(in crate::app) fn requested_loader_version(instance: &Instance) -> &str {
    instance
        .loader_version
        .as_deref()
        .filter(|version| !version.is_empty() && *version != "latest")
        .unwrap_or("latest")
}

fn cached_loader_metadata(
    config: &Config,
    root: &Path,
    loader: &str,
    url: &str,
    file_name: &str,
) -> Result<String, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join(loader);
    let path = fetch_to_cache_with_ttl(url, &cache_dir, file_name, LOADER_METADATA_TTL)?;
    fs::read_to_string(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))
}

pub(in crate::app) fn fabric_meta_url(instance: &Instance) -> String {
    if let Ok(url) = env::var("MODSTAGE_FABRIC_META_URL") {
        return url;
    }

    match instance.loader_version.as_deref() {
        Some(version) if version != "latest" => {
            format!(
                "https://meta.fabricmc.net/v2/versions/loader/{}/{}",
                instance.minecraft, version
            )
        }
        _ => format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}",
            instance.minecraft
        ),
    }
}

fn fabric_launcher_libraries(parsed: Option<FabricLibraries>) -> Vec<LoaderLibrary> {
    let mut libraries = Vec::new();

    let Some(parsed) = parsed else {
        return Vec::new();
    };

    append_fabric_libraries(&mut libraries, "common", parsed.common);
    append_fabric_libraries(&mut libraries, "client", parsed.client);
    append_fabric_libraries(&mut libraries, "server", parsed.server);

    libraries
}

fn parse_fabric_metadata(metadata: &str) -> Result<FabricMetadata, String> {
    match serde_json::from_str::<FabricMetadata>(metadata) {
        Ok(metadata) => Ok(metadata),
        Err(object_error) => {
            let mut versions: Vec<Value> = serde_json::from_str(metadata)
                .map_err(|_| format!("failed to parse Fabric metadata: {object_error}"))?;
            versions
                .drain(..)
                .next()
                .ok_or_else(|| "Fabric metadata did not include any versions".to_string())
                .and_then(|version| {
                    serde_json::from_value(version)
                        .map_err(|error| format!("failed to parse Fabric metadata: {error}"))
                })
        }
    }
}

fn append_fabric_libraries(
    libraries: &mut Vec<LoaderLibrary>,
    side: &str,
    side_libraries: Option<Vec<FabricLibrary>>,
) {
    for library in side_libraries.unwrap_or_default() {
        libraries.push(LoaderLibrary {
            side: side.to_string(),
            name: library.name,
            url: library.url,
        });
    }
}

pub(in crate::app) fn installer_loader_meta_url(loader: &str) -> Option<String> {
    match loader {
        "forge" => env::var("MODSTAGE_FORGE_META_URL").ok(),
        "neoforge" => env::var("MODSTAGE_NEOFORGE_META_URL").ok(),
        _ => None,
    }
}

fn first_party_latest_installer_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
    loader: &str,
) -> Result<Option<LoaderMetadata>, String> {
    match loader {
        "forge" => latest_forge_loader_metadata(config, instance, root),
        "neoforge" => latest_neoforge_loader_metadata(config, instance, root),
        _ => Ok(None),
    }
}

fn latest_forge_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    let metadata_url =
        env::var("MODSTAGE_FORGE_MAVEN_METADATA_URL").unwrap_or(FORGE_MAVEN_METADATA_URL.into());
    let metadata = cached_loader_metadata(
        config,
        root,
        "forge",
        &metadata_url,
        &format!("{}-loader-manifest.json", instance.minecraft),
    )?;
    let manifest: HashMap<String, Vec<String>> = serde_json::from_str(&metadata)
        .map_err(|error| format!("failed to parse Forge metadata: {error}"))?;
    let version = manifest
        .get(&instance.minecraft)
        .and_then(|versions| {
            versions
                .iter()
                .max_by(|left, right| compare_loader_versions(left, right))
        })
        .cloned()
        .ok_or_else(|| {
            format!(
                "Forge metadata did not include loader versions for Minecraft {}",
                instance.minecraft
            )
        })?;
    pinned_installer_loader_metadata_for_version("forge", instance, &version)
}

fn latest_neoforge_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    let source = neoforge_metadata_source(&instance.minecraft);
    let metadata_url =
        env::var(source.override_env).unwrap_or_else(|_| source.default_url.to_string());
    let metadata = cached_loader_metadata(
        config,
        root,
        "neoforge",
        &metadata_url,
        &format!("{}-loader-manifest.xml", instance.minecraft),
    )?;
    let version =
        latest_neoforge_version(&metadata, source, &instance.minecraft).ok_or_else(|| {
            format!(
                "NeoForge metadata did not include loader versions for Minecraft {}",
                instance.minecraft
            )
        })?;
    pinned_installer_loader_metadata_for_version(source.artifact_kind, instance, &version)
}

#[derive(Clone, Copy)]
struct NeoForgeMetadataSource {
    default_url: &'static str,
    override_env: &'static str,
    artifact_kind: &'static str,
    legacy_1201: bool,
}

fn neoforge_metadata_source(minecraft: &str) -> NeoForgeMetadataSource {
    if minecraft == "1.20.1" {
        NeoForgeMetadataSource {
            default_url: NEOFORGE_LEGACY_MAVEN_METADATA_URL,
            override_env: "MODSTAGE_NEOFORGE_LEGACY_MAVEN_METADATA_URL",
            artifact_kind: "neoforge-legacy",
            legacy_1201: true,
        }
    } else {
        NeoForgeMetadataSource {
            default_url: NEOFORGE_MAVEN_METADATA_URL,
            override_env: "MODSTAGE_NEOFORGE_MAVEN_METADATA_URL",
            artifact_kind: "neoforge",
            legacy_1201: false,
        }
    }
}

fn latest_neoforge_version(
    metadata: &str,
    source: NeoForgeMetadataSource,
    minecraft: &str,
) -> Option<String> {
    maven_metadata_versions(metadata)
        .into_iter()
        .filter(|version| {
            source.legacy_1201 || neoforge_minecraft_version(version).as_deref() == Some(minecraft)
        })
        .max_by(|left, right| compare_loader_versions(left, right))
}

fn maven_metadata_versions(metadata: &str) -> Vec<String> {
    metadata
        .split("<version>")
        .skip(1)
        .filter_map(|part| {
            part.split_once("</version>")
                .map(|(version, _)| version.trim())
        })
        .filter(|version| !version.is_empty())
        .map(str::to_string)
        .collect()
}

fn neoforge_minecraft_version(version: &str) -> Option<String> {
    if version.contains("25w") {
        let snapshot = version
            .trim_start_matches("0.")
            .split('.')
            .next()
            .filter(|part| !part.is_empty())?;
        return Some(format!("1.0.{snapshot}"));
    }

    let core = version.split('-').next().unwrap_or(version);
    let mut parts = core.split('.');
    let major_or_year = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?;

    if major_or_year >= 26 {
        let hotfix = parts.next()?;
        if hotfix == "0" {
            Some(format!("{major_or_year}.{minor}"))
        } else {
            Some(format!("{major_or_year}.{minor}.{hotfix}"))
        }
    } else if minor == "0" {
        Some(format!("1.{major_or_year}"))
    } else {
        Some(format!("1.{major_or_year}.{minor}"))
    }
}

fn compare_loader_versions(left: &str, right: &str) -> std::cmp::Ordering {
    loader_version_key(left).cmp(&loader_version_key(right))
}

fn loader_version_key(version: &str) -> Vec<u32> {
    let version = loader_version_for_ordering(version);
    version
        .split(|char: char| !char.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

fn loader_version_for_ordering(version: &str) -> &str {
    if let Some((minecraft, loader)) = version.split_once('-')
        && minecraft
            .chars()
            .all(|char| char.is_ascii_digit() || char == '.')
        && loader
            .chars()
            .next()
            .is_some_and(|char| char.is_ascii_digit())
    {
        loader
    } else {
        version
    }
}

pub(in crate::app) fn pinned_installer_loader_metadata(
    loader: &str,
    instance: &Instance,
) -> Result<Option<LoaderMetadata>, String> {
    let Some(version) = instance.loader_version.as_deref() else {
        return Ok(None);
    };
    if version == "latest" {
        return Ok(None);
    }
    pinned_installer_loader_metadata_for_version(loader, instance, version)
}

fn pinned_installer_loader_metadata_for_version(
    loader: &str,
    instance: &Instance,
    version: &str,
) -> Result<Option<LoaderMetadata>, String> {
    let artifact_version = installer_artifact_version(loader, &instance.minecraft, version);

    let installer_maven = match loader {
        "neoforge" => format!("net.neoforged:neoforge:{artifact_version}:installer"),
        "neoforge-legacy" => format!("net.neoforged:forge:{artifact_version}:installer"),
        "forge" => format!("net.minecraftforge:forge:{artifact_version}:installer"),
        _ => return Ok(None),
    };

    Ok(Some(LoaderMetadata {
        kind: if loader == "neoforge-legacy" {
            "neoforge"
        } else {
            loader
        }
        .to_string(),
        version: artifact_version,
        loader_maven: None,
        intermediary_maven: None,
        installer_maven: Some(installer_maven),
        libraries: Vec::new(),
        client_main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_string(),
        server_main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_string(),
    }))
}

fn installer_artifact_version(loader: &str, minecraft: &str, version: &str) -> String {
    if loader == "forge" && !version.contains('-') {
        format!("{minecraft}-{version}")
    } else {
        version.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_instance(loader_version: Option<&str>) -> Instance {
        Instance {
            name: "fabric-26.1.2".to_string(),
            minecraft: "26.1.2".to_string(),
            loader: "fabric".to_string(),
            loader_version: loader_version.map(str::to_string),
            sides: vec!["server".to_string()],
            modrinth_pack: None,
            server_properties: Vec::new(),
            mods: Vec::new(),
            fixtures: Vec::new(),
        }
    }

    #[test]
    fn loader_metadata_cache_names_include_the_requested_version() {
        assert_eq!(
            loader_metadata_cache_name(&test_instance(None)),
            "26.1.2-latest-loader.json"
        );
        assert_eq!(
            loader_metadata_cache_name(&test_instance(Some("latest"))),
            "26.1.2-latest-loader.json"
        );
        assert_eq!(
            loader_metadata_cache_name(&test_instance(Some("0.19.3"))),
            "26.1.2-0.19.3-loader.json"
        );
        assert_eq!(
            loader_metadata_cache_name(&test_instance(Some("0.19.3/rc"))),
            "26.1.2-0.19.3_rc-loader.json"
        );
    }

    #[test]
    fn neoforge_version_numbers_map_to_minecraft_versions() {
        assert_eq!(
            neoforge_minecraft_version("21.1.231").as_deref(),
            Some("1.21.1")
        );
        assert_eq!(
            neoforge_minecraft_version("26.1.0.16").as_deref(),
            Some("26.1")
        );
        assert_eq!(
            neoforge_minecraft_version("26.1.2.66-beta").as_deref(),
            Some("26.1.2")
        );
        assert_eq!(
            neoforge_minecraft_version("0.25w14craftmine.5-beta").as_deref(),
            Some("1.0.25w14craftmine")
        );
    }

    #[test]
    fn loader_version_ordering_ignores_minecraft_prefix_but_keeps_suffix_numbers() {
        assert_eq!(loader_version_for_ordering("1.20.1-47.1.106"), "47.1.106");
        assert_eq!(
            loader_version_for_ordering("26.1.2.66-beta"),
            "26.1.2.66-beta"
        );
        assert_eq!(
            compare_loader_versions("1.20.1-47.1.106", "47.1.82"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_loader_versions("26.1.2.66-beta", "26.1.2.21-beta"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn latest_neoforge_version_reads_modern_and_legacy_metadata() {
        let modern = r#"<metadata><versioning><versions>
<version>21.1.231</version>
<version>26.1.2.21-beta</version>
<version>26.1.2.66-beta</version>
</versions></versioning></metadata>"#;
        let legacy = r#"<metadata><versioning><versions>
<version>1.20.1-47.1.106</version>
<version>47.1.82</version>
</versions></versioning></metadata>"#;

        assert_eq!(
            latest_neoforge_version(modern, neoforge_metadata_source("26.1.2"), "26.1.2")
                .as_deref(),
            Some("26.1.2.66-beta")
        );
        assert_eq!(
            latest_neoforge_version(legacy, neoforge_metadata_source("1.20.1"), "1.20.1")
                .as_deref(),
            Some("1.20.1-47.1.106")
        );
    }

    #[test]
    fn fabric_latest_metadata_only_requires_the_first_version_to_be_launchable() {
        let metadata = r#"[
  {
    "loader": {
      "maven": "net.fabricmc:fabric-loader:0.19.2",
      "version": "0.19.2"
    },
    "intermediary": {
      "maven": "net.fabricmc:intermediary:0.0.0",
      "version": "0.0.0"
    },
    "launcherMeta": {
      "libraries": {
        "common": [
          {
            "name": "org.example:library:1.0.0",
            "url": "https://maven.fabricmc.net/"
          }
        ],
        "client": [],
        "server": []
      },
      "mainClass": {
        "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
        "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
      }
    }
  },
  {
    "loader": {
      "version": "old-entry-without-maven"
    },
    "intermediary": {
      "version": "old-entry-without-maven"
    },
    "launcherMeta": {
      "mainClass": {
        "client": "unused",
        "server": "unused"
      }
    }
  }
]"#;

        let parsed = parse_fabric_metadata(metadata).expect("first Fabric version should parse");

        assert_eq!(parsed.loader.maven, "net.fabricmc:fabric-loader:0.19.2");
        assert_eq!(parsed.loader.version.as_deref(), Some("0.19.2"));
    }
}
