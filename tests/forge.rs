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
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let installer = metadata
        .join("net")
        .join("minecraftforge")
        .join("forge")
        .join("26.1.2-64.0.4")
        .join("forge-26.1.2-64.0.4-installer.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::create_dir_all(
        installer
            .parent()
            .expect("installer jar should have parent"),
    )
    .expect("failed to create Forge maven dir");
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
name = "forge-pinned-test"

[repositories]
forge = "file://{}"

[[instance]]
name = "forge-pinned-26.1.2"
minecraft = "26.1.2"
loader = "forge"
loader_version = "26.1.2-64.0.4"
sides = ["client", "server"]
"#,
            metadata.display()
        ),
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let output = run_in_with_env(
        &["resolve", "forge-pinned-26.1.2"],
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
        "resolve should use pinned Forge metadata and built-in Maven\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
    for expected in [
        r#"kind = "forge""#,
        r#"version = "26.1.2-64.0.4""#,
        r#"installer_maven = "net.minecraftforge:forge:26.1.2-64.0.4:installer""#,
        r#"client_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
        r#"server_main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains(r#"name = "net.minecraftforge:forge:26.1.2-64.0.4:installer""#),
        "Forge installer should be used as metadata, not added to the launch classpath\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn resolve_adds_forge_installer_version_libraries_to_the_launch_classpath() {
    let project = temp_dir("forge-profile-project");
    let metadata = temp_dir("forge-profile-metadata");
    let data_home = temp_dir("forge-profile-data");
    let cache_home = temp_dir("forge-profile-cache");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let installer = metadata
        .join("net")
        .join("minecraftforge")
        .join("forge")
        .join("26.1.2-64.0.4")
        .join("forge-26.1.2-64.0.4-installer.jar");
    let bootstrap = metadata.join("bootstraplauncher-2.0.0.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::create_dir_all(
        installer
            .parent()
            .expect("installer jar should have parent"),
    )
    .expect("failed to create Forge maven dir");
    fs::write(&bootstrap, b"bootstraplauncher").expect("failed to write bootstrap launcher jar");
    let bootstrap_url = format!("file://{}", bootstrap.display());
    let installer_version_json = format!(
        r#"{{
  "mainClass": "net.minecraftforge.bootstrap.ForgeBootstrap",
  "arguments": {{
    "game": [
      "--launchTarget",
      "forge_client"
    ],
    "jvm": [
      "-Dforge.test=true"
    ]
  }},
  "libraries": [
    {{
      "name": "cpw.mods:bootstraplauncher:2.0.0",
      "downloads": {{
        "artifact": {{
          "path": "cpw/mods/bootstraplauncher/2.0.0/bootstraplauncher-2.0.0.jar",
          "url": "{bootstrap_url}"
        }}
      }}
    }},
    {{
      "name": "net.minecraftforge:forge:26.1.2-64.0.4:client",
      "downloads": {{
        "artifact": {{
          "path": "net/minecraftforge/forge/26.1.2-64.0.4/forge-26.1.2-64.0.4-client.jar",
          "url": ""
        }}
      }}
    }}
  ]
}}"#
    );
    write_stored_jar(
        &installer,
        &[("version.json", installer_version_json.as_bytes())],
    );
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
name = "forge-profile-test"

[repositories]
forge = "file://{}"

[[instance]]
name = "forge-profile-26.1.2"
minecraft = "26.1.2"
loader = "forge"
loader_version = "26.1.2-64.0.4"
sides = ["client", "server"]
"#,
            metadata.display()
        ),
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let output = run_in_with_env(
        &["resolve", "forge-profile-26.1.2"],
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
        "resolve should extract Forge installer profile libraries\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
    for expected in [
        r#"client_main_class = "net.minecraftforge.bootstrap.ForgeBootstrap""#,
        r#"server_main_class = "net.minecraftforge.bootstrap.ForgeBootstrap""#,
        r#"kind = "jvm""#,
        r#"arg = "-Dforge.test=true""#,
        r#"kind = "game""#,
        r#"arg = "--launchTarget""#,
        r#"arg = "forge_client""#,
        r#"name = "cpw.mods:bootstraplauncher:2.0.0""#,
        &format!(r#"url = "{bootstrap_url}""#),
        r#"sha256 = "603eb608091a4fb09e0c529d8eaa13fcc9c12b21dc8abd170f7f030217c7c729""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains(r#"name = "net.minecraftforge:forge:26.1.2-64.0.4:client""#),
        "generated Forge profile libraries with empty URLs should not be locked\n{lock}"
    );
    assert!(
        !lock.contains(r#"name = "net.minecraftforge:forge:26.1.2-64.0.4:installer""#),
        "Forge installer should be used as metadata, not added to the launch classpath\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
