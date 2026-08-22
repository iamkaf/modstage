use super::*;
use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

const HTTP_USER_AGENT: &str = concat!(
    "iamkaf/modstage/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/iamkaf/modstage)"
);

static HTTP_CLIENT: LazyLock<reqwest::blocking::Client> = LazyLock::new(|| {
    reqwest::blocking::Client::builder()
        .user_agent(HTTP_USER_AGENT)
        .timeout(Duration::from_secs(120))
        .build()
        .expect("HTTP client configuration should be valid")
});

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn fetch_to_cache(
    url: &str,
    cache_dir: &Path,
    file_name: &str,
) -> Result<PathBuf, String> {
    fetch_to_cache_inner(url, cache_dir, file_name, None)
}

pub(super) fn fetch_to_cache_with_ttl(
    url: &str,
    cache_dir: &Path,
    file_name: &str,
    ttl: Duration,
) -> Result<PathBuf, String> {
    fetch_to_cache_inner(url, cache_dir, file_name, Some(ttl))
}

fn fetch_to_cache_inner(
    url: &str,
    cache_dir: &Path,
    file_name: &str,
    ttl: Option<Duration>,
) -> Result<PathBuf, String> {
    fs::create_dir_all(cache_dir)
        .map_err(|error| format!("failed to create {}: {error}", cache_dir.display()))?;
    let destination = cache_dir.join(file_name);

    if let Some(ttl) = ttl
        && cached_file_is_fresh(&destination, ttl)
    {
        return Ok(destination);
    }

    if let Some(path) = file_url_to_path(url)? {
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
        let temp_destination = temporary_destination(&destination)?;
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
                if destination.is_file() {
                    return Ok(destination);
                }
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

fn temporary_destination(destination: &Path) -> Result<PathBuf, String> {
    let file_name = destination.file_name().ok_or_else(|| {
        format!(
            "cache destination has no filename: {}",
            destination.display()
        )
    })?;
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp_name = format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        sequence
    );
    Ok(destination.with_file_name(temp_name))
}

fn cached_file_is_fresh(path: &Path, ttl: Duration) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    modified.elapsed().is_ok_and(|age| age <= ttl)
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
        } else if url.starts_with("file:") {
            let root =
                file_url_to_path(url)?.ok_or_else(|| format!("unsupported file URL `{url}`"))?;
            if let Some(path) = maven_artifact_under(root, coordinates) {
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

fn file_url_to_path(value: &str) -> Result<Option<PathBuf>, String> {
    if !value.starts_with("file:") {
        return Ok(None);
    }

    let url =
        url::Url::parse(value).map_err(|error| format!("invalid file URL `{value}`: {error}"))?;
    url.to_file_path()
        .map(Some)
        .map_err(|()| format!("file URL `{value}` does not identify a local path"))
}

pub(super) fn maven_artifact_url(repository: &str, coordinates: &MavenCoordinates<'_>) -> String {
    format!(
        "{}/{}",
        repository.trim_end_matches('/'),
        coordinates.artifact_relative_path()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_downloads_stay_beside_the_destination_and_are_unique() {
        let destination = Path::new("cache").join("nested").join("artifact.jar");

        let first = temporary_destination(&destination).unwrap();
        let second = temporary_destination(&destination).unwrap();

        assert_eq!(first.parent(), destination.parent());
        assert_eq!(second.parent(), destination.parent());
        assert_ne!(first, second);
        assert!(
            first
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".artifact.jar.")
        );
    }
}
