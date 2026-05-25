use super::*;

pub(super) fn java_list() -> Result<(), String> {
    let runtimes = discover_java_runtimes();

    if runtimes.is_empty() {
        return Err("no Java runtimes found on PATH".to_string());
    }

    for runtime in runtimes {
        let info = inspect_java(&runtime)?;
        print_java_info(&runtime, &info);
    }

    Ok(())
}

pub(super) fn java_doctor(args: &[String]) -> Result<(), String> {
    let java = parse_java_arg(args)?.unwrap_or_else(|| PathBuf::from(java_bin()));
    let info = inspect_java(&java)?;

    print_java_info(&java, &info);

    Ok(())
}

pub(super) fn java_install(major: &str, args: &[String]) -> Result<(), String> {
    let major: u32 = major
        .parse()
        .map_err(|error| format!("invalid Java major version `{major}`: {error}"))?;
    if let Some(java) = parse_java_arg(args)? {
        return register_existing_java(major, &java);
    }
    let metadata_url = azul_metadata_url(major)?;
    let cache_dir = cache_home()?
        .join("modstage")
        .join("downloads")
        .join("java");
    let metadata_path = fetch_to_cache(&metadata_url, &cache_dir, &format!("azul-{major}.json"))?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let download_url = json_string(&metadata, "download_url")
        .ok_or_else(|| "Azul metadata did not include download_url".to_string())?;
    let archive_name =
        json_string(&metadata, "name").unwrap_or_else(|| format!("zulu-java-{major}.zip"));
    let archive_path = fetch_to_cache(&download_url, &cache_dir, &archive_name)?;
    let archive = fs::read(&archive_path)
        .map_err(|error| format!("failed to read {}: {error}", archive_path.display()))?;
    let sha256 = sha256_hex(&archive);
    let install_dir = data_home()?
        .join("modstage")
        .join("java")
        .join(major.to_string());
    fs::create_dir_all(&install_dir)
        .map_err(|error| format!("failed to create {}: {error}", install_dir.display()))?;
    let managed_archive = install_dir.join(&archive_name);
    fs::copy(&archive_path, &managed_archive).map_err(|error| {
        format!(
            "failed to copy Java archive to {}: {error}",
            managed_archive.display()
        )
    })?;
    fs::write(
        install_dir.join("runtime.toml"),
        format!(
            "major = {major}\nmetadata_url = \"{}\"\ndownload_url = \"{}\"\narchive_name = \"{}\"\narchive = \"{}\"\nsha256 = \"{}\"\n",
            toml_escape(&metadata_url),
            toml_escape(&download_url),
            toml_escape(&archive_name),
            toml_escape(&managed_archive.display().to_string()),
            sha256
        ),
    )
    .map_err(|error| format!("failed to write managed Java record: {error}"))?;

    println!("java major: {major}");
    println!("java archive: {}", archive_path.display());
    println!("managed java: {}", install_dir.display());
    println!("sha256: {sha256}");

    Ok(())
}

pub(super) fn register_existing_java(major: u32, java: &Path) -> Result<(), String> {
    let info = inspect_java(java)?;
    if info.major != major {
        return Err(format!(
            "{} reported Java {} but Java {major} was requested",
            java.display(),
            info.major
        ));
    }

    let install_dir = data_home()?
        .join("modstage")
        .join("java")
        .join(major.to_string());
    fs::create_dir_all(&install_dir)
        .map_err(|error| format!("failed to create {}: {error}", install_dir.display()))?;
    fs::write(
        install_dir.join("runtime.toml"),
        format!(
            "major = {major}\njava = \"{}\"\nversion = \"{}\"\narch = \"{}\"\n",
            toml_escape(&java.display().to_string()),
            toml_escape(&info.version),
            toml_escape(&info.arch)
        ),
    )
    .map_err(|error| format!("failed to write managed Java record: {error}"))?;

    println!("java major: {major}");
    println!("java: {}", java.display());
    println!("version: {}", info.version);
    println!("managed java: {}", install_dir.display());

    Ok(())
}

pub(super) fn azul_metadata_url(major: u32) -> Result<String, String> {
    if let Ok(url) = env::var("MODSTAGE_AZUL_METADATA_URL") {
        return Ok(url);
    }

    Ok(format!(
        "https://api.azul.com/metadata/v1/zulu/packages?arch={}&java_version={major}&os={}&archive_type=zip&javafx_bundled=false&java_package_type=jre&page_size=1",
        env::consts::ARCH,
        env::consts::OS
    ))
}

pub(super) fn parse_java_arg(args: &[String]) -> Result<Option<PathBuf>, String> {
    let mut java = None;
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        if arg == "--java" {
            let path = iter
                .next()
                .ok_or_else(|| "--java requires a path".to_string())?;
            java = Some(PathBuf::from(path));
        } else {
            return Err(format!("unknown java doctor option `{arg}`"));
        }
    }

    Ok(java)
}

pub(super) fn managed_java_for_major(major: u32) -> Result<Option<PathBuf>, String> {
    let install_dir = data_home()?
        .join("modstage")
        .join("java")
        .join(major.to_string());
    let record_path = install_dir.join("runtime.toml");
    if record_path.is_file() {
        let record = fs::read_to_string(&record_path)
            .map_err(|error| format!("failed to read {}: {error}", record_path.display()))?;
        if let Some(java) = block_string_value(&record, "java") {
            let java = PathBuf::from(java);
            if java.is_file() {
                validate_managed_java_major(&java, major)?;
                return Ok(Some(java));
            }
        }
    }

    let java = install_dir.join("bin").join(java_bin());
    if java.is_file() {
        validate_managed_java_major(&java, major)?;
        return Ok(Some(java));
    }

    Ok(None)
}

pub(super) fn validate_managed_java_major(java: &Path, expected: u32) -> Result<(), String> {
    let info = inspect_java(java)?;
    if info.major != expected {
        return Err(format!(
            "managed Java {} requires Java {expected} but reported Java {}",
            java.display(),
            info.major
        ));
    }

    Ok(())
}

pub(super) fn discover_java_runtimes() -> Vec<PathBuf> {
    let Some(path) = env::var_os("PATH") else {
        return Vec::new();
    };
    let mut runtimes = Vec::new();

    for dir in env::split_paths(&path) {
        let candidate = dir.join(java_bin());
        if candidate.is_file() && !runtimes.contains(&candidate) {
            runtimes.push(candidate);
        }
    }

    runtimes
}

pub(super) fn inspect_java(java: &Path) -> Result<JavaInfo, String> {
    let output = Command::new(java)
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .map_err(|error| format!("failed to run {}: {error}", java.display()))?;

    if !output.status.success() {
        return Err(format!(
            "{} failed Java validation with status {}",
            java.display(),
            output.status
        ));
    }

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let version = property(&text, "java.version")
        .ok_or_else(|| format!("{} did not report java.version", java.display()))?;
    let arch = property(&text, "os.arch")
        .ok_or_else(|| format!("{} did not report os.arch", java.display()))?;
    let major = java_major(&version)?;

    Ok(JavaInfo {
        version,
        major,
        arch,
    })
}

pub(super) fn property(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };

        if name.trim() == key {
            return Some(value.trim().to_string());
        }
    }

    None
}

pub(super) fn java_major(version: &str) -> Result<u32, String> {
    let mut parts = version.split('.');
    let first = parts
        .next()
        .ok_or_else(|| format!("invalid Java version `{version}`"))?;
    let major = if first == "1" {
        parts
            .next()
            .ok_or_else(|| format!("invalid Java version `{version}`"))?
    } else {
        first
    };
    let major = major
        .split_once('-')
        .map_or(major, |(before_dash, _)| before_dash);

    major
        .parse()
        .map_err(|error| format!("invalid Java version `{version}`: {error}"))
}

pub(super) fn print_java_info(java: &Path, info: &JavaInfo) {
    println!("java: {}", java.display());
    println!("version: {}", info.version);
    println!("major: {}", info.major);
    println!("arch: {}", info.arch);
}

pub(super) struct JavaInfo {
    version: String,
    major: u32,
    arch: String,
}

pub(super) fn java_bin() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}
