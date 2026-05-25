use super::*;

pub(super) fn verify_locked_mod_hashes(
    root: &Path,
    instance: &Instance,
    cache_dir: &Path,
) -> Result<(), String> {
    for source in &instance.mods {
        let Some((path, expected)) =
            restore_locked_mod_path_and_hash(root, &instance.name, source, cache_dir)?
        else {
            continue;
        };
        let bytes = fs::read(&path)
            .map_err(|error| format!("failed to read locked mod {}: {error}", path.display()))?;
        let actual = sha256_hex(&bytes);
        if actual != expected {
            return Err(format!(
                "locked mod `{source}` hash mismatch: expected {expected}, got {actual}"
            ));
        }
    }

    Ok(())
}

pub(super) fn verify_locked_artifact_hash(
    root: &Path,
    instance: &str,
    side: &str,
    artifact: &Path,
) -> Result<(), String> {
    let key = format!("{side}_sha256");
    let Some(expected) = locked_value(root, instance, &key)? else {
        return Ok(());
    };
    let bytes = fs::read(artifact).map_err(|error| {
        format!(
            "failed to read {side} artifact {}: {error}",
            artifact.display()
        )
    })?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "{side} artifact hash mismatch: expected {expected}, got {actual}"
        ));
    }

    Ok(())
}

pub(super) struct LockedMod {
    pub(super) path: PathBuf,
    pub(super) sha256: String,
}

pub(super) fn restore_locked_mod_path_and_hash(
    root: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<(PathBuf, String)>, String> {
    restore_locked_mod(root, instance, source, cache_dir)
        .map(|locked| locked.map(|locked| (locked.path, locked.sha256)))
}

pub(super) fn restore_locked_mod_path(
    root: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    restore_locked_mod(root, instance, source, cache_dir)
        .map(|locked| locked.map(|locked| locked.path))
}

pub(super) fn restore_locked_mod(
    root: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<LockedMod>, String> {
    let Some(lock) = locked_instance_block(root, instance)? else {
        return Ok(None);
    };
    for block in lock.split("[[mod]]").skip(1) {
        if block_string_value(block, "source").as_deref() == Some(source)
            && let Some(path) = block_string_value(block, "path")
            && let Some(sha256) = block_string_value(block, "sha256")
        {
            let path = PathBuf::from(path);
            let url = block_string_value(block, "url");
            let path = if path.is_file() {
                path
            } else if let Some(url) = &url {
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("mod.jar");
                fetch_to_cache(url, cache_dir, file_name)?
            } else {
                path
            };
            return Ok(Some(LockedMod { path, sha256 }));
        }
    }

    Ok(None)
}

pub(super) fn lock_is_stale_for_instance(lock_path: &Path, instance: &str) -> Result<bool, String> {
    if !lock_path.is_file() {
        return Ok(true);
    }

    let lock = fs::read_to_string(lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    Ok(instance_block(&lock, instance).is_none())
}

pub(super) fn locked_minecraft_url(
    root: &Path,
    instance: &str,
    key: &str,
) -> Result<Option<String>, String> {
    locked_value(root, instance, key)
}

pub(super) fn locked_value(
    root: &Path,
    instance: &str,
    key: &str,
) -> Result<Option<String>, String> {
    let Some(lock) = locked_instance_block(root, instance)? else {
        return Ok(None);
    };
    Ok(block_string_value(&lock, key))
}

pub(super) fn locked_instance_block(root: &Path, instance: &str) -> Result<Option<String>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    Ok(instance_block(&lock, instance).map(str::to_string))
}

pub(super) fn instance_block<'a>(lock: &'a str, instance: &str) -> Option<&'a str> {
    lock.split("[[instance]]")
        .skip(1)
        .find(|block| block_string_value(block, "instance").as_deref() == Some(instance))
}

pub(super) fn locked_main_class(
    root: &Path,
    instance: &str,
    side: &str,
) -> Result<Option<String>, String> {
    let side_key = format!("{side}_main_class");
    if let Some(main_class) = locked_value(root, instance, &side_key)? {
        return Ok(Some(main_class));
    }

    locked_value(root, instance, "main_class")
}

pub(super) fn locked_java_major(root: &Path, instance: &str) -> Result<Option<u32>, String> {
    locked_value(root, instance, "java_major")?
        .map(|value| {
            value
                .parse()
                .map_err(|error| format!("invalid locked java_major `{value}`: {error}"))
        })
        .transpose()
}

pub(super) fn fetch_locked_libraries(
    root: &Path,
    instance: &str,
    cache_dir: &Path,
    side: &str,
) -> Result<Vec<PathBuf>, String> {
    let Some(lock) = locked_instance_block(root, instance)? else {
        return Ok(Vec::new());
    };
    let mut libraries = Vec::new();
    for block in lock.split("[[library]]").skip(1) {
        if let Some(library_side) = block_string_value(block, "side")
            && library_side != "common"
            && library_side != side
        {
            continue;
        }
        let file_name = block_string_value(block, "path")
            .and_then(|path| path.rsplit('/').next().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "library.jar".to_string());
        let name = block_string_value(block, "name").unwrap_or_else(|| file_name.clone());
        if let Some(url) = block_string_value(block, "url") {
            let path = fetch_to_cache(&url, cache_dir, &file_name)?;
            verify_locked_library_hash(block, &name, &path)?;
            libraries.push(path);
        } else if let Some(path) = block_string_value(block, "path") {
            let path = PathBuf::from(path);
            verify_locked_library_hash(block, &name, &path)?;
            libraries.push(path);
        }
    }

    Ok(libraries)
}

pub(super) fn verify_locked_library_hash(
    block: &str,
    name: &str,
    path: &Path,
) -> Result<(), String> {
    let Some(expected) = block_string_value(block, "sha256") else {
        return Ok(());
    };
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read locked library {}: {error}", path.display()))?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "locked library `{name}` hash mismatch: expected {expected}, got {actual}"
        ));
    }

    Ok(())
}

pub(super) fn fetch_locked_assets(
    root: &Path,
    instance: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let assets_dir = cache_dir.join("assets");
    let Some(lock) = locked_instance_block(root, instance)? else {
        return Ok(assets_dir);
    };
    if let Some(index_url) = block_string_value(&lock, "index_url") {
        let id = block_string_value(&lock, "id").unwrap_or_else(|| "assets".to_string());
        let index_path = fetch_to_cache(
            &index_url,
            &assets_dir.join("indexes"),
            &format!("{id}.json"),
        )?;
        if let Some(expected) = block_string_value(&lock, "index_sha256") {
            verify_file_hash("locked asset index", &id, &index_path, &expected)?;
        }
    }

    for block in lock.split("[[asset]]").skip(1) {
        let Some(hash) = block_string_value(block, "hash") else {
            continue;
        };
        let Some(url) = block_string_value(block, "url") else {
            continue;
        };
        let name = block_string_value(block, "name").unwrap_or_else(|| hash.clone());
        let object_path = fetch_to_cache(&url, &asset_object_dir(cache_dir, &hash), &hash)?;
        if let Some(expected) = block_string_value(block, "sha256") {
            verify_file_hash("locked asset", &name, &object_path, &expected)?;
        }
    }

    Ok(assets_dir)
}

pub(super) fn verify_file_hash(
    kind: &str,
    name: &str,
    path: &Path,
    expected: &str,
) -> Result<(), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read {kind} {}: {error}", path.display()))?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "{kind} `{name}` hash mismatch: expected {expected}, got {actual}"
        ));
    }

    Ok(())
}

pub(super) fn join_classpath(paths: &[PathBuf]) -> String {
    let separator = if cfg!(windows) { ";" } else { ":" };
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(separator)
}

pub(super) fn block_string_value(block: &str, key: &str) -> Option<String> {
    block
        .lines()
        .map(str::trim)
        .find_map(|line| string_value(line, key))
}
