use super::*;

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

    Ok(Some(LoaderMetadata {
        kind: "fabric".to_string(),
        version: json_string(&metadata, "version").unwrap_or_else(|| {
            instance
                .loader_version
                .clone()
                .unwrap_or_else(|| "latest".to_string())
        }),
        loader_maven: Some(
            json_object_string(&metadata, "loader", "maven").ok_or_else(|| {
                "Fabric metadata did not include loader maven coordinate".to_string()
            })?,
        ),
        intermediary_maven: Some(
            json_object_string(&metadata, "intermediary", "maven").ok_or_else(|| {
                "Fabric metadata did not include intermediary maven coordinate".to_string()
            })?,
        ),
        installer_maven: None,
        libraries: fabric_launcher_libraries(&metadata),
        client_main_class: json_object_string(&metadata, "mainClass", "client")
            .ok_or_else(|| "Fabric metadata did not include client main class".to_string())?,
        server_main_class: json_object_string(&metadata, "mainClass", "server")
            .ok_or_else(|| "Fabric metadata did not include server main class".to_string())?,
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

        return Ok(Some(LoaderMetadata {
            kind: loader.to_string(),
            version: json_string(&metadata, "version").unwrap_or_else(|| {
                instance
                    .loader_version
                    .clone()
                    .unwrap_or_else(|| "latest".to_string())
            }),
            loader_maven: None,
            intermediary_maven: None,
            installer_maven: Some(json_string(&metadata, "installer_maven").ok_or_else(|| {
                format!("{loader} metadata did not include installer maven coordinate")
            })?),
            libraries: Vec::new(),
            client_main_class: json_string(&metadata, "client_main_class")
                .ok_or_else(|| format!("{loader} metadata did not include client main class"))?,
            server_main_class: json_string(&metadata, "server_main_class")
                .ok_or_else(|| format!("{loader} metadata did not include server main class"))?,
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

pub(in crate::app) fn fabric_launcher_libraries(metadata: &str) -> Vec<LoaderLibrary> {
    let mut libraries = Vec::new();

    for side in ["common", "client", "server"] {
        let Some(section) = launcher_libraries_section(metadata, side) else {
            continue;
        };

        for block in json_object_blocks(section) {
            let Some(name) = json_string(block, "name") else {
                continue;
            };
            let Some(url) = json_string(block, "url") else {
                continue;
            };
            libraries.push(LoaderLibrary {
                side: side.to_string(),
                name,
                url,
            });
        }
    }

    libraries
}

pub(in crate::app) fn launcher_libraries_section<'a>(metadata: &'a str, side: &str) -> Option<&'a str> {
    let libraries_start = metadata.find("\"libraries\"")?;
    let libraries = &metadata[libraries_start..];
    let side_start = libraries.find(&format!("\"{side}\""))?;
    let side_text = &libraries[side_start..];
    let array_start = side_text.find('[')?;
    let array = &side_text[array_start + 1..];
    let mut depth = 1_i32;

    for (index, character) in array.char_indices() {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&array[..index]);
                }
            }
            _ => {}
        }
    }

    None
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

    let installer_maven = match loader {
        "neoforge" => format!("net.neoforged:neoforge:{version}:installer"),
        "forge" => format!("net.minecraftforge:forge:{version}:installer"),
        _ => return Ok(None),
    };

    Ok(Some(LoaderMetadata {
        kind: loader.to_string(),
        version: version.to_string(),
        loader_maven: None,
        intermediary_maven: None,
        installer_maven: Some(installer_maven),
        libraries: Vec::new(),
        client_main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_string(),
        server_main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".to_string(),
    }))
}
