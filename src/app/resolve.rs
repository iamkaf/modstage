use super::*;

pub(super) fn resolve_instance(
    explicit_config: Option<PathBuf>,
    selected: Option<&str>,
) -> Result<(), String> {
    let project = ProjectContext::load(explicit_config)?;
    let config = &project.config;
    let instances: Vec<&Instance> = match selected {
        Some(name) => config
            .instances
            .iter()
            .find(|instance| instance.name == name)
            .map(|instance| vec![instance])
            .ok_or_else(|| format!("unknown instance `{name}`"))?,
        None => {
            if config.instances.is_empty() {
                return Err("modstage.toml does not define any instances".to_string());
            }
            config.instances.iter().collect()
        }
    };
    let lock_path = project.lock_path();
    let repositories = repositories_with_builtins(&config.repositories);
    let maven_cache = project.dirs.cache.join("downloads").join("maven");
    let mut lock = LockfileWriter::new(&config.project_name, &config.repositories);
    for instance in &instances {
        resolve_instance_lock(
            &mut lock,
            config,
            instance,
            &project.root,
            &repositories,
            &maven_cache,
        )?;
    }

    fs::write(&lock_path, lock.finish())
        .map_err(|error| format!("failed to write {}: {error}", lock_path.display()))?;
    let resolved = instances
        .iter()
        .map(|instance| instance.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    println!("resolved {resolved} into {}", lock_path.display());

    Ok(())
}

pub(super) fn resolve_instance_lock(
    lock: &mut LockfileWriter,
    config: &Config,
    instance: &Instance,
    config_root: &Path,
    repositories: &[(String, String)],
    maven_cache: &Path,
) -> Result<(), String> {
    lock.begin_instance(instance);
    let metadata = resolve_minecraft_metadata(config, instance, config_root)?;
    if let Some(metadata) = metadata {
        lock.minecraft(instance, metadata);
    }
    let loader = resolve_loader_metadata(config, instance, config_root)?;
    if let Some(loader) = loader {
        let mut resolved_loader_libraries = Vec::new();
        let mut installer_profile = None;
        for coordinate in [
            loader.loader_maven.as_deref(),
            loader.intermediary_maven.as_deref(),
            loader.installer_maven.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(resolved) =
                resolved_maven_library_artifact(repositories, maven_cache, coordinate, None)?
            {
                let is_installer = loader.installer_maven.as_deref() == Some(coordinate);
                if is_installer && matches!(loader.kind.as_str(), "forge" | "neoforge") {
                    let profile_cache = maven_cache.join(&loader.kind).join("installer-profile");
                    installer_profile =
                        Some(resolve_installer_profile(&resolved.path, &profile_cache)?);
                    continue;
                }
                resolved_loader_libraries.push(resolved);
            }
        }
        let profile_main_class = installer_profile
            .as_ref()
            .and_then(|profile| profile.main_class.as_deref());
        let client_main_class = profile_main_class.unwrap_or(&loader.client_main_class);
        let server_main_class = profile_main_class.unwrap_or(&loader.server_main_class);
        lock.loader(&loader, client_main_class, server_main_class);
        if let Some(profile) = &installer_profile {
            for arg in &profile.jvm_args {
                lock.argument("jvm", arg);
            }
            for arg in &profile.game_args {
                lock.argument("game", arg);
            }
        }
        for resolved in resolved_loader_libraries {
            lock.library(resolved.library);
        }
        if let Some(profile) = installer_profile {
            for library in profile.libraries {
                lock.library(LockLibrary {
                    name: library.name,
                    repository: None,
                    side: None,
                    url: Some(library.url),
                    path: library.path,
                    sha256: library.sha256,
                });
            }
        }
        for library in &loader.libraries {
            let repository = vec![(
                format!("{}-{}", loader.kind, library.side),
                library.url.clone(),
            )];
            if let Some(entry) = resolved_maven_library_with_side(
                &repository,
                maven_cache,
                &library.name,
                Some(&library.side),
            )? {
                lock.library(entry);
            }
        }
    }
    lock.mods(&instance.mods);
    for source in &instance.mods {
        if let Some(path) = local_mod_path(config_root, source) {
            let path = path.canonicalize().map_err(|error| {
                format!("failed to resolve local mod {}: {error}", path.display())
            })?;
            let bytes = fs::read(&path)
                .map_err(|error| format!("failed to read local mod {}: {error}", path.display()))?;
            lock.local_mod(source, &path, sha256_hex(&bytes));
        } else if let Some(modrinth) = modrinth_source(source) {
            let resolved = resolve_modrinth_mod(config, instance, config_root, &modrinth)?;
            lock.modrinth_mod(source, resolved);
        } else if let Some(coordinates) = MavenCoordinates::parse(source) {
            let Some(artifact) = resolve_maven_artifact(repositories, &coordinates, maven_cache)?
            else {
                return Err(format!(
                    "failed to resolve Maven mod `{source}` from ordered repositories"
                ));
            };
            let raw_path = artifact.path;
            let path = raw_path.canonicalize().map_err(|error| {
                format!(
                    "failed to resolve Maven artifact {}: {error}",
                    raw_path.display()
                )
            })?;
            let bytes = fs::read(&path).map_err(|error| {
                format!("failed to read Maven artifact {}: {error}", path.display())
            })?;
            lock.maven_mod(
                source,
                artifact.repository,
                artifact.url,
                &path,
                sha256_hex(&bytes),
            );
        }
    }

    Ok(())
}

pub(super) fn resolved_maven_library_with_side(
    repositories: &[(String, String)],
    cache_dir: &Path,
    coordinate: &str,
    side: Option<&str>,
) -> Result<Option<LockLibrary>, String> {
    Ok(
        resolved_maven_library_artifact(repositories, cache_dir, coordinate, side)?
            .map(|resolved| resolved.library),
    )
}

pub(super) struct ResolvedMavenLibrary {
    pub(super) library: LockLibrary,
    pub(super) path: PathBuf,
}

pub(super) fn resolved_maven_library_artifact(
    repositories: &[(String, String)],
    cache_dir: &Path,
    coordinate: &str,
    side: Option<&str>,
) -> Result<Option<ResolvedMavenLibrary>, String> {
    let Some(coordinates) = MavenCoordinates::parse_coordinate(coordinate) else {
        return Ok(None);
    };
    let Some(artifact) = resolve_maven_artifact(repositories, &coordinates, cache_dir)? else {
        return Ok(None);
    };
    let raw_path = artifact.path;
    let path = raw_path.canonicalize().map_err(|error| {
        format!(
            "failed to resolve Maven artifact {}: {error}",
            raw_path.display()
        )
    })?;
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read Maven artifact {}: {error}", path.display()))?;

    Ok(Some(ResolvedMavenLibrary {
        library: LockLibrary {
            name: coordinate.to_string(),
            repository: Some(artifact.repository),
            side: side.map(str::to_string),
            url: artifact.url,
            path: path.display().to_string(),
            sha256: sha256_hex(&bytes),
        },
        path,
    }))
}

pub(super) fn repositories_with_builtins(
    repositories: &[(String, String)],
) -> Vec<(String, String)> {
    let mut result = repositories.to_vec();
    for (name, url) in [
        ("fabric", "https://maven.fabricmc.net"),
        ("forge", "https://maven.minecraftforge.net"),
        ("neoforge", "https://maven.neoforged.net/releases"),
        ("central", "https://repo.maven.apache.org/maven2"),
    ] {
        if !result.iter().any(|(existing, _)| existing == name) {
            result.push((name.to_string(), url.to_string()));
        }
    }
    result
}
