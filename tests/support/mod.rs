use std::path::Path;

pub fn file_url(path: &Path) -> String {
    url::Url::from_file_path(path)
        .unwrap_or_else(|()| panic!("{} should convert to a file URL", path.display()))
        .to_string()
}

pub fn file_url_path(path: &Path) -> String {
    file_url(path)
        .strip_prefix("file://")
        .expect("file URL should have a file:// prefix")
        .to_string()
}
