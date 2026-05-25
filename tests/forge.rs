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
fn resolve_records_forge_loader_metadata() {
    let project = temp_dir("forge-project");
    let metadata = temp_dir("forge-metadata");
    let data_home = temp_dir("forge-data");
    let cache_home = temp_dir("forge-cache");
    let forge_metadata = metadata.join("forge-loader.json");
    fs::write(
        &forge_metadata,
        r#"{
  "version": "60.0.0",
  "installer_maven": "net.minecraftforge:forge:26.1.2-60.0.0",
  "client_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher",
  "server_main_class": "cpw.mods.bootstraplauncher.BootstrapLauncher"
}"#,
    )
    .expect("failed to write Forge metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "forge-test"

[[instance]]
name = "forge-26.1.2"
minecraft = "26.1.2"
loader = "forge"
loader_version = "latest"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let forge_url = format!("file://{}", forge_metadata.display());
    let output = run_in_with_env(
        &["resolve", "forge-26.1.2"],
        &project,
        &[
            ("MODSTAGE_FORGE_META_URL", &forge_url),
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
        r#"kind = "forge""#,
        r#"version = "60.0.0""#,
        r#"installer_maven = "net.minecraftforge:forge:26.1.2-60.0.0""#,
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
fn resolve_uses_pinned_forge_loader_version_without_metadata_override() {
    let project = temp_dir("forge-pinned-project");
    let metadata = temp_dir("forge-pinned-metadata");
    let data_home = temp_dir("forge-pinned-data");
    let cache_home = temp_dir("forge-pinned-cache");
    let fake_bin = temp_dir("forge-pinned-bin");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let installer = metadata.join("forge-26.1.2-64.0.4.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&installer, b"installer").expect("failed to write Forge installer jar");
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
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://maven.minecraftforge.net/net/minecraftforge/forge/26.1.2-64.0.4/forge-26.1.2-64.0.4.jar) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
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
name = "forge-pinned-test"

[[instance]]
name = "forge-pinned-26.1.2"
minecraft = "26.1.2"
loader = "forge"
loader_version = "26.1.2-64.0.4"
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
        &["resolve", "forge-pinned-26.1.2"],
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
        "resolve should use pinned Forge metadata and built-in Maven\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let urls = fs::read_to_string(metadata.join("curl-urls.txt"))
        .expect("fake curl should record fetched URLs");
    assert!(
        urls.contains("https://maven.minecraftforge.net/net/minecraftforge/forge/26.1.2-64.0.4/forge-26.1.2-64.0.4.jar"),
        "resolve should fetch the pinned Forge installer from built-in Maven\n{urls}"
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
    for expected in [
        r#"kind = "forge""#,
        r#"version = "26.1.2-64.0.4""#,
        r#"installer_maven = "net.minecraftforge:forge:26.1.2-64.0.4""#,
        r#"client_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
        r#"server_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
        r#"name = "net.minecraftforge:forge:26.1.2-64.0.4""#,
        r#"repository = "forge""#,
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
    fs::remove_dir_all(fake_bin).expect("failed to remove fake bin");
}
