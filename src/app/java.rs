fn java_list() -> Result<(), String> {
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

fn java_doctor(args: &[String]) -> Result<(), String> {
    let java = parse_java_arg(args)?.unwrap_or_else(|| PathBuf::from(java_bin()));
    let info = inspect_java(&java)?;

    print_java_info(&java, &info);

    Ok(())
}

fn java_install(major: &str) -> Result<(), String> {
    let major: u32 = major
        .parse()
        .map_err(|error| format!("invalid Java major version `{major}`: {error}"))?;
    let metadata_url = azul_metadata_url(major)?;
    let cache_dir = cache_home()?.join("modstage").join("downloads").join("java");
    let metadata_path = fetch_to_cache(&metadata_url, &cache_dir, &format!("azul-{major}.json"))?;
    let metadata = fs::read_to_string(&metadata_path)
        .map_err(|error| format!("failed to read {}: {error}", metadata_path.display()))?;
    let download_url = json_string(&metadata, "download_url")
        .ok_or_else(|| "Azul metadata did not include download_url".to_string())?;
    let archive_name = json_string(&metadata, "name")
        .unwrap_or_else(|| format!("zulu-java-{major}.zip"));
    let archive_path = fetch_to_cache(&download_url, &cache_dir, &archive_name)?;
    let archive = fs::read(&archive_path)
        .map_err(|error| format!("failed to read {}: {error}", archive_path.display()))?;

    println!("java major: {major}");
    println!("java archive: {}", archive_path.display());
    println!("sha256: {}", sha256_hex(&archive));

    Ok(())
}

fn azul_metadata_url(major: u32) -> Result<String, String> {
    if let Ok(url) = env::var("MODSTAGE_AZUL_METADATA_URL") {
        return Ok(url);
    }

    Ok(format!(
        "https://api.azul.com/metadata/v1/zulu/packages?arch={}&java_version={major}&os={}&archive_type=zip&javafx_bundled=false&java_package_type=jre&page_size=1",
        env::consts::ARCH,
        env::consts::OS
    ))
}

fn parse_java_arg(args: &[String]) -> Result<Option<PathBuf>, String> {
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

fn discover_java_runtimes() -> Vec<PathBuf> {
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

fn inspect_java(java: &Path) -> Result<JavaInfo, String> {
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

fn property(text: &str, key: &str) -> Option<String> {
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

fn java_major(version: &str) -> Result<u32, String> {
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

fn print_java_info(java: &Path, info: &JavaInfo) {
    println!("java: {}", java.display());
    println!("version: {}", info.version);
    println!("major: {}", info.major);
    println!("arch: {}", info.arch);
}

struct JavaInfo {
    version: String,
    major: u32,
    arch: String,
}

fn java_bin() -> &'static str {
    if cfg!(target_os = "windows") {
        "java.exe"
    } else {
        "java"
    }
}
