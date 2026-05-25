use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");

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
#[cfg(unix)]
fn resolve_uses_pinned_neoforge_loader_version_without_metadata_override() {
    let project = temp_dir("neoforge-pinned-project");
    let metadata = temp_dir("neoforge-pinned-metadata");
    let data_home = temp_dir("neoforge-pinned-data");
    let cache_home = temp_dir("neoforge-pinned-cache");
    let fake_bin = temp_dir("neoforge-pinned-bin");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let installer = metadata.join("neoforge-26.1.2.22-beta.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
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
    let curl = fake_bin.join("curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://maven.neoforged.net/releases/net/neoforged/neoforge/26.1.2.22-beta/neoforge-26.1.2.22-beta-installer.jar) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
            metadata.display(),
            installer.display()
        ),
    )
    .expect("failed to write fake curl");
    let mut permissions = fs::metadata(&curl)
        .expect("fake curl metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&curl, permissions).expect("failed to chmod fake curl");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "neoforge-pinned-test"

[[instance]]
name = "neoforge-pinned-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "26.1.2.22-beta"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_in_with_env(
        &["resolve", "neoforge-pinned-26.1.2"],
        &project,
        &[
            ("PATH", &path),
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

    let urls = fs::read_to_string(metadata.join("curl-urls.txt"))
        .expect("fake curl should record fetched URLs");
    assert!(
        urls.contains("https://maven.neoforged.net/releases/net/neoforged/neoforge/26.1.2.22-beta/neoforge-26.1.2.22-beta-installer.jar"),
        "resolve should fetch the pinned NeoForge installer classifier from built-in Maven\n{urls}"
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
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
    fs::remove_dir_all(fake_bin).expect("failed to remove fake bin");
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

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");

    assert!(
        !lock.contains(r#"name = "net.neoforged:neoforge:4.0.0""#),
        "NeoForge installer should be used as metadata, not added to the launch classpath\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
