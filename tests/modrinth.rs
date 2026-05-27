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
  "loaders": ["vanilla"],
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
loader = "vanilla"
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

#[test]
fn resolve_honors_a_pinned_modrinth_version_source() {
    let project = temp_dir("modrinth-pinned-project");
    let metadata = temp_dir("modrinth-pinned-metadata");
    let data_home = temp_dir("modrinth-pinned-data");
    let cache_home = temp_dir("modrinth-pinned-cache");
    let old_jar = metadata.join("sample-mod-1.0.0.jar");
    let pinned_jar = metadata.join("sample-mod-2.0.0.jar");
    fs::write(&old_jar, b"old").expect("failed to write old Modrinth jar");
    fs::write(&pinned_jar, b"abc").expect("failed to write pinned Modrinth jar");
    let versions = metadata.join("sample-mod-versions.json");
    fs::write(
        &versions,
        format!(
            r#"[{{
  "id": "old-version",
  "project_id": "sample-project",
  "version_number": "1.0.0",
  "game_versions": ["26.1.2"],
  "loaders": ["vanilla"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-1.0.0.jar",
    "url": "file://{}",
    "hashes": {{"sha1": "c00dbbc9dadfbe1e232e93a729dd4752fade0abf"}}
  }}]
}}, {{
  "id": "pinned-version",
  "project_id": "sample-project",
  "version_number": "2.0.0",
  "game_versions": ["26.1.2"],
  "loaders": ["vanilla"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-2.0.0.jar",
    "url": "file://{}",
    "hashes": {{"sha1": "a9993e364706816aba3e25717850c26c9cd0d89d"}}
  }}]
}}]"#,
            old_jar.display(),
            pinned_jar.display()
        ),
    )
    .expect("failed to write Modrinth versions metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "modrinth-pinned-test"

[[instance]]
name = "modrinth-pinned-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
mods = [
  "modrinth:sample-mod:2.0.0",
]
"#,
    )
    .expect("failed to write config");

    let versions_url = format!("file://{}", versions.display());
    let output = run_in_with_env(
        &["resolve", "modrinth-pinned-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        r#"source = "modrinth:sample-mod:2.0.0""#,
        r#"project = "sample-mod""#,
        r#"version_id = "pinned-version""#,
        r#"version_number = "2.0.0""#,
        r#"filename = "sample-mod-2.0.0.jar""#,
        &format!(r#"url = "file://{}""#, pinned_jar.display()),
        r#"sha1 = "a9993e364706816aba3e25717850c26c9cd0d89d""#,
        r#"sha256 = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains("old-version") && !lock.contains("sample-mod-1.0.0.jar"),
        "pinned Modrinth source should not resolve the first available version\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn resolve_treats_explicit_modrinth_latest_as_the_first_compatible_version() {
    let project = temp_dir("modrinth-latest-project");
    let metadata = temp_dir("modrinth-latest-metadata");
    let data_home = temp_dir("modrinth-latest-data");
    let cache_home = temp_dir("modrinth-latest-cache");
    let latest_jar = metadata.join("sample-mod-2.0.0.jar");
    let older_jar = metadata.join("sample-mod-1.0.0.jar");
    fs::write(&latest_jar, b"abc").expect("failed to write latest Modrinth jar");
    fs::write(&older_jar, b"old").expect("failed to write older Modrinth jar");
    let versions = metadata.join("sample-mod-versions.json");
    fs::write(
        &versions,
        format!(
            r#"[{{
  "id": "latest-version",
  "project_id": "sample-project",
  "version_number": "2.0.0",
  "game_versions": ["26.1.2"],
  "loaders": ["vanilla"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-2.0.0.jar",
    "url": "file://{}",
    "hashes": {{"sha1": "a9993e364706816aba3e25717850c26c9cd0d89d"}}
  }}]
}}, {{
  "id": "older-version",
  "project_id": "sample-project",
  "version_number": "1.0.0",
  "game_versions": ["26.1.2"],
  "loaders": ["vanilla"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-1.0.0.jar",
    "url": "file://{}",
    "hashes": {{"sha1": "c00dbbc9dadfbe1e232e93a729dd4752fade0abf"}}
  }}]
}}]"#,
            latest_jar.display(),
            older_jar.display()
        ),
    )
    .expect("failed to write Modrinth versions metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "modrinth-latest-test"

[[instance]]
name = "modrinth-latest-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
mods = [
  "modrinth:sample-mod:latest",
]
"#,
    )
    .expect("failed to write config");

    let versions_url = format!("file://{}", versions.display());
    let output = run_in_with_env(
        &["resolve", "modrinth-latest-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        r#"source = "modrinth:sample-mod:latest""#,
        r#"version_id = "latest-version""#,
        r#"version_number = "2.0.0""#,
        r#"filename = "sample-mod-2.0.0.jar""#,
    ] {
        assert!(
            lock.contains(expected),
            "lockfile should contain {expected:?}\n{lock}"
        );
    }
    assert!(
        !lock.contains("older-version") && !lock.contains("sample-mod-1.0.0.jar"),
        "explicit latest should resolve the first compatible Modrinth version\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn resolve_reuses_cached_modrinth_version_metadata() {
    let project = temp_dir("modrinth-cache-project");
    let metadata = temp_dir("modrinth-cache-metadata");
    let data_home = temp_dir("modrinth-cache-data");
    let cache_home = temp_dir("modrinth-cache-cache");
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
  "loaders": ["vanilla"],
  "files": [{{
    "primary": true,
    "filename": "sample-mod-1.0.0.jar",
    "url": "file://{}",
    "hashes": {{"sha1": "a9993e364706816aba3e25717850c26c9cd0d89d"}}
  }}]
}}]"#,
            jar.display()
        ),
    )
    .expect("failed to write Modrinth versions metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "modrinth-cache-test"

[[instance]]
name = "modrinth-cache-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
mods = [
  "modrinth:sample-mod",
]
"#,
    )
    .expect("failed to write config");

    let versions_url = format!("file://{}", versions.display());
    let envs = [
        (
            "MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL",
            versions_url.as_str(),
        ),
        (
            "XDG_DATA_HOME",
            data_home.to_str().expect("data path is not UTF-8"),
        ),
        (
            "XDG_CACHE_HOME",
            cache_home.to_str().expect("cache path is not UTF-8"),
        ),
    ];

    let first = run_in_with_env(&["resolve", "modrinth-cache-26.1.2"], &project, &envs);
    assert!(
        first.status.success(),
        "first resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&first.stdout),
        String::from_utf8_lossy(&first.stderr)
    );
    fs::remove_file(&versions).expect("failed to remove source metadata");

    let second = run_in_with_env(&["resolve", "modrinth-cache-26.1.2"], &project, &envs);
    assert!(
        second.status.success(),
        "second resolve should reuse cached metadata\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&second.stdout),
        String::from_utf8_lossy(&second.stderr)
    );

    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("modstage.lock should exist");
    assert!(
        lock.contains(r#"version_id = "sample-version""#),
        "lockfile should be resolved from cached metadata\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
