use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run_in_with_env(args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> Output {
    let mut command = modstage();
    command.args(args).current_dir(cwd);

    for (key, value) in envs {
        command.env(key, value);
    }

    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
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

#[test]
fn resolve_records_neoforge_loader_metadata() {
    let project = temp_dir("neoforge-project");
    let metadata = temp_dir("neoforge-metadata");
    let data_home = temp_dir("neoforge-data");
    let cache_home = temp_dir("neoforge-cache");
    let neoforge_metadata = metadata.join("neoforge-loader.json");
    fs::write(
        &neoforge_metadata,
        r#"{
  "version": "4.0.0",
  "installer_maven": "net.neoforged:neoforge:4.0.0",
  "client_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher",
  "server_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher"
}"#,
    )
    .expect("failed to write NeoForge metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "neoforge-test"

[[instance]]
name = "neoforge-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "latest"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let neoforge_url = format!("file://{}", neoforge_metadata.display());
    let output = run_in_with_env(
        &["resolve", "neoforge-26.1.2"],
        &project,
        &[
            ("MODSTAGE_NEOFORGE_META_URL", &neoforge_url),
            ("XDG_DATA_HOME", data_home.to_str().expect("data path is not UTF-8")),
            ("XDG_CACHE_HOME", cache_home.to_str().expect("cache path is not UTF-8")),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(project.join("modstage.lock"))
        .expect("modstage.lock should exist");

    for expected in [
        "[loader]",
        r#"kind = "neoforge""#,
        r#"version = "4.0.0""#,
        r#"installer_maven = "net.neoforged:neoforge:4.0.0""#,
        r#"client_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
        r#"server_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn resolve_adds_neoforge_installer_artifact_to_the_launch_classpath() {
    let project = temp_dir("neoforge-classpath-project");
    let metadata = temp_dir("neoforge-classpath-metadata");
    let data_home = temp_dir("neoforge-classpath-data");
    let cache_home = temp_dir("neoforge-classpath-cache");
    let repo = metadata.join("repo");
    let installer_dir = repo
        .join("net")
        .join("neoforged")
        .join("neoforge")
        .join("4.0.0");
    fs::create_dir_all(&installer_dir).expect("failed to create installer artifact dir");
    fs::write(installer_dir.join("neoforge-4.0.0.jar"), b"installer")
        .expect("failed to write installer jar");
    let neoforge_metadata = metadata.join("neoforge-loader.json");
    fs::write(
        &neoforge_metadata,
        r#"{
  "version": "4.0.0",
  "installer_maven": "net.neoforged:neoforge:4.0.0",
  "client_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher",
  "server_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher"
}"#,
    )
    .expect("failed to write NeoForge metadata");
    fs::write(
        project.join("modstage.toml"),
        format!(
            r#"[project]
name = "neoforge-classpath-test"

[repositories]
neoforge = "file://{}"

[[instance]]
name = "neoforge-classpath-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "latest"
sides = ["client", "server"]
"#,
            repo.display()
        ),
    )
    .expect("failed to write config");

    let neoforge_url = format!("file://{}", neoforge_metadata.display());
    let output = run_in_with_env(
        &["resolve", "neoforge-classpath-26.1.2"],
        &project,
        &[
            ("MODSTAGE_NEOFORGE_META_URL", &neoforge_url),
            ("XDG_DATA_HOME", data_home.to_str().expect("data path is not UTF-8")),
            ("XDG_CACHE_HOME", cache_home.to_str().expect("cache path is not UTF-8")),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(project.join("modstage.lock"))
        .expect("modstage.lock should exist");

    for expected in [
        "[[library]]",
        r#"name = "net.neoforged:neoforge:4.0.0""#,
        r#"repository = "neoforge""#,
        r#"path = ""#,
        r#"sha256 = "9c0d294c05fc1d88d698034609bb81c0c69196327594e4c69d2915c80fd9850c""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
