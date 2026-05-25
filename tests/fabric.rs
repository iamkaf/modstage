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
    let fake_bin = temp_dir("fabric-default-bin");
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
    let curl = fake_bin.join("curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://meta.fabricmc.net/v2/versions/loader/26.1.2) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
            metadata.display(),
            fabric_metadata.display()
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

    let manifest_url = format!("file://{}", manifest.display());
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_in_with_env(
        &["resolve", "fabric-default-26.1.2"],
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
        "resolve should use the default Fabric metadata endpoint\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let urls = fs::read_to_string(metadata.join("curl-urls.txt"))
        .expect("fake curl should record fetched URLs");
    assert!(
        urls.contains("https://meta.fabricmc.net/v2/versions/loader/26.1.2"),
        "resolve should fetch default Fabric metadata URL\n{urls}"
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
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
    fs::remove_dir_all(fake_bin).expect("failed to remove fake bin");
}

#[test]
#[cfg(unix)]
fn resolve_uses_builtin_fabric_maven_repository_for_loader_artifacts() {
    let project = temp_dir("fabric-builtin-repo-project");
    let metadata = temp_dir("fabric-builtin-repo-metadata");
    let data_home = temp_dir("fabric-builtin-repo-data");
    let cache_home = temp_dir("fabric-builtin-repo-cache");
    let fake_bin = temp_dir("fabric-builtin-repo-bin");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let loader = metadata.join("fabric-loader-0.16.14.jar");
    let intermediary = metadata.join("intermediary-26.1.2.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
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
    let curl = fake_bin.join("curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://meta.fabricmc.net/v2/versions/loader/26.1.2) cp {} \"$out\" ;;\n  https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.14/fabric-loader-0.16.14.jar) cp {} \"$out\" ;;\n  https://maven.fabricmc.net/net/fabricmc/intermediary/26.1.2/intermediary-26.1.2.jar) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
            metadata.display(),
            fabric_metadata.display(),
            loader.display(),
            intermediary.display()
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
name = "fabric-builtin-repo-test"

[[instance]]
name = "fabric-builtin-repo-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
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
        &["resolve", "fabric-builtin-repo-26.1.2"],
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
        "resolve should use built-in Fabric Maven repository\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let urls = fs::read_to_string(metadata.join("curl-urls.txt"))
        .expect("fake curl should record fetched URLs");
    for expected in [
        "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.14/fabric-loader-0.16.14.jar",
        "https://maven.fabricmc.net/net/fabricmc/intermediary/26.1.2/intermediary-26.1.2.jar",
    ] {
        assert!(
            urls.contains(expected),
            "resolve should fetch {expected}\n{urls}"
        );
    }

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
    for expected in [
        r#"name = "net.fabricmc:fabric-loader:0.16.14""#,
        r#"name = "net.fabricmc:intermediary:26.1.2""#,
        r#"repository = "fabric""#,
        r#"url = "https://maven.fabricmc.net/net/fabricmc/fabric-loader/0.16.14/fabric-loader-0.16.14.jar""#,
        r#"url = "https://maven.fabricmc.net/net/fabricmc/intermediary/26.1.2/intermediary-26.1.2.jar""#,
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
            repo.display()
        ),
    )
    .expect("failed to write config");

    let fabric_url = format!("file://{}", fabric_metadata.display());
    let output = run_in_with_env(
        &["resolve", "fabric-classpath-26.1.2"],
        &project,
        &[
            ("MODSTAGE_FABRIC_META_URL", &fabric_url),
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
