use super::*;

pub(in crate::app) struct InstallerProfileLibrary {
    pub(in crate::app) name: String,
    pub(in crate::app) path: String,
    pub(in crate::app) url: String,
    pub(in crate::app) sha256: String,
}

pub(in crate::app) struct InstallerProfile {
    pub(in crate::app) main_class: Option<String>,
    pub(in crate::app) jvm_args: Vec<String>,
    pub(in crate::app) game_args: Vec<String>,
    pub(in crate::app) libraries: Vec<InstallerProfileLibrary>,
}

pub(in crate::app) fn resolve_installer_profile(
    installer_path: &Path,
    cache_dir: &Path,
) -> Result<InstallerProfile, String> {
    let Some(version_json) = jar_entry_text(installer_path, &["version.json", "/version.json"])? else {
        return Ok(InstallerProfile {
            main_class: None,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
            libraries: Vec::new(),
        });
    };
    let main_class = json_string(&version_json, "mainClass");
    let jvm_args = profile_arguments(&version_json, "jvm");
    let game_args = profile_arguments(&version_json, "game");
    let mut libraries = Vec::new();

    for block in minecraft_library_blocks(&version_json) {
        let Some(name) = json_string(block, "name") else {
            continue;
        };
        let Some(artifact) = json_object_after(block, "artifact") else {
            continue;
        };
        let Some(path) = json_string(artifact, "path") else {
            continue;
        };
        let Some(url) = json_string(artifact, "url") else {
            continue;
        };
        if url.is_empty() {
            continue;
        }
        let file_name = path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("library.jar");
        let library_path = fetch_to_cache(&url, cache_dir, file_name)?;
        let bytes = fs::read(&library_path)
            .map_err(|error| format!("failed to read {}: {error}", library_path.display()))?;

        libraries.push(InstallerProfileLibrary {
            name,
            path,
            url,
            sha256: sha256_hex(&bytes),
        });
    }

    Ok(InstallerProfile {
        main_class,
        jvm_args,
        game_args,
        libraries,
    })
}

pub(in crate::app) fn profile_arguments(version_json: &str, kind: &str) -> Vec<String> {
    let Some(arguments) = json_object_after(version_json, "arguments") else {
        return Vec::new();
    };
    json_string_array(arguments, kind).unwrap_or_default()
}

pub(in crate::app) fn jar_entry_text(
    jar_path: &Path,
    entries: &[&str],
) -> Result<Option<String>, String> {
    jar_entry_bytes(jar_path, entries).and_then(|entry| {
        entry
            .map(String::from_utf8)
            .transpose()
            .map_err(|error| format!("installer jar entry is not UTF-8: {error}"))
    })
}

pub(in crate::app) fn jar_entry_bytes(
    jar_path: &Path,
    entries: &[&str],
) -> Result<Option<Vec<u8>>, String> {
    let bytes = fs::read(jar_path)
        .map_err(|error| format!("failed to read jar {}: {error}", jar_path.display()))?;

    match stored_zip_entry(&bytes, entries) {
        Ok(Some(contents)) => Ok(Some(contents)),
        Ok(None) => Ok(None),
        Err(ZipReadError::UnsupportedCompression) => jar_entry_bytes_with_tool(jar_path, entries),
        Err(ZipReadError::Malformed(message)) => Err(format!(
            "failed to read jar {}: {message}",
            jar_path.display()
        )),
    }
}

enum ZipReadError {
    Malformed(String),
    UnsupportedCompression,
}

fn stored_zip_entry(bytes: &[u8], entries: &[&str]) -> Result<Option<Vec<u8>>, ZipReadError> {
    let Some(eocd) = end_of_central_directory(bytes) else {
        return Err(ZipReadError::Malformed(
            "end of central directory not found".to_string(),
        ));
    };
    let entry_count = read_u16(bytes, eocd + 10)? as usize;
    let central_offset = read_u32(bytes, eocd + 16)? as usize;
    let mut cursor = central_offset;

    for _ in 0..entry_count {
        if read_u32(bytes, cursor)? != 0x0201_4b50 {
            return Err(ZipReadError::Malformed(
                "central directory header not found".to_string(),
            ));
        }
        let method = read_u16(bytes, cursor + 10)?;
        let compressed_size = read_u32(bytes, cursor + 20)? as usize;
        let name_len = read_u16(bytes, cursor + 28)? as usize;
        let extra_len = read_u16(bytes, cursor + 30)? as usize;
        let comment_len = read_u16(bytes, cursor + 32)? as usize;
        let local_offset = read_u32(bytes, cursor + 42)? as usize;
        let name_start = cursor + 46;
        let name_end = checked_add(name_start, name_len)?;
        let name = bytes
            .get(name_start..name_end)
            .and_then(|name| std::str::from_utf8(name).ok())
            .ok_or_else(|| ZipReadError::Malformed("entry name is not UTF-8".to_string()))?;

        if entries
            .iter()
            .any(|entry| entry.trim_start_matches('/') == name)
        {
            if method != 0 {
                return Err(ZipReadError::UnsupportedCompression);
            }
            if read_u32(bytes, local_offset)? != 0x0403_4b50 {
                return Err(ZipReadError::Malformed(
                    "local file header not found".to_string(),
                ));
            }
            let local_name_len = read_u16(bytes, local_offset + 26)? as usize;
            let local_extra_len = read_u16(bytes, local_offset + 28)? as usize;
            let data_start = checked_add(local_offset + 30, local_name_len + local_extra_len)?;
            let data_end = checked_add(data_start, compressed_size)?;
            let data = bytes
                .get(data_start..data_end)
                .ok_or_else(|| ZipReadError::Malformed("entry data is truncated".to_string()))?;
            return Ok(Some(data.to_vec()));
        }

        cursor = checked_add(name_end, extra_len + comment_len)?;
    }

    Ok(None)
}

fn end_of_central_directory(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 4 {
        return None;
    }
    let start = bytes.len().saturating_sub(66_000);

    (start..=bytes.len() - 4)
        .rev()
        .find(|offset| bytes[*offset..].starts_with(&0x0605_4b50_u32.to_le_bytes()))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ZipReadError> {
    let bytes = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| ZipReadError::Malformed("zip header is truncated".to_string()))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ZipReadError> {
    let bytes = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| ZipReadError::Malformed("zip header is truncated".to_string()))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn checked_add(left: usize, right: usize) -> Result<usize, ZipReadError> {
    left.checked_add(right)
        .ok_or_else(|| ZipReadError::Malformed("zip offset overflow".to_string()))
}

fn jar_entry_bytes_with_tool(jar_path: &Path, entries: &[&str]) -> Result<Option<Vec<u8>>, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX_EPOCH: {error}"))?
        .as_nanos();
    let temp = env::temp_dir().join(format!("modstage-jar-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&temp).map_err(|error| {
        format!(
            "failed to create temporary jar extraction dir {}: {error}",
            temp.display()
        )
    })?;

    for entry in entries {
        let entry = entry.trim_start_matches('/');
        let status = Command::new("jar")
            .arg("xf")
            .arg(jar_path)
            .arg(entry)
            .current_dir(&temp)
            .status()
            .map_err(|error| format!("failed to run jar tool for {}: {error}", jar_path.display()))?;
        if !status.success() {
            continue;
        }

        let extracted = temp.join(entry);
        if extracted.is_file() {
            let bytes = fs::read(&extracted)
                .map_err(|error| format!("failed to read {}: {error}", extracted.display()))?;
            let _ = fs::remove_dir_all(&temp);
            return Ok(Some(bytes));
        }
    }

    let _ = fs::remove_dir_all(&temp);
    Ok(None)
}
