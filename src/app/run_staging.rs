use super::*;

pub(super) fn reconcile_mods(
    root: &Path,
    lock_path: &Path,
    instance: &Instance,
    mods_dir: &Path,
    cache_dir: &Path,
) -> Result<(), String> {
    for entry in fs::read_dir(mods_dir)
        .map_err(|error| format!("failed to read {}: {error}", mods_dir.display()))?
    {
        let path = entry
            .map_err(|error| format!("failed to read mod directory entry: {error}"))?
            .path();

        if path.is_file() {
            fs::remove_file(&path).map_err(|error| {
                format!("failed to remove stale mod {}: {error}", path.display())
            })?;
        }
    }

    for source in &instance.mods {
        let Some(path) = resolved_mod_path(root, lock_path, instance, source, cache_dir)? else {
            continue;
        };
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("resolved mod has no filename: {}", path.display()))?;
        fs::copy(&path, mods_dir.join(file_name))
            .map_err(|error| format!("failed to stage mod {}: {error}", path.display()))?;
    }

    Ok(())
}

pub(super) fn apply_fixtures(
    root: &Path,
    side: &str,
    instance: &Instance,
    game_dir: &Path,
) -> Result<(), String> {
    for fixture in &instance.fixtures {
        if let Some(fixture_side) = &fixture.side
            && fixture_side != side
        {
            continue;
        }

        let source = root.join(&fixture.from);
        let destination = game_dir.join(&fixture.to);
        copy_fixture_tree(&source, &destination, fixture.replace)?;
    }

    Ok(())
}

pub(super) fn write_side_launcher_metadata(
    instance: &Instance,
    side: &str,
    game_dir: &Path,
) -> Result<(), String> {
    let mut metadata = format!(
        "instance = \"{}\"\nside = \"{}\"\nminecraft = \"{}\"\nloader = \"{}\"\n",
        toml_escape(&instance.name),
        toml_escape(side),
        toml_escape(&instance.minecraft),
        toml_escape(&instance.loader)
    );

    if let Some(loader_version) = &instance.loader_version {
        metadata.push_str(&format!(
            "loader_version = \"{}\"\n",
            toml_escape(loader_version)
        ));
    }

    metadata.push_str("mods = [");
    metadata.push_str(
        &instance
            .mods
            .iter()
            .map(|source| format!("\"{}\"", toml_escape(source)))
            .collect::<Vec<_>>()
            .join(", "),
    );
    metadata.push_str("]\n");

    let path = game_dir.join("modstage-launch.toml");
    fs::write(&path, metadata).map_err(|error| {
        format!(
            "failed to write launcher metadata {}: {error}",
            path.display()
        )
    })
}

pub(super) fn copy_fixture_tree(
    source: &Path,
    destination: &Path,
    replace: bool,
) -> Result<(), String> {
    if source.is_file() {
        if destination.exists() && !replace {
            return Ok(());
        }

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::copy(source, destination)
            .map_err(|error| format!("failed to copy fixture {}: {error}", source.display()))?;
        return Ok(());
    }

    if !source.is_dir() {
        return Err(format!(
            "fixture source {} does not exist",
            source.display()
        ));
    }

    fs::create_dir_all(destination)
        .map_err(|error| format!("failed to create {}: {error}", destination.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("failed to read fixture {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| format!("failed to read fixture entry: {error}"))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if source_path.is_dir() || source_path.is_file() {
            copy_fixture_tree(&source_path, &destination_path, replace)?;
        }
    }

    Ok(())
}

pub(super) fn resolved_mod_path(
    root: &Path,
    lock_path: &Path,
    instance: &Instance,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    if let Some(path) = local_mod_path(root, source) {
        return path
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("failed to resolve local mod {}: {error}", path.display()));
    }

    if let Some(path) = restore_locked_mod_path(lock_path, &instance.name, source, cache_dir)? {
        return path
            .canonicalize()
            .map(Some)
            .map_err(|error| format!("failed to resolve locked mod {}: {error}", path.display()));
    }

    if let Some(coordinates) = MavenCoordinates::parse(source) {
        return Ok(maven_artifact(&[], &coordinates)
            .map(|(_, path)| path)
            .and_then(|path| path.canonicalize().ok()));
    }

    Ok(None)
}
