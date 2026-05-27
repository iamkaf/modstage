use super::*;
use serde::Deserialize;

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

    pinned_installer_loader_metadata(loader, instance)
}

pub(in crate::app) fn loader_metadata_text(
    config: &Config,
    root: &Path,
    loader: &str,
    url: &str,
    instance: &Instance,
) -> Result<String, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join(loader);
    let path = fetch_to_cache(
        url,
        &cache_dir,
        &format!("{}-loader.json", instance.minecraft),
    )?;
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
            let mut versions: Vec<FabricMetadata> = serde_json::from_str(metadata)
                .map_err(|_| format!("failed to parse Fabric metadata: {object_error}"))?;
            versions
                .drain(..)
                .next()
                .ok_or_else(|| "Fabric metadata did not include any versions".to_string())
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
    let artifact_version = installer_artifact_version(loader, &instance.minecraft, version);

    let installer_maven = match loader {
        "neoforge" => format!("net.neoforged:neoforge:{artifact_version}:installer"),
        "forge" => format!("net.minecraftforge:forge:{artifact_version}:installer"),
        _ => return Ok(None),
    };

    Ok(Some(LoaderMetadata {
        kind: loader.to_string(),
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
