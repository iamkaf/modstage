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
fn resolve_records_fabric_loader_metadata() {
    let project = temp_dir("fabric-project");
    let metadata = temp_dir("fabric-metadata");
    let data_home = temp_dir("fabric-data");
    let cache_home = temp_dir("fabric-cache");
    let fabric_metadata = metadata.join("fabric-loader.json");
    fs::write(
        &fabric_metadata,
        r#"[{
  "loader": {
    "version": "0.16.14",
    "maven": "net.fabricmc:fabric-loader:0.16.14"
  },
  "intermediary": {
    "maven": "net.fabricmc:intermediary:26.1.2"
  },
  "launcherMeta": {
    "mainClass": {
      "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
      "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
    }
  }
}]"#,
    )
    .expect("failed to write fabric metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "fabric-test"

[[instance]]
name = "fabric-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let fabric_url = format!("file://{}", fabric_metadata.display());
    let output = run_in_with_env(
        &["resolve", "fabric-26.1.2"],
        &project,
        &[
            ("MODSTAGE_FABRIC_META_URL", &fabric_url),
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
        r#"kind = "fabric""#,
        r#"version = "0.16.14""#,
        r#"loader_maven = "net.fabricmc:fabric-loader:0.16.14""#,
        r#"intermediary_maven = "net.fabricmc:intermediary:26.1.2""#,
        r#"client_main_class = "net.fabricmc.loader.impl.launch.knot.KnotClient""#,
        r#"server_main_class = "net.fabricmc.loader.impl.launch.knot.KnotServer""#,
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
