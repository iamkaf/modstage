use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{file_url, file_url_path};

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
            file_url_path(&client),
            file_url_path(&server)
        ),
    )
    .expect("failed to write version json");
    let manifest = metadata.join("version_manifest.json");
    fs::write(
        &manifest,
        format!(
            r#"{{ "versions": [{{ "id": "26.1.2", "url": "file://{}" }}] }}"#,
            file_url_path(&version_json)
        ),
    )
    .expect("failed to write manifest");
    file_url(&manifest)
}

fn state_lock_path(data_home: &Path, root: &Path, project_name: &str, instance: &str) -> PathBuf {
    data_home
        .join("modstage")
        .join("instances")
        .join(format!(
            "{project_name}-{:08x}",
            stable_hash(
                &dunce::canonicalize(root)
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

    let fabric_url = file_url(&fabric_metadata);
    let output = run_in_with_env(
        &["resolve", "fabric-26.1.2"],
        &project,
        &[
            ("MODSTAGE_FABRIC_META_URL", &fabric_url),
            (
                "MODSTAGE_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "MODSTAGE_CACHE_HOME",
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
        "fabric-test",
        "fabric-26.1.2",
    ))
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

#[test]
#[cfg(unix)]
fn resolve_uses_default_fabric_metadata_for_latest_loader() {
    let project = temp_dir("fabric-default-project");
    let metadata = temp_dir("fabric-default-metadata");
    let data_home = temp_dir("fabric-default-data");
    let cache_home = temp_dir("fabric-default-cache");
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
            file_url_path(&client),
            file_url_path(&server)
        ),
    )
    .expect("failed to write version json");
    let manifest = metadata.join("version_manifest.json");
    fs::write(
        &manifest,
        format!(
            r#"{{ "versions": [{{ "id": "26.1.2", "url": "file://{}" }}] }}"#,
            file_url_path(&version_json)
        ),
    )
    .expect("failed to write manifest");
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
name = "fabric-default-test"

[[instance]]
name = "fabric-default-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = file_url(&manifest);
    let fabric_meta_url = file_url(&fabric_metadata);
    let output = run_in_with_env(
        &["resolve", "fabric-default-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("MODSTAGE_FABRIC_META_URL", &fabric_meta_url),
            (
                "MODSTAGE_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "MODSTAGE_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should use the default Fabric metadata endpoint\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "fabric-default-test",
        "fabric-default-26.1.2",
    ))
    .expect("modstage.lock should exist");
    assert!(
        lock.contains(r#"kind = "fabric""#)
            && lock.contains(r#"version = "0.16.14""#)
            && lock.contains(
                r#"client_main_class = "net.fabricmc.loader.impl.launch.knot.KnotClient""#
            ),
        "lockfile should include Fabric loader metadata\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn resolve_uses_builtin_fabric_maven_repository_for_loader_artifacts() {
    let project = temp_dir("fabric-builtin-repo-project");
    let metadata = temp_dir("fabric-builtin-repo-metadata");
    let data_home = temp_dir("fabric-builtin-repo-data");
    let cache_home = temp_dir("fabric-builtin-repo-cache");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let loader = metadata
        .join("net")
        .join("fabricmc")
        .join("fabric-loader")
        .join("0.16.14")
        .join("fabric-loader-0.16.14.jar");
    let intermediary = metadata
        .join("net")
        .join("fabricmc")
        .join("intermediary")
        .join("26.1.2")
        .join("intermediary-26.1.2.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::create_dir_all(loader.parent().expect("loader jar should have parent"))
        .expect("failed to create loader maven dir");
    fs::create_dir_all(
        intermediary
            .parent()
            .expect("intermediary jar should have parent"),
    )
    .expect("failed to create intermediary maven dir");
    fs::write(&loader, b"loader").expect("failed to write loader jar");
    fs::write(&intermediary, b"intermediary").expect("failed to write intermediary jar");
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
            file_url_path(&client),
            file_url_path(&server)
        ),
    )
    .expect("failed to write version json");
    let manifest = metadata.join("version_manifest.json");
    fs::write(
        &manifest,
        format!(
            r#"{{ "versions": [{{ "id": "26.1.2", "url": "file://{}" }}] }}"#,
            file_url_path(&version_json)
        ),
    )
    .expect("failed to write manifest");
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
        format!(
            r#"[project]
name = "fabric-builtin-repo-test"

[repositories]
fabric = "file://{}"

[[instance]]
name = "fabric-builtin-repo-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client", "server"]
"#,
            file_url_path(&metadata)
        ),
    )
    .expect("failed to write config");

    let manifest_url = file_url(&manifest);
    let fabric_meta_url = file_url(&fabric_metadata);
    let output = run_in_with_env(
        &["resolve", "fabric-builtin-repo-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("MODSTAGE_FABRIC_META_URL", &fabric_meta_url),
            (
                "MODSTAGE_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "MODSTAGE_CACHE_HOME",
                cache_home.to_str().expect("cache path is not UTF-8"),
            ),
        ],
    );

    assert!(
        output.status.success(),
        "resolve should use built-in Fabric Maven repository\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "fabric-builtin-repo-test",
        "fabric-builtin-repo-26.1.2",
    ))
    .expect("modstage.lock should exist");
    for expected in [
        r#"name = "net.fabricmc:fabric-loader:0.16.14""#,
        r#"name = "net.fabricmc:intermediary:26.1.2""#,
        r#"repository = "fabric""#,
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
fn resolve_adds_fabric_loader_artifacts_to_the_launch_classpath() {
    let project = temp_dir("fabric-classpath-project");
    let metadata = temp_dir("fabric-classpath-metadata");
    let data_home = temp_dir("fabric-classpath-data");
    let cache_home = temp_dir("fabric-classpath-cache");
    let repo = metadata.join("repo");
    let loader_dir = repo
        .join("net")
        .join("fabricmc")
        .join("fabric-loader")
        .join("0.16.14");
    let intermediary_dir = repo
        .join("net")
        .join("fabricmc")
        .join("intermediary")
        .join("26.1.2");
    fs::create_dir_all(&loader_dir).expect("failed to create loader artifact dir");
    fs::create_dir_all(&intermediary_dir).expect("failed to create intermediary artifact dir");
    fs::write(loader_dir.join("fabric-loader-0.16.14.jar"), b"loader")
        .expect("failed to write loader jar");
    fs::write(
        intermediary_dir.join("intermediary-26.1.2.jar"),
        b"intermediary",
    )
    .expect("failed to write intermediary jar");
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
        format!(
            r#"[project]
name = "fabric-classpath-test"

[repositories]
fabric = "file://{}"

[[instance]]
name = "fabric-classpath-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client", "server"]
"#,
            file_url_path(&repo)
        ),
    )
    .expect("failed to write config");

    let fabric_url = file_url(&fabric_metadata);
    let output = run_in_with_env(
        &["resolve", "fabric-classpath-26.1.2"],
        &project,
        &[
            ("MODSTAGE_FABRIC_META_URL", &fabric_url),
            (
                "MODSTAGE_DATA_HOME",
                data_home.to_str().expect("data path is not UTF-8"),
            ),
            (
                "MODSTAGE_CACHE_HOME",
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
        "fabric-classpath-test",
        "fabric-classpath-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        "[[library]]",
        r#"name = "net.fabricmc:fabric-loader:0.16.14""#,
        r#"name = "net.fabricmc:intermediary:26.1.2""#,
        r#"repository = "fabric""#,
        r#"sha256 = "d47712cceb4c780603026e6325221c1bcff90679ebc076baa51c71ebe796717c""#,
        r#"sha256 = "37aa37290af965ab652c6843ca9310bba154d0561a763685bbb6cc4063f5d9b2""#,
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

fn write_fabric_loader_metadata(path: &Path, version: &str) {
    fs::write(
        path,
        format!(
            r#"{{
  "loader": {{
    "version": "{version}",
    "maven": "net.fabricmc:fabric-loader:{version}"
  }},
  "intermediary": {{
    "maven": "net.fabricmc:intermediary:26.1.2"
  }},
  "launcherMeta": {{
    "mainClass": {{
      "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
      "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
    }}
  }}
}}"#
        ),
    )
    .expect("failed to write fabric metadata");
}

fn write_fabric_pin_config(project: &Path, version: &str) {
    fs::write(
        project.join("modstage.toml"),
        format!(
            r#"[project]
name = "fabric-pin-cache"

[[instance]]
name = "fabric-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "{version}"
sides = ["client", "server"]
"#
        ),
    )
    .expect("failed to write config");
}

#[test]
fn resolve_uses_fresh_loader_metadata_after_pin_change() {
    let project = temp_dir("fabric-pin-cache-project");
    let metadata = temp_dir("fabric-pin-cache-metadata");
    let data_home = temp_dir("fabric-pin-cache-data");
    let cache_home = temp_dir("fabric-pin-cache-cache");
    let fabric_metadata = metadata.join("fabric-loader.json");
    write_fabric_loader_metadata(&fabric_metadata, "0.18.4");
    write_fabric_pin_config(&project, "0.18.4");

    let fabric_url = file_url(&fabric_metadata);
    let envs = [
        ("MODSTAGE_FABRIC_META_URL", fabric_url.as_str()),
        (
            "MODSTAGE_DATA_HOME",
            data_home.to_str().expect("data path is not UTF-8"),
        ),
        (
            "MODSTAGE_CACHE_HOME",
            cache_home.to_str().expect("cache path is not UTF-8"),
        ),
    ];
    let first = run_in_with_env(&["resolve", "fabric-26.1.2"], &project, &envs);
    assert!(
        first.status.success(),
        "first resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );

    write_fabric_loader_metadata(&fabric_metadata, "0.19.3");
    write_fabric_pin_config(&project, "0.19.3");
    let second = run_in_with_env(&["resolve", "fabric-26.1.2"], &project, &envs);
    assert!(
        second.status.success(),
        "second resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "fabric-pin-cache",
        "fabric-26.1.2",
    ))
    .expect("modstage.lock should exist");
    assert!(
        lock.contains(r#"loader_version = "0.19.3""#)
            && lock.contains(r#"version = "0.19.3""#)
            && lock.contains(r#"loader_maven = "net.fabricmc:fabric-loader:0.19.3""#)
            && !lock.contains("0.18.4"),
        "resolve should not reuse cached metadata from a different loader pin\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
