use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{file_url, file_url_path};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
    let root = std::env::temp_dir().join(format!("modstage-{name}-{}-{nanos}", std::process::id()));

    fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}

fn spawn_mojang_http_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("failed to bind test HTTP server");
    let address = listener
        .local_addr()
        .expect("failed to read test HTTP server address");
    let base = format!("http://{address}");
    let server_base = base.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut request = [0_u8; 1024];
            let Ok(count) = stream.read(&mut request) else {
                continue;
            };
            let request = String::from_utf8_lossy(&request[..count]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let body = match path {
                "/version_manifest.json" => Some(format!(
                    r#"{{"versions":[{{"id":"26.1.2","url":"{server_base}/version.json"}}]}}"#
                )),
                "/version.json" => Some(format!(
                    r#"{{
  "id": "26.1.2",
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "{server_base}/client.jar" }},
    "server": {{ "url": "{server_base}/server.jar" }}
  }}
}}"#
                )),
                "/client.jar" => Some("client".to_string()),
                "/server.jar" => Some("server".to_string()),
                _ => None,
            };
            match body {
                Some(body) => {
                    let body = body.as_bytes();
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let body = b"not found";
                    let header = format!(
                        "HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(header.as_bytes());
                    let _ = stream.write_all(body);
                }
            }
        }
    });

    base
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
fn resolve_fetches_and_records_mojang_version_metadata() {
    let project = temp_dir("mojang-project");
    let metadata = temp_dir("mojang-metadata");
    let data_home = temp_dir("mojang-data");
    let cache_home = temp_dir("mojang-cache");
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

    let manifest_url = file_url(&manifest);
    let output = run_in_with_env(
        &["resolve", "vanilla-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
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
        "mojang-test",
        "vanilla-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        "[minecraft]",
        r#"version = "26.1.2""#,
        &format!(r#"manifest_url = "{manifest_url}""#),
        &format!(r#"version_url = "{}""#, file_url(&version_json)),
        "manifest_sha256 = ",
        "version_sha256 = ",
        r#"java_major = 25"#,
        &format!(r#"client_url = "{}""#, file_url(&client)),
        r#"client_sha256 = "948fe603f61dc036b5c596dc09fe3ce3f3d30dc90f024c85f3c82db2ccab679d""#,
        &format!(r#"server_url = "{}""#, file_url(&server)),
        r#"server_sha256 = "b3eacd33433b31b5252351032c9b3e7a2e7aa7738d5decdf0dd6c62680853c06""#,
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
fn resolve_uses_internal_http_downloader_without_curl() {
    let project = temp_dir("mojang-http-project");
    let data_home = temp_dir("mojang-http-data");
    let cache_home = temp_dir("mojang-http-cache");
    let empty_path = temp_dir("mojang-empty-path");
    let base = spawn_mojang_http_server();
    let manifest = format!("{base}/version_manifest.json");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "mojang-http"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let output = run_in_with_env(
        &["resolve", "vanilla-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest),
            ("PATH", empty_path.to_str().expect("path is not UTF-8")),
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
        "resolve should use the internal HTTP client instead of curl\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
    fs::remove_dir_all(empty_path).expect("failed to remove empty PATH");
}

#[test]
fn resolve_finds_mojang_version_entry_when_latest_mentions_version_first() {
    let project = temp_dir("mojang-latest-project");
    let metadata = temp_dir("mojang-latest-metadata");
    let data_home = temp_dir("mojang-latest-data");
    let cache_home = temp_dir("mojang-latest-cache");
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
            r#"{{
  "latest": {{ "release": "26.1.2", "snapshot": "26.2-snapshot-8" }},
  "versions": [
    {{ "id": "26.2-snapshot-8", "url": "file:///not-used.json" }},
    {{ "id": "26.1.2", "url": "file://{}" }}
  ]
}}"#,
            file_url_path(&version_json)
        ),
    )
    .expect("failed to write manifest");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "mojang-latest-test"

[[instance]]
name = "vanilla-latest-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = file_url(&manifest);
    let output = run_in_with_env(
        &["resolve", "vanilla-latest-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
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
        "resolve should use the version entry, not latest.release\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "mojang-latest-test",
        "vanilla-latest-26.1.2",
    ))
    .expect("modstage.lock should exist");
    assert!(
        lock.contains("[minecraft]")
            && lock.contains(&format!(r#"version_url = "{}""#, file_url(&version_json))),
        "lockfile should include Minecraft metadata from the matching version entry\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn resolve_uses_the_default_mojang_manifest_when_no_override_is_set() {
    let project = temp_dir("mojang-default-project");
    let metadata = temp_dir("mojang-default-metadata");
    let data_home = temp_dir("mojang-default-data");
    let cache_home = temp_dir("mojang-default-cache");
    let fake_bin = temp_dir("mojang-default-bin");
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
    let curl = fake_bin.join("curl");
    fs::write(
        &curl,
        format!(
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://piston-meta.mojang.com/mc/game/version_manifest_v2.json) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
            metadata.display(),
            manifest.display()
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
name = "mojang-default-test"

[[instance]]
name = "vanilla-default-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = run_in_with_env(
        &["resolve", "vanilla-default-26.1.2"],
        &project,
        &[
            ("PATH", &path),
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
        "resolve should use the default Mojang manifest\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "mojang-default-test",
        "vanilla-default-26.1.2",
    ))
    .expect("modstage.lock should exist");
    assert!(
        lock.contains(
            r#"manifest_url = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json""#
        ) && lock.contains(r#"version = "26.1.2""#)
            && lock.contains(r#"java_major = 25"#),
        "lockfile should record metadata fetched through the default manifest\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
    fs::remove_dir_all(fake_bin).expect("failed to remove fake bin");
}

#[test]
fn resolve_records_mojang_main_class_and_libraries() {
    let project = temp_dir("mojang-launch-project");
    let metadata = temp_dir("mojang-launch-metadata");
    let data_home = temp_dir("mojang-launch-data");
    let cache_home = temp_dir("mojang-launch-cache");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let library = metadata.join("example-lib-1.0.0.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&library, b"library").expect("failed to write library jar");
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "mainClass": "net.minecraft.client.main.Main",
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "file://{}" }},
    "server": {{ "url": "file://{}" }}
  }},
  "libraries": [{{
    "name": "com.example:example-lib:1.0.0",
    "rules": [{{ "action": "allow", "os": {{ "name": "osx" }} }}],
    "downloads": {{
      "artifact": {{
        "path": "com/example/example-lib/1.0.0/example-lib-1.0.0.jar",
        "url": "file://{}"
      }}
    }}
  }}]
}}"#,
            file_url_path(&client),
            file_url_path(&server),
            file_url_path(&library)
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
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "mojang-launch-test"

[[instance]]
name = "vanilla-launch-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = file_url(&manifest);
    let output = run_in_with_env(
        &["resolve", "vanilla-launch-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
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
        "mojang-launch-test",
        "vanilla-launch-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        "[launch]",
        r#"main_class = "net.minecraft.client.main.Main""#,
        "[[library]]",
        r#"name = "com.example:example-lib:1.0.0""#,
        r#"path = "com/example/example-lib/1.0.0/example-lib-1.0.0.jar""#,
        &format!(r#"url = "{}""#, file_url(&library)),
        r#"sha256 = "b718f1354f7247312eca086d9a024afe5fa717ddea5adeddd6f12bcf945b2e8c""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains(r#"name = "osx""#),
        "library parser must not treat rule OS names as library coordinates\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn resolve_downloads_mojang_asset_objects_without_locking_them() {
    let project = temp_dir("mojang-assets-project");
    let metadata = temp_dir("mojang-assets-metadata");
    let data_home = temp_dir("mojang-assets-data");
    let cache_home = temp_dir("mojang-assets-cache");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    let asset_bytes = b"hello";
    let asset_hash = "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d";
    let asset_object = metadata.join("example.ogg");
    fs::write(&asset_object, asset_bytes).expect("failed to write asset object");
    let assets_json = metadata.join("assets-26.json");
    fs::write(
        &assets_json,
        format!(
            r#"{{
  "objects": {{
    "minecraft/sounds/example.ogg": {{
      "hash": "{asset_hash}",
      "size": 5,
      "url": "file://{}"
    }}
  }}
}}"#,
            file_url_path(&asset_object)
        ),
    )
    .expect("failed to write asset index json");
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "assetIndex": {{
    "id": "26",
    "url": "file://{}"
  }},
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "file://{}" }},
    "server": {{ "url": "file://{}" }}
  }}
}}"#,
            file_url_path(&assets_json),
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
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "mojang-assets-test"

[[instance]]
name = "vanilla-assets-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = file_url(&manifest);
    let output = run_in_with_env(
        &["resolve", "vanilla-assets-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
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
        "mojang-assets-test",
        "vanilla-assets-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        "[assets]",
        r#"id = "26""#,
        &format!(r#"index_url = "{}""#, file_url(&assets_json)),
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains("[[asset]]")
            && !lock.contains("minecraft/sounds/example.ogg")
            && !lock.contains(asset_hash),
        "lockfile must not record individual Minecraft asset objects\n{lock}"
    );
    let cached_object = cache_home
        .join("modstage")
        .join("downloads")
        .join("mojang")
        .join("assets")
        .join("objects")
        .join(&asset_hash[..2])
        .join(asset_hash);
    assert_eq!(
        fs::read(&cached_object).expect("resolve should download the asset object"),
        asset_bytes,
        "cached asset object should match the source bytes"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
