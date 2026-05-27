use super::*;
use toml_edit::{DocumentMut, Item, Table};

pub(super) struct LockedInstance {
    header: String,
    document: DocumentMut,
}

impl LockedInstance {
    pub(super) fn read(root: &Path, instance: &str) -> Result<Option<Self>, String> {
        let lock_path = root.join("modstage.lock");
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
    let Some(lock) = LockedInstance::read(root, instance)? else {
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
    Ok(LockedInstance::read(root, instance)?.and_then(|lock| lock.value(key)))
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
    root: &Path,
    instance: &str,
    side: &str,
) -> Result<Option<String>, String> {
    Ok(LockedInstance::read(root, instance)?.and_then(|lock| lock.main_class(side)))
}

pub(super) fn locked_java_major(root: &Path, instance: &str) -> Result<Option<u32>, String> {
    LockedInstance::read(root, instance)?
        .map(|lock| lock.java_major())
        .transpose()
        .map(Option::flatten)
}

pub(super) fn locked_arguments(
    root: &Path,
    instance: &str,
    kind: &str,
) -> Result<Vec<String>, String> {
    Ok(LockedInstance::read(root, instance)?
        .map(|lock| lock.arguments(kind))
        .unwrap_or_default())
}

pub(super) fn fetch_locked_libraries(
    root: &Path,
    instance: &str,
    cache_dir: &Path,
    side: &str,
) -> Result<Vec<PathBuf>, String> {
    let Some(lock) = LockedInstance::read(root, instance)? else {
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
    root: &Path,
    instance: &str,
    cache_dir: &Path,
) -> Result<PathBuf, String> {
    let assets_dir = cache_dir.join("assets");
    let Some(lock) = LockedInstance::read(root, instance)? else {
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
