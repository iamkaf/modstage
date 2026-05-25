use super::*;
use std::sync::LazyLock;

static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .build()
        .expect("HTTP client configuration should be valid")
});

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
        let response = HTTP_CLIENT
            .get(url)
            .send()
            .map_err(|error| format!("failed to fetch {url}: {error}"))?
            .error_for_status()
            .map_err(|error| format!("failed to fetch {url}: {error}"))?;
        let bytes = response
            .bytes()
            .map_err(|error| format!("failed to read response body from {url}: {error}"))?;
        let temp_name = format!(".{}.{:?}.tmp", file_name, std::thread::current().id());
        let temp_destination = destination.with_file_name(temp_name);
        fs::write(&temp_destination, &bytes).map_err(|error| {
            format!(
                "failed to write {} from {url}: {error}",
                temp_destination.display()
            )
        })?;
        match fs::rename(&temp_destination, &destination) {
            Ok(()) => return Ok(destination),
            Err(error) => {
                let _ = fs::remove_file(&temp_destination);
                return Err(format!(
                    "failed to move {} to {}: {error}",
                    temp_destination.display(),
                    destination.display()
                ));
            }
        }
    }

    Err(format!("unsupported URL `{url}`"))
}
