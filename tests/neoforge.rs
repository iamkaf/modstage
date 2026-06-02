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
    if !envs
        .iter()
        .any(|(key, _)| *key == "MODSTAGE_MOJANG_MANIFEST_URL")
    {
        command.env("MODSTAGE_MOJANG_MANIFEST_URL", default_mojang_manifest(cwd));
    }

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
    let root = std::env::temp_dir().join(format!("modstage-{name}-{}-{nanos}", std::process::id()));

    fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}

fn push_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_stored_jar(path: &Path, entries: &[(&str, &[u8])]) {
    let mut bytes = Vec::new();
    let mut central = Vec::new();

    for (name, contents) in entries {
        let offset = bytes.len() as u32;
        push_u32(&mut bytes, 0x0403_4b50);
        push_u16(&mut bytes, 20);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, contents.len() as u32);
        push_u32(&mut bytes, contents.len() as u32);
        push_u16(&mut bytes, name.len() as u16);
        push_u16(&mut bytes, 0);
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(contents);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, 20);
        push_u16(&mut central, 20);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, contents.len() as u32);
        push_u32(&mut central, contents.len() as u32);
        push_u16(&mut central, name.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, 0);
        push_u32(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }

    let central_offset = bytes.len() as u32;
    let central_size = central.len() as u32;
    bytes.extend_from_slice(&central);
    push_u32(&mut bytes, 0x0605_4b50);
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, 0);
    push_u16(&mut bytes, entries.len() as u16);
    push_u16(&mut bytes, entries.len() as u16);
    push_u32(&mut bytes, central_size);
    push_u32(&mut bytes, central_offset);
    push_u16(&mut bytes, 0);

    fs::write(path, bytes).expect("failed to write stored jar");
}

fn default_mojang_manifest(root: &Path) -> String {
    let metadata = root.join(".modstage-test-mojang");
    fs::create_dir_all(&metadata).expect("failed to create test Mojang metadata dir");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "file://{}" }},
    "server": {{ "url": "file://{}" }}
  }}
}}"#,
            client.display(),
            server.display()
        ),
    )
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
    format!("file://{}", manifest.display())
}

fn state_lock_path(data_home: &Path, root: &Path, project_name: &str, instance: &str) -> PathBuf {
    data_home
        .join("modstage")
        .join("instances")
        .join(format!(
            "{project_name}-{:08x}",
            stable_hash(
                &root
                    .canonicalize()
                    .expect("project root should canonicalize")
                    .display()
                    .to_string()
            )
        ))
        .join(instance)
        .join("modstage.lock")
}

fn stable_hash(value: &str) -> u32 {
    let mut hash = 0x811c9dc5_u32;

    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x01000193);
    }

    hash
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
            (
                "XDG_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "XDG_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "neoforge-test",
        "neoforge-26.1.2",
    ))
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
fn resolve_neoforge_latest_uses_neoforged_maven_metadata() {
    let project = temp_dir("neoforge-manifest-project");
    let metadata = temp_dir("neoforge-manifest-metadata");
    let data_home = temp_dir("neoforge-manifest-data");
    let cache_home = temp_dir("neoforge-manifest-cache");
    let neoforge_manifest = metadata.join("neoforge-manifest.json");
    fs::write(
        &neoforge_manifest,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<metadata>
  <groupId>net.neoforged</groupId>
  <artifactId>neoforge</artifactId>
  <versioning>
    <versions>
      <version>21.1.231</version>
      <version>26.1.2.21-beta</version>
      <version>26.1.2.22-beta</version>
    </versions>
  </versioning>
</metadata>
"#,
    )
    .expect("failed to write NeoForge manifest");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "neoforge-manifest-test"

[[instance]]
name = "neoforge-latest-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "latest"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", neoforge_manifest.display());
    let output = run_in_with_env(
        &["resolve", "neoforge-latest-26.1.2"],
        &project,
        &[
            ("MODSTAGE_NEOFORGE_MAVEN_METADATA_URL", &manifest_url),
            (
                "XDG_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "XDG_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "neoforge-manifest-test",
        "neoforge-latest-26.1.2",
    ))
    .expect("modstage.lock should exist");
    for expected in [
        "[loader]",
        r#"kind = "neoforge""#,
        r#"version = "26.1.2.22-beta""#,
        r#"installer_maven = "net.neoforged:neoforge:26.1.2.22-beta:installer""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains("26.1.2.21-beta"),
        "latest should use the newest NeoForge version for the selected Minecraft version\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn resolve_uses_pinned_neoforge_loader_version_without_metadata_override() {
    let project = temp_dir("neoforge-pinned-project");
    let metadata = temp_dir("neoforge-pinned-metadata");
    let data_home = temp_dir("neoforge-pinned-data");
    let cache_home = temp_dir("neoforge-pinned-cache");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let installer = metadata
        .join("net")
        .join("neoforged")
        .join("neoforge")
        .join("26.1.2.22-beta")
        .join("neoforge-26.1.2.22-beta-installer.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::create_dir_all(
        installer
            .parent()
            .expect("installer jar should have parent"),
    )
    .expect("failed to create NeoForge maven dir");
    write_stored_jar(&installer, &[]);
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "file://{}" }},
    "server": {{ "url": "file://{}" }}
  }}
}}"#,
            client.display(),
            server.display()
        ),
    )
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
        format!(
            r#"[project]
name = "neoforge-pinned-test"

[repositories]
neoforge = "file://{}"

[[instance]]
name = "neoforge-pinned-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "26.1.2.22-beta"
sides = ["client", "server"]
"#,
            metadata.display()
        ),
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let output = run_in_with_env(
        &["resolve", "neoforge-pinned-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            (
                "XDG_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "XDG_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should use pinned NeoForge metadata and built-in Maven\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "neoforge-pinned-test",
        "neoforge-pinned-26.1.2",
    ))
    .expect("modstage.lock should exist");
    for expected in [
        r#"kind = "neoforge""#,
        r#"version = "26.1.2.22-beta""#,
        r#"installer_maven = "net.neoforged:neoforge:26.1.2.22-beta:installer""#,
        r#"client_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
        r#"server_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains(r#"name = "net.neoforged:neoforge:26.1.2.22-beta:installer""#),
        "NeoForge installer should be used as metadata, not added to the launch classpath\n{lock}"
    );

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
    write_stored_jar(&installer_dir.join("neoforge-4.0.0.jar"), &[]);
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
            (
                "XDG_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "XDG_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "neoforge-classpath-test",
        "neoforge-classpath-26.1.2",
    ))
    .expect("modstage.lock should exist");

    assert!(
        !lock.contains(r#"name = "net.neoforged:neoforge:4.0.0""#),
        "NeoForge installer should be used as metadata, not added to the launch classpath\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
