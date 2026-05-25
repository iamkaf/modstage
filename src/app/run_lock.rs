use super::*;

pub(super) fn verify_locked_mod_hashes(root: &Path, instance: &Instance) -> Result<(), String> {
    for source in &instance.mods {
        let Some((path, expected)) = locked_mod_path_and_hash(root, source)? else {
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
    side: &str,
    artifact: &Path,
) -> Result<(), String> {
    let key = format!("{side}_sha256");
    let Some(expected) = locked_value(root, &key)? else {
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

pub(super) fn locked_mod_path_and_hash(
    root: &Path,
    source: &str,
) -> Result<Option<(PathBuf, String)>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    for block in lock.split("[[mod]]").skip(1) {
        if block_string_value(block, "source").as_deref() == Some(source)
            && let Some(path) = block_string_value(block, "path")
            && let Some(sha256) = block_string_value(block, "sha256")
        {
            return Ok(Some((PathBuf::from(path), sha256)));
        }
    }

    Ok(None)
}

pub(super) fn locked_mod_path(root: &Path, source: &str) -> Result<Option<PathBuf>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    for block in lock.split("[[mod]]").skip(1) {
        if block_string_value(block, "source").as_deref() == Some(source)
            && let Some(path) = block_string_value(block, "path")
        {
            return Ok(Some(PathBuf::from(path)));
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
    Ok(!lock.contains(&format!("instance = \"{instance}\"")))
}

pub(super) fn locked_minecraft_url(root: &Path, key: &str) -> Result<Option<String>, String> {
    locked_value(root, key)
}

pub(super) fn locked_value(root: &Path, key: &str) -> Result<Option<String>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(None);
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    Ok(block_string_value(&lock, key))
}

pub(super) fn locked_main_class(root: &Path, side: &str) -> Result<Option<String>, String> {
    let side_key = format!("{side}_main_class");
    if let Some(main_class) = locked_value(root, &side_key)? {
        return Ok(Some(main_class));
    }

    locked_value(root, "main_class")
}

pub(super) fn fetch_locked_libraries(
    root: &Path,
    cache_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let lock_path = root.join("modstage.lock");
    if !lock_path.is_file() {
        return Ok(Vec::new());
    }

    let lock = fs::read_to_string(&lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    let mut libraries = Vec::new();
    for block in lock.split("[[library]]").skip(1) {
        let file_name = block_string_value(block, "path")
            .and_then(|path| path.rsplit('/').next().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "library.jar".to_string());
        if let Some(url) = block_string_value(block, "url") {
            libraries.push(fetch_to_cache(&url, cache_dir, &file_name)?);
        } else if let Some(path) = block_string_value(block, "path") {
            libraries.push(PathBuf::from(path));
        }
    }

    Ok(libraries)
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
