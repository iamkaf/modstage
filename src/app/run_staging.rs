use super::*;

pub(super) fn reconcile_mods(
    lock_path: &Path,
    instance: &Instance,
    root: &Path,
    mods_dir: &Path,
    cache_dir: &Path,
) -> Result<(), String> {
    let mut sources = locked_mod_sources(lock_path, &instance.name)?;
    if sources.is_empty() {
        sources.clone_from(&instance.mods);
    }
    for source in sources {
        let path = restore_locked_mod_path(lock_path, &instance.name, &source, cache_dir)?
            .or_else(|| local_mod_path(root, &source));
        let Some(path) = path else { continue };
        let file_name = path
            .file_name()
            .ok_or_else(|| format!("resolved mod has no filename: {}", path.display()))?;
        fs::copy(&path, mods_dir.join(file_name))
            .map_err(|error| format!("failed to stage mod {}: {error}", path.display()))?;
    }

    Ok(())
}

pub(super) fn clear_mods(mods_dir: &Path) -> Result<(), String> {
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

    Ok(())
}

pub(super) fn reconcile_pack_files(
    lock_path: &Path,
    instance: &Instance,
    side: &str,
    game_dir: &Path,
    cache_dir: &Path,
) -> Result<(), String> {
    let manifest = game_dir.join(".modstage-pack-files");
    if manifest.is_file() {
        let previous = fs::read_to_string(&manifest)
            .map_err(|error| format!("failed to read {}: {error}", manifest.display()))?;
        for destination in previous.lines().filter(|line| !line.is_empty()) {
            let destination = safe_pack_destination(destination)?;
            let path = game_dir.join(destination);
            if path.is_file() {
                fs::remove_file(&path)
                    .map_err(|error| format!("failed to remove {}: {error}", path.display()))?;
            }
        }
    }

    let files = restore_locked_pack_files(lock_path, &instance.name, side, cache_dir)?;
    let mut destinations = Vec::new();
    for file in files {
        let output = game_dir.join(&file.destination);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
        }
        fs::copy(&file.path, &output)
            .map_err(|error| format!("failed to stage pack file {}: {error}", output.display()))?;
        destinations.push(file.destination.to_string_lossy().replace('\\', "/"));
    }
    destinations.sort();
    fs::write(
        &manifest,
        destinations.join("\n") + if destinations.is_empty() { "" } else { "\n" },
    )
    .map_err(|error| format!("failed to write {}: {error}", manifest.display()))?;
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

pub(super) fn apply_server_properties(instance: &Instance, game_dir: &Path) -> Result<(), String> {
    if instance.server_properties.is_empty() {
        return Ok(());
    }
    let path = game_dir.join("server.properties");
    let existing = if path.is_file() {
        fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?
    } else {
        String::new()
    };
    let mut remaining = instance
        .server_properties
        .iter()
        .cloned()
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut output = Vec::new();
    for line in existing.lines() {
        let trimmed = line.trim_start();
        let key = (!trimmed.starts_with('#') && !trimmed.starts_with('!'))
            .then(|| trimmed.split(['=', ':']).next().unwrap_or("").trim())
            .filter(|key| !key.is_empty());
        if let Some((key, value)) = key.and_then(|key| remaining.remove_entry(key)) {
            output.push(format!("{key}={value}"));
        } else {
            output.push(line.to_string());
        }
    }
    output.extend(
        remaining
            .into_iter()
            .map(|(key, value)| format!("{key}={value}")),
    );
    fs::write(&path, output.join("\n") + "\n")
        .map_err(|error| format!("failed to write {}: {error}", path.display()))
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
