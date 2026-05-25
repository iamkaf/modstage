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
fn resolve_fetches_and_records_mojang_version_metadata() {
    let project = temp_dir("mojang-project");
    let metadata = temp_dir("mojang-metadata");
    let data_home = temp_dir("mojang-data");
    let cache_home = temp_dir("mojang-cache");
    let version_json = metadata.join("26.1.2.json");
    fs::write(&version_json, r#"{"id":"26.1.2","javaVersion":{"majorVersion":25}}"#)
        .expect("failed to write version json");
    let manifest = metadata.join("version_manifest.json");
    fs::write(
        &manifest,
        format!(
            r#"{{ "versions": [{{ "id": "26.1.2", "url": "file://{}" }}] }}"#,
            version_json.display()
        ),
    )
    .expect("failed to write manifest");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "mojang-test"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let output = run_in_with_env(
        &["resolve", "vanilla-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
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
        "[minecraft]",
        r#"version = "26.1.2""#,
        &format!(r#"manifest_url = "{manifest_url}""#),
        &format!(r#"version_url = "file://{}""#, version_json.display()),
        "manifest_sha256 = ",
        "version_sha256 = ",
        r#"java_major = 25"#,
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
