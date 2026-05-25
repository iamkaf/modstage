struct MinecraftMetadata {
    manifest_url: String,
    manifest_sha256: String,
    version_url: String,
    version_sha256: String,
    java_major: u32,
    client_url: String,
    client_sha256: String,
    server_url: String,
    server_sha256: String,
}

struct LoaderMetadata {
    kind: String,
    version: String,
    loader_maven: Option<String>,
    intermediary_maven: Option<String>,
    installer_maven: Option<String>,
    client_main_class: String,
    server_main_class: String,
}

struct ModrinthMod {
    project: String,
    version_id: String,
    version_number: String,
    filename: String,
    url: String,
    path: PathBuf,
    sha1: String,
    sha512: String,
    sha256: String,
}

fn resolve_modrinth_mod(
    config: &Config,
    instance: &Instance,
    root: &Path,
    project: &str,
) -> Result<ModrinthMod, String> {
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("modrinth").join(project);
    let metadata_url = modrinth_versions_url(project, instance);
    let metadata_path = fetch_to_cache(
        &metadata_url,
        &cache_dir,
        &format!("{}-{}-{}.json", project, instance.minecraft, instance.loader),
    )?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let file_metadata = primary_modrinth_file(&metadata)
        .ok_or_else(|| format!("Modrinth project `{project}` did not include a primary file"))?;
    let filename = json_string(file_metadata, "filename")
        .ok_or_else(|| format!("Modrinth project `{project}` primary file had no filename"))?;
    let url = json_string(file_metadata, "url")
        .ok_or_else(|| format!("Modrinth project `{project}` primary file had no URL"))?;
    let path = fetch_to_cache(&url, &cache_dir, &filename)?;
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read Modrinth file {}: {error}", path.display()))?;

    Ok(ModrinthMod {
        project: project.to_string(),
        version_id: json_string(&metadata, "id")
            .ok_or_else(|| format!("Modrinth project `{project}` metadata had no version id"))?,
        version_number: json_string(&metadata, "version_number")
            .ok_or_else(|| format!("Modrinth project `{project}` metadata had no version number"))?,
        filename,
        url,
        path,
        sha1: json_object_string(file_metadata, "hashes", "sha1").unwrap_or_default(),
        sha512: json_object_string(file_metadata, "hashes", "sha512").unwrap_or_default(),
        sha256: sha256_hex(&bytes),
    })
}

fn primary_modrinth_file(metadata: &str) -> Option<&str> {
    let primary = metadata.find("\"primary\"")?;
    let file_start = metadata[..primary].rfind('{')?;
    Some(&metadata[file_start..])
}

fn modrinth_project(source: &str) -> Option<&str> {
    source.strip_prefix("modrinth:")
        .filter(|project| !project.is_empty())
}

fn modrinth_versions_url(project: &str, instance: &Instance) -> String {
    if let Ok(url) = env::var("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL") {
        return url;
    }

    format!(
        "https://api.modrinth.com/v2/project/{project}/version?loaders=%5B%22{}%22%5D&game_versions=%5B%22{}%22%5D",
        instance.loader, instance.minecraft
    )
}

fn resolve_loader_metadata(
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

fn resolve_fabric_loader_metadata(
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

fn resolve_neoforge_loader_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<LoaderMetadata>, String> {
    resolve_installer_loader_metadata(config, instance, root, "neoforge")
}

fn resolve_installer_loader_metadata(
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

fn loader_metadata_text(
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

fn fabric_meta_url() -> Option<String> {
    env::var("MODSTAGE_FABRIC_META_URL").ok()
}

fn installer_loader_meta_url(loader: &str) -> Option<String> {
    match loader {
        "forge" => env::var("MODSTAGE_FORGE_META_URL").ok(),
        "neoforge" => env::var("MODSTAGE_NEOFORGE_META_URL").ok(),
        _ => None,
    }
}

fn resolve_minecraft_metadata(
    config: &Config,
    instance: &Instance,
    root: &Path,
) -> Result<Option<MinecraftMetadata>, String> {
    let Some(manifest_url) = mojang_manifest_url() else {
        return Ok(None);
    };
    let dirs = StateDirs::for_project(&config.project_name, root)?;
    let cache_dir = dirs.cache.join("downloads").join("mojang");
    let manifest_path = fetch_to_cache(&manifest_url, &cache_dir, "version_manifest.json")?;
    let manifest = fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest_text = String::from_utf8_lossy(&manifest);
    let version_url = manifest_version_url(&manifest_text, &instance.minecraft)
        .ok_or_else(|| format!("Minecraft version `{}` not found in manifest", instance.minecraft))?;
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
    }))
}

fn mojang_manifest_url() -> Option<String> {
    env::var("MODSTAGE_MOJANG_MANIFEST_URL").ok()
}

fn fetch_to_cache(url: &str, cache_dir: &Path, file_name: &str) -> Result<PathBuf, String> {
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

fn manifest_version_url(manifest: &str, version: &str) -> Option<String> {
    let id_key = manifest.find(&format!("\"{version}\""))?;
    let before_id = &manifest[..id_key];
    let id_field = before_id.rfind("\"id\"")?;
    let after_version = &manifest[id_field..];
    json_string(after_version, "url")
}

fn json_string(text: &str, key: &str) -> Option<String> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn json_object_string(text: &str, object_key: &str, value_key: &str) -> Option<String> {
    let object_start = text.find(&format!("\"{object_key}\""))?;
    json_string(&text[object_start..], value_key)
}

fn json_u32(text: &str, key: &str) -> Option<u32> {
    let key_start = text.find(&format!("\"{key}\""))?;
    let after_key = &text[key_start + key.len() + 2..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());

    rest[..end].parse().ok()
}

