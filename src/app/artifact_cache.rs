use super::*;
use std::sync::LazyLock;

static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .build()
        .expect("HTTP client configuration should be valid")
});

pub(super) fn fetch_to_cache(
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

pub(super) struct ResolvedMavenArtifact {
    pub(super) repository: String,
    pub(super) path: PathBuf,
    pub(super) url: Option<String>,
}

pub(super) fn resolve_maven_artifact(
    repositories: &[(String, String)],
    coordinates: &MavenCoordinates<'_>,
    cache_dir: &Path,
) -> Result<Option<ResolvedMavenArtifact>, String> {
    for (name, url) in repositories {
        if url == "mavenLocal" {
            if let Some(path) = maven_local_artifact(coordinates) {
                return Ok(Some(ResolvedMavenArtifact {
                    repository: name.clone(),
                    path,
                    url: None,
                }));
            }
        } else if let Some(root) = url.strip_prefix("file://") {
            if let Some(path) = maven_artifact_under(PathBuf::from(root), coordinates) {
                return Ok(Some(ResolvedMavenArtifact {
                    repository: name.clone(),
                    path,
                    url: None,
                }));
            }
        } else if url.starts_with("https://") || url.starts_with("http://") {
            let artifact_url = maven_artifact_url(url, coordinates);
            if let Ok(path) = fetch_to_cache(
                &artifact_url,
                &cache_dir.join(name),
                &coordinates.file_name(),
            ) {
                return Ok(Some(ResolvedMavenArtifact {
                    repository: name.clone(),
                    path,
                    url: Some(artifact_url),
                }));
            }
        }
    }

    Ok(
        maven_local_artifact(coordinates).map(|path| ResolvedMavenArtifact {
            repository: "mavenLocal".to_string(),
            path,
            url: None,
        }),
    )
}

pub(super) fn maven_artifact_url(repository: &str, coordinates: &MavenCoordinates<'_>) -> String {
    format!(
        "{}/{}",
        repository.trim_end_matches('/'),
        coordinates.artifact_relative_path()
    )
}
