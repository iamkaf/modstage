use super::*;

pub(super) struct StateDirs {
    pub(super) project_id: String,
    pub(super) data: PathBuf,
    pub(super) cache: PathBuf,
}

impl StateDirs {
    pub(super) fn for_project(project_name: &str, root: &Path) -> Result<Self, String> {
        let project_id = format!(
            "{}-{:08x}",
            project_name,
            stable_hash(&root.display().to_string())
        );
        let data = data_dir()?;
        let cache = cache_dir()?;

        Ok(Self {
            project_id,
            data,
            cache,
        })
    }
}

pub(super) fn data_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("MODSTAGE_DATA_HOME") {
        return Ok(PathBuf::from(path).join("modstage"));
    }

    Ok(data_home()?.join("modstage"))
}

pub(super) fn cache_dir() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("MODSTAGE_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("modstage"));
    }

    #[cfg(target_os = "windows")]
    return Ok(cache_home()?.join("modstage").join("Cache"));

    #[cfg(not(target_os = "windows"))]
    Ok(cache_home()?.join("modstage"))
}

#[cfg(target_os = "linux")]
pub(super) fn data_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".local").join("share"))
}

#[cfg(target_os = "linux")]
pub(super) fn cache_home() -> Result<PathBuf, String> {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path));
    }

    Ok(home_dir()?.join(".cache"))
}

#[cfg(target_os = "macos")]
pub(super) fn data_home() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library").join("Application Support"))
}

#[cfg(target_os = "macos")]
pub(super) fn cache_home() -> Result<PathBuf, String> {
    Ok(home_dir()?.join("Library").join("Caches"))
}

#[cfg(target_os = "windows")]
pub(super) fn data_home() -> Result<PathBuf, String> {
    env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "APPDATA is not set".to_string())
}

#[cfg(target_os = "windows")]
pub(super) fn cache_home() -> Result<PathBuf, String> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is not set".to_string())
}

pub(super) fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

pub(super) fn stable_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5_u32;

    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }

    hash
}
