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
fn resolve_downloads_direct_modrinth_mod_file_into_the_lockfile() {
    let project = temp_dir("modrinth-project");
    let metadata = temp_dir("modrinth-metadata");
    let data_home = temp_dir("modrinth-data");
    let cache_home = temp_dir("modrinth-cache");
    let jar = metadata.join("sample-mod-1.0.0.jar");
    fs::write(&jar, b"abc").expect("failed to write Modrinth jar");
    let versions = metadata.join("sample-mod-versions.json");
    fs::write(
        &versions,
        format!(
            r#"[{{
  "id": "sample-version",
  "project_id": "sample-project",
  "version_number": "1.0.0",
  "game_versions": ["26.1.2"],
  "loaders": ["fabric"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-1.0.0.jar",
    "url": "file://{}",
    "hashes": {{
      "sha512": "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
      "sha1": "a9993e364706816aba3e25717850c26c9cd0d89d"
    }}
  }}]
}}]"#,
            jar.display()
        ),
    )
    .expect("failed to write Modrinth versions metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "modrinth-test"

[[instance]]
name = "modrinth-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client", "server"]
mods = [
  "modrinth:sample-mod",
]
"#,
    )
    .expect("failed to write config");

    let versions_url = format!("file://{}", versions.display());
    let output = run_in_with_env(
        &["resolve", "modrinth-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        "[[mod]]",
        r#"source = "modrinth:sample-mod""#,
        r#"provider = "modrinth""#,
        r#"project = "sample-mod""#,
        r#"version_id = "sample-version""#,
        r#"version_number = "1.0.0""#,
        r#"filename = "sample-mod-1.0.0.jar""#,
        &format!(r#"url = "file://{}""#, jar.display()),
        r#"sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d""#,
        r#"sha512 = "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f""#,
        r#"sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad""#,
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
