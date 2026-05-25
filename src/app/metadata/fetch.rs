use super::*;

pub(in crate::app) fn fetch_to_cache(
    url: &str,
    cache_dir: &Path,
    file_name: &str,
) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_dir)
        .map_err(|error| format!("failed to create {}: {error}", cache_dir.display()))?;
    let destination = cache_dir.join(file_name);

    if let Some(path) = url.strip_prefix("file://") {
        fs::copy(path, &destination).map_err(|error| {
            format!("failed to copy {url} to {}: {error}", destination.display())
        })?;
        return Ok(destination);
    }

    if url.starts_with("https://") || url.starts_with("http://") {
        let status = Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&destination)
            .arg(url)
            .status()
            .map_err(|error| format!("failed to run curl for {url}: {error}"))?;

        if status.success() {
            return Ok(destination);
        }

        return Err(format!("curl failed for {url} with status {status}"));
    }

    Err(format!("unsupported URL `{url}`"))
}
