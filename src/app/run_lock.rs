use super::*;
use std::io::Read;
use toml_edit::{DocumentMut, Item, Table};
use zip::ZipArchive;

pub(super) struct LockedInstance {
    header: String,
    document: DocumentMut,
}

impl LockedInstance {
    pub(super) fn read(lock_path: &Path, instance: &str) -> Result<Option<Self>, String> {
        if !lock_path.is_file() {
            return Ok(None);
        }

        let lock = fs::read_to_string(&lock_path)
            .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
        let Some(block) = instance_block(&lock, instance) else {
            return Ok(None);
        };
        let (header, body) = split_instance_body(block);
        let document = body
            .parse::<DocumentMut>()
            .map_err(|error| format!("failed to parse {}: {error}", lock_path.display()))?;

        Ok(Some(Self {
            header: header.to_string(),
            document,
        }))
    }

    pub(super) fn value(&self, key: &str) -> Option<String> {
        block_string_value(&self.header, key)
            .or_else(|| table_string(self.document.get(key)))
            .or_else(|| {
                ["minecraft", "loader", "launch", "assets"]
                    .into_iter()
                    .find_map(|table| {
                        table_string(
                            self.document
                                .get(table)
                                .and_then(Item::as_table)
                                .and_then(|table| table.get(key)),
                        )
                    })
            })
    }

    pub(super) fn table_value(&self, table: &str, key: &str) -> Option<String> {
        table_string(
            self.document
                .get(table)
                .and_then(Item::as_table)
                .and_then(|table| table.get(key)),
        )
    }

    pub(super) fn main_class(&self, side: &str) -> Option<String> {
        let side_key = format!("{side}_main_class");
        self.value(&side_key).or_else(|| self.value("main_class"))
    }

    pub(super) fn java_major(&self) -> Result<Option<u32>, String> {
        self.value("java_major")
            .map(|value| {
                value
                    .parse()
                    .map_err(|error| format!("invalid locked java_major `{value}`: {error}"))
            })
            .transpose()
    }

    pub(super) fn arguments(&self, kind: &str) -> Vec<String> {
        let mut args = Vec::new();

        let Some(arguments) = self
            .document
            .get("argument")
            .and_then(Item::as_array_of_tables)
        else {
            return args;
        };

        args.extend(arguments.iter().filter_map(|argument| {
            (table_string(argument.get("kind")).as_deref() == Some(kind))
                .then(|| table_string(argument.get("arg")))
                .flatten()
        }));

        args
    }

    fn array_tables(&self, name: &str) -> Option<&toml_edit::ArrayOfTables> {
        self.document.get(name).and_then(Item::as_array_of_tables)
    }
}

pub(super) fn verify_locked_mod_hashes(
    lock_path: &Path,
    instance: &Instance,
    cache_dir: &Path,
) -> Result<(), String> {
    for source in locked_mod_sources(lock_path, &instance.name)? {
        let Some((path, expected)) =
            restore_locked_mod_path_and_hash(lock_path, &instance.name, &source, cache_dir)?
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

pub(super) fn locked_mod_sources(lock_path: &Path, instance: &str) -> Result<Vec<String>, String> {
    let Some(lock) = LockedInstance::read(lock_path, instance)? else {
        return Ok(Vec::new());
    };
    Ok(lock
        .array_tables("mod")
        .into_iter()
        .flatten()
        .filter_map(|block| table_string(block.get("source")))
        .collect())
}

pub(super) struct LockedPackFile {
    pub(super) destination: PathBuf,
    pub(super) path: PathBuf,
}

pub(super) fn restore_locked_pack_files(
    lock_path: &Path,
    instance: &str,
    side: &str,
    cache_dir: &Path,
) -> Result<Vec<LockedPackFile>, String> {
    let Some(lock) = LockedInstance::read(lock_path, instance)? else {
        return Ok(Vec::new());
    };
    let mut files = Vec::new();
    for block in lock.array_tables("pack_file").into_iter().flatten() {
        let sides = block
            .get("sides")
            .and_then(Item::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .any(|value| value == side)
            })
            .unwrap_or(false);
        if !sides {
            continue;
        }
        let destination = table_string(block.get("destination"))
            .ok_or_else(|| "locked pack file is missing destination".to_string())?;
        let destination = safe_pack_destination(&destination)?;
        let expected = table_string(block.get("sha256"))
            .ok_or_else(|| "locked pack file is missing sha256".to_string())?;
        let path = restore_pack_file(&lock, block, &destination, &expected, cache_dir)?;
        files.push(LockedPackFile { destination, path });
    }
    Ok(files)
}

fn restore_pack_file(
    lock: &LockedInstance,
    block: &Table,
    destination: &Path,
    expected: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let locked_path = table_string(block.get("path"))
        .map(PathBuf::from)
        .ok_or_else(|| "locked pack file is missing path".to_string())?;
    let path = if locked_path.is_file() {
        locked_path
    } else if let Some(url) = table_string(block.get("url")) {
        let file_name = destination
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                format!(
                    "pack destination has no filename: {}",
                    destination.display()
                )
            })?;
        fetch_to_cache(&url, &cache_dir.join(expected), file_name)?
    } else if let Some(entry) = table_string(block.get("archive_entry")) {
        restore_pack_override(lock, &entry, destination, expected, cache_dir)?
    } else {
        return Err(format!(
            "locked pack file {} is missing and has no restoration source",
            destination.display()
        ));
    };
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "failed to read locked pack file {}: {error}",
            path.display()
        )
    })?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "locked pack file `{}` hash mismatch: expected {expected}, got {actual}",
            destination.display()
        ));
    }
    Ok(path)
}

fn restore_pack_override(
    lock: &LockedInstance,
    entry: &str,
    destination: &Path,
    expected: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let archive_url = lock
        .table_value("pack", "archive_url")
        .ok_or_else(|| "locked Modrinth pack is missing archive_url".to_string())?;
    let locked_archive = lock
        .table_value("pack", "archive_path")
        .map(PathBuf::from)
        .ok_or_else(|| "locked Modrinth pack is missing archive_path".to_string())?;
    let archive_name = locked_archive
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pack.mrpack");
    let archive = if locked_archive.is_file() {
        locked_archive
    } else {
        fetch_to_cache(&archive_url, &cache_dir.join("archive"), archive_name)?
    };
    if let Some(expected_archive) = lock.table_value("pack", "archive_sha256") {
        let bytes = fs::read(&archive).map_err(|error| {
            format!("failed to read pack archive {}: {error}", archive.display())
        })?;
        let actual = sha256_hex(&bytes);
        if actual != expected_archive {
            return Err(format!(
                "locked Modrinth pack hash mismatch: expected {expected_archive}, got {actual}"
            ));
        }
    }
    let file = fs::File::open(&archive)
        .map_err(|error| format!("failed to open pack archive {}: {error}", archive.display()))?;
    let mut zip = ZipArchive::new(file)
        .map_err(|error| format!("failed to open pack archive {}: {error}", archive.display()))?;
    let mut source = zip
        .by_name(entry)
        .map_err(|error| format!("pack archive is missing `{entry}`: {error}"))?;
    let mut bytes = Vec::new();
    source
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to extract pack archive entry `{entry}`: {error}"))?;
    let actual = sha256_hex(&bytes);
    if actual != expected {
        return Err(format!(
            "locked pack override `{entry}` hash mismatch: expected {expected}, got {actual}"
        ));
    }
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            format!(
                "pack destination has no filename: {}",
                destination.display()
            )
        })?;
    let output_dir = cache_dir.join(expected);
    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("failed to create {}: {error}", output_dir.display()))?;
    let output = output_dir.join(file_name);
    fs::write(&output, bytes)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    Ok(output)
}

pub(super) fn verify_locked_artifact_hash(
    lock_path: &Path,
    instance: &str,
    side: &str,
    artifact: &Path,
) -> Result<(), String> {
    let key = format!("{side}_sha256");
    let Some(expected) = locked_value(lock_path, instance, &key)? else {
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
    lock_path: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<(PathBuf, String)>, String> {
    restore_locked_mod(lock_path, instance, source, cache_dir)
        .map(|locked| locked.map(|locked| (locked.path, locked.sha256)))
}

pub(super) fn restore_locked_mod_path(
    lock_path: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<PathBuf>, String> {
    restore_locked_mod(lock_path, instance, source, cache_dir)
        .map(|locked| locked.map(|locked| locked.path))
}

pub(super) fn restore_locked_mod(
    lock_path: &Path,
    instance: &str,
    source: &str,
    cache_dir: &Path,
) -> Result<Option<LockedMod>, String> {
    let Some(lock) = LockedInstance::read(lock_path, instance)? else {
        return Ok(None);
    };
    for block in lock.array_tables("mod").into_iter().flatten() {
        if table_string(block.get("source")).as_deref() == Some(source)
            && let Some(path) = table_string(block.get("path"))
            && let Some(sha256) = table_string(block.get("sha256"))
        {
            let path = PathBuf::from(path);
            let url = table_string(block.get("url"));
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

pub(super) fn instance_resolve_digest(config: &Config, instance: &Instance) -> String {
    let mut payload = format!(
        "minecraft={}\nloader={}\nloader_version={}\n",
        instance.minecraft,
        instance.loader,
        requested_loader_version(instance)
    );
    payload.push_str("sides=");
    payload.push_str(&instance.sides.join("\t"));
    payload.push('\n');
    payload.push_str("pack=");
    if let Some(pack) = &instance.modrinth_pack {
        payload.push_str(pack);
    }
    payload.push('\n');
    payload.push_str("mods=");
    payload.push_str(&instance.mods.join("\t"));
    payload.push('\n');
    for (key, value) in &instance.server_properties {
        payload.push_str("prop=");
        payload.push_str(key);
        payload.push('\t');
        payload.push_str(value);
        payload.push('\n');
    }
    for (name, url) in &config.repositories {
        payload.push_str("repo=");
        payload.push_str(name);
        payload.push('\t');
        payload.push_str(url);
        payload.push('\n');
    }
    sha256_hex(payload.as_bytes())
}

pub(super) fn lock_is_stale_for_instance(
    lock_path: &Path,
    config: &Config,
    instance: &Instance,
) -> Result<bool, String> {
    if !lock_path.is_file() {
        return Ok(true);
    }

    let lock = fs::read_to_string(lock_path)
        .map_err(|error| format!("failed to read {}: {error}", lock_path.display()))?;
    let Some(block) = instance_block(&lock, &instance.name) else {
        return Ok(true);
    };
    // Older locks omit config_digest and only go stale when the instance name is missing.
    let Some(digest) = block_string_value(block, "config_digest") else {
        return Ok(false);
    };
    Ok(digest != instance_resolve_digest(config, instance))
}

pub(super) fn locked_minecraft_url(
    lock_path: &Path,
    instance: &str,
    key: &str,
) -> Result<Option<String>, String> {
    locked_value(lock_path, instance, key)
}

pub(super) fn locked_value(
    lock_path: &Path,
    instance: &str,
    key: &str,
) -> Result<Option<String>, String> {
    Ok(LockedInstance::read(lock_path, instance)?.and_then(|lock| lock.value(key)))
}

pub(super) fn locked_table_value(
    lock_path: &Path,
    instance: &str,
    table: &str,
    key: &str,
) -> Result<Option<String>, String> {
    Ok(LockedInstance::read(lock_path, instance)?.and_then(|lock| lock.table_value(table, key)))
}

fn instance_block<'a>(lock: &'a str, instance: &str) -> Option<&'a str> {
    lock.split("[[instance]]")
        .skip(1)
        .find(|block| block_string_value(block, "instance").as_deref() == Some(instance))
}

fn split_instance_body(block: &str) -> (&str, &str) {
    let body_start = block
        .lines()
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len() + 1;
            Some((start, line))
        })
        .find_map(|(start, line)| line.trim_start().starts_with('[').then_some(start))
        .unwrap_or(block.len());

    block.split_at(body_start)
}

pub(super) fn locked_main_class(
    lock_path: &Path,
    instance: &str,
    side: &str,
) -> Result<Option<String>, String> {
    Ok(LockedInstance::read(lock_path, instance)?.and_then(|lock| lock.main_class(side)))
}

pub(super) fn locked_java_major(lock_path: &Path, instance: &str) -> Result<Option<u32>, String> {
    LockedInstance::read(lock_path, instance)?
        .map(|lock| lock.java_major())
        .transpose()
        .map(Option::flatten)
}

pub(super) fn locked_arguments(
    lock_path: &Path,
    instance: &str,
    kind: &str,
) -> Result<Vec<String>, String> {
    Ok(LockedInstance::read(lock_path, instance)?
        .map(|lock| lock.arguments(kind))
        .unwrap_or_default())
}

pub(super) fn fetch_locked_libraries(
    lock_path: &Path,
    instance: &str,
    cache_dir: &Path,
    side: &str,
) -> Result<Vec<PathBuf>, String> {
    let Some(lock) = LockedInstance::read(lock_path, instance)? else {
        return Ok(Vec::new());
    };
    let mut libraries = Vec::new();
    for block in lock.array_tables("library").into_iter().flatten() {
        if let Some(library_side) = table_string(block.get("side"))
            && library_side != "common"
            && library_side != side
        {
            continue;
        }
        let file_name = table_string(block.get("path"))
            .and_then(|path| path.rsplit('/').next().map(str::to_string))
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "library.jar".to_string());
        let name = table_string(block.get("name")).unwrap_or_else(|| file_name.clone());
        if let Some(url) = table_string(block.get("url")) {
            let path = fetch_to_cache(&url, cache_dir, &file_name)?;
            verify_locked_library_hash(block, &name, &path)?;
            libraries.push(path);
        } else if let Some(path) = table_string(block.get("path")) {
            let path = PathBuf::from(path);
            verify_locked_library_hash(block, &name, &path)?;
            libraries.push(path);
        }
    }

    Ok(libraries)
}

pub(super) fn verify_locked_library_hash(
    block: &Table,
    name: &str,
    path: &Path,
) -> Result<(), String> {
    let Some(expected) = table_string(block.get("sha256")) else {
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

fn table_string(item: Option<&Item>) -> Option<String> {
    let item = item?;
    if let Some(value) = item.as_str() {
        Some(value.to_string())
    } else if let Some(value) = item.as_integer() {
        Some(value.to_string())
    } else {
        item.as_bool().map(|value| value.to_string())
    }
}

pub(super) fn fetch_locked_assets(
    lock_path: &Path,
    instance: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let assets_dir = cache_dir.join("assets");
    let Some(lock) = LockedInstance::read(lock_path, instance)? else {
        return Ok(assets_dir);
    };
    if let Some(index_url) = lock.value("index_url") {
        let id = lock.value("id").unwrap_or_else(|| "assets".to_string());
        let index_path = fetch_to_cache(
            &index_url,
            &assets_dir.join("indexes"),
            &format!("{id}.json"),
        )?;
        if let Some(expected) = lock.value("index_sha256") {
            verify_file_hash("locked asset index", &id, &index_path, &expected)?;
        }
        hydrate_asset_objects(&index_path, &assets_dir)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(loader_version: Option<&str>, mods: &[&str]) -> (Config, Instance) {
        let instance = Instance {
            name: "vanilla-26.1.2".to_string(),
            minecraft: "26.1.2".to_string(),
            loader: "vanilla".to_string(),
            loader_version: loader_version.map(str::to_string),
            sides: vec!["server".to_string()],
            modrinth_pack: None,
            server_properties: Vec::new(),
            mods: mods.iter().map(|source| (*source).to_string()).collect(),
            fixtures: Vec::new(),
        };
        let config = Config {
            project_name: "digest-test".to_string(),
            repositories: Vec::new(),
            instances: Vec::new(),
        };
        (config, instance)
    }

    #[test]
    fn resolve_digest_changes_when_loader_or_mods_change() {
        let (config, latest) = test_config(None, &[]);
        let explicit_latest = test_config(Some("latest"), &[]).1;
        let pinned = test_config(Some("0.18.4"), &[]).1;
        let later_loader = test_config(Some("0.19.3"), &[]).1;
        let with_mod = test_config(None, &["./extra.jar"]).1;

        assert_eq!(
            instance_resolve_digest(&config, &latest),
            instance_resolve_digest(&config, &explicit_latest)
        );
        assert_ne!(
            instance_resolve_digest(&config, &pinned),
            instance_resolve_digest(&config, &later_loader)
        );
        assert_ne!(
            instance_resolve_digest(&config, &latest),
            instance_resolve_digest(&config, &with_mod)
        );
    }
}
