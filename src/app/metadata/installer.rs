use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct InstallerVersionJson {
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    arguments: Option<InstallerArguments>,
    libraries: Option<Vec<InstallerLibraryEntry>>,
}

#[derive(Deserialize)]
struct InstallerArguments {
    jvm: Option<Vec<String>>,
    game: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct InstallerLibraryEntry {
    name: String,
    downloads: Option<InstallerLibraryDownloads>,
    #[serde(default)]
    rules: Vec<LibraryRule>,
}

#[derive(Deserialize)]
struct InstallerLibraryDownloads {
    artifact: Option<InstallerLibraryArtifact>,
}

#[derive(Deserialize)]
struct InstallerLibraryArtifact {
    path: String,
    url: String,
    sha1: Option<String>,
}

pub(in crate::app) struct InstallerProfileLibrary {
    pub(in crate::app) name: String,
    pub(in crate::app) path: String,
    /// Where to download the library and its hash, or `None` for a library the installer's
    /// processors generate, such as the patched client jar.
    pub(in crate::app) download: Option<(String, String)>,
}

pub(in crate::app) struct InstallerProfile {
    pub(in crate::app) main_class: Option<String>,
    pub(in crate::app) jvm_args: Vec<String>,
    pub(in crate::app) game_args: Vec<String>,
    pub(in crate::app) libraries: Vec<InstallerProfileLibrary>,
    /// Libraries from `install_profile.json`, which the installer places in the libraries
    /// directory without adding them to the classpath.
    pub(in crate::app) directory_libraries: Vec<InstallerProfileLibrary>,
}

#[derive(Deserialize)]
struct InstallProfileJson {
    libraries: Option<Vec<InstallerLibraryEntry>>,
}

pub(in crate::app) fn resolve_installer_profile(
    installer_path: &Path,
    cache_dir: &Path,
) -> Result<InstallerProfile, String> {
    let Some(version_json) = jar_entry_text(installer_path, &["version.json", "/version.json"])?
    else {
        return Ok(InstallerProfile {
            main_class: None,
            jvm_args: Vec::new(),
            game_args: Vec::new(),
            libraries: Vec::new(),
            directory_libraries: Vec::new(),
        });
    };
    let parsed: InstallerVersionJson = serde_json::from_str(&version_json)
        .map_err(|error| format!("failed to parse installer version metadata: {error}"))?;
    let main_class = parsed.main_class;
    let jvm_args = parsed
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.jvm.clone())
        .unwrap_or_default();
    let game_args = parsed
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.game.clone())
        .unwrap_or_default();
    let libraries = fetch_installer_libraries(parsed.libraries.as_deref(), cache_dir, true)?;
    let directory_libraries = match jar_entry_text(installer_path, &["install_profile.json"])? {
        Some(profile) => {
            let parsed: InstallProfileJson = serde_json::from_str(&profile)
                .map_err(|error| format!("failed to parse installer profile: {error}"))?;
            fetch_installer_libraries(parsed.libraries.as_deref(), cache_dir, false)?
        }
        None => Vec::new(),
    };

    Ok(InstallerProfile {
        main_class,
        jvm_args,
        game_args,
        libraries,
        directory_libraries,
    })
}

/// Downloads the libraries an installer lists. Entries without a URL are generated during
/// installation; they are kept, undownloaded, only when `keep_generated` is set.
fn fetch_installer_libraries(
    entries: Option<&[InstallerLibraryEntry]>,
    cache_dir: &Path,
    keep_generated: bool,
) -> Result<Vec<InstallerProfileLibrary>, String> {
    let mut libraries = Vec::new();
    for library in entries.unwrap_or_default() {
        if !rules_allow(&library.rules, current_os()) {
            continue;
        }
        let Some(artifact) = library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
        else {
            continue;
        };
        if artifact.url.is_empty() {
            if keep_generated {
                libraries.push(InstallerProfileLibrary {
                    name: library.name.clone(),
                    path: artifact.path.clone(),
                    download: None,
                });
            }
            continue;
        }
        let library_path = fetch_library(
            &artifact.url,
            &artifact.path,
            artifact.sha1.as_deref(),
            cache_dir,
        )?;
        let bytes = fs::read(&library_path)
            .map_err(|error| format!("failed to read {}: {error}", library_path.display()))?;

        libraries.push(InstallerProfileLibrary {
            name: library.name.clone(),
            path: artifact.path.clone(),
            download: Some((artifact.url.clone(), sha256_hex(&bytes))),
        });
    }
    Ok(libraries)
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
            .map_err(|error| {
                format!("failed to run jar tool for {}: {error}", jar_path.display())
            })?;
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
