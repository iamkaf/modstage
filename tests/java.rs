use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run(args: &[&str]) -> Output {
    modstage()
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

fn run_with_env(args: &[&str], envs: &[(&str, &str)]) -> Output {
    let mut command = modstage();
    command.args(args);

    for (key, value) in envs {
        command.env(key, value);
    }

    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

#[test]
fn java_doctor_reports_the_configured_java_runtime() {
    let java = java_on_path().expect("test environment must have java on PATH");
    let output = run(&["java", "doctor", "--java", java.to_str().expect("java path is not UTF-8")]);

    assert!(
        output.status.success(),
        "java doctor should validate the configured runtime\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("java:")
            && text.contains("version:")
            && text.contains("major:")
            && text.contains("arch:"),
        "java doctor should report runtime details\n{text}"
    );
}

#[test]
fn java_list_reports_discovered_runtimes() {
    let output = run(&["java", "list"]);

    assert!(
        output.status.success(),
        "java list should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("java:") && text.contains("major:"),
        "java list should report at least one runtime\n{text}"
    );
}

fn java_on_path() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;

    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(java_bin());
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn java_bin() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

#[test]
fn java_install_caches_a_managed_runtime_archive() {
    let temp = temp_dir("java-install");
    let data_home = temp.join("data");
    let cache_home = temp.join("cache");
    let archive = temp.join("zulu-jre.zip");
    fs::write(&archive, b"abc").expect("failed to write fake java archive");
    let metadata = temp.join("azul.json");
    fs::write(
        &metadata,
        format!(
            r#"[{{"download_url":"file://{}","name":"zulu-test-jre.zip"}}]"#,
            archive.display()
        ),
    )
    .expect("failed to write fake Azul metadata");

    let output = run_with_env(
        &["java", "install", "25"],
        &[
            ("MODSTAGE_AZUL_METADATA_URL", &format!("file://{}", metadata.display())),
            ("XDG_DATA_HOME", data_home.to_str().expect("data path is not UTF-8")),
            ("XDG_CACHE_HOME", cache_home.to_str().expect("cache path is not UTF-8")),
        ],
    );

    assert!(
        output.status.success(),
        "java install should cache the runtime archive\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let cached = cache_home
        .join("modstage")
        .join("downloads")
        .join("java")
        .join("zulu-test-jre.zip");
    assert!(
        cached.is_file()
            && stdout.contains("java archive:")
            && stdout.contains("sha256: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        "java install should report cached archive and hash\n{stdout}"
    );

    fs::remove_dir_all(temp).expect("failed to remove temp dir");
}

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "modstage-{name}-{}-{nanos}",
        std::process::id()
    ));

    fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}
