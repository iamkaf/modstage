use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

mod support;

use support::{file_url, file_url_path};
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

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

fn write_pack(path: &Path, index: &str, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("failed to create test mrpack");
    let mut archive = ZipWriter::new(file);
    let options = SimpleFileOptions::default();
    archive
        .start_file("modrinth.index.json", options)
        .expect("failed to start pack index");
    archive
        .write_all(index.as_bytes())
        .expect("failed to write pack index");
    for (name, contents) in entries {
        archive
            .start_file(*name, options)
            .expect("failed to start pack override");
        archive
            .write_all(contents)
            .expect("failed to write pack override");
    }
    archive.finish().expect("failed to finish test mrpack");
}

#[test]
fn modrinth_pack_resolve_and_stage_preserve_sides_overrides_and_locked_restoration() {
    let project = temp_dir("modrinth-pack-project");
    let metadata = temp_dir("modrinth-pack-metadata");
    let data_home = temp_dir("modrinth-pack-data");
    let cache_home = temp_dir("modrinth-pack-cache");
    let common = metadata.join("common.jar");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    fs::write(&common, b"common").expect("failed to write common mod");
    fs::write(&client, b"client").expect("failed to write client mod");
    fs::write(&server, b"server").expect("failed to write server mod");
    let pack = metadata.join("sample-pack.mrpack");
    let index = format!(
        r#"{{
  "formatVersion": 1,
  "game": "minecraft",
  "versionId": "pack-index-1",
  "name": "Sample Pack",
  "summary": "test",
  "files": [
    {{
      "path": "mods/common.jar",
      "hashes": {{"sha1": "common"}},
      "env": {{"client": "required", "server": "required"}},
      "downloads": ["file://{}"],
      "fileSize": 6
    }},
    {{
      "path": "mods/client.jar",
      "hashes": {{"sha1": "client"}},
      "env": {{"client": "required", "server": "unsupported"}},
      "downloads": ["file://{}"],
      "fileSize": 6
    }},
    {{
      "path": "mods/server.jar",
      "hashes": {{"sha1": "server"}},
      "env": {{"client": "unsupported", "server": "required"}},
      "downloads": ["file://{}"],
      "fileSize": 6
    }}
  ],
  "dependencies": {{"minecraft": "26.1.2"}}
}}"#,
        file_url_path(&common),
        file_url_path(&client),
        file_url_path(&server)
    );
    write_pack(
        &pack,
        &index,
        &[
            ("overrides/config/common.toml", b"common=true"),
            ("client-overrides/options.txt", b"client-option"),
            ("server-overrides/server.properties", b"server-option"),
        ],
    );
    let versions = metadata.join("sample-pack-versions.json");
    fs::write(
        &versions,
        format!(
            r#"[{{
  "id": "sample-pack-version",
  "version_number": "1.0.0",
  "files": [{{
    "primary": true,
    "filename": "sample-pack.mrpack",
    "url": "file://{}",
    "hashes": {{"sha1": "pack"}}
  }}]
}}]"#,
            file_url_path(&pack)
        ),
    )
    .expect("failed to write pack metadata");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "modrinth-pack-test"

[[instance]]
name = "pack-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
modrinth_pack = "modrinth:sample-pack:1.0.0"
"#,
    )
    .expect("failed to write config");
    let versions_url = file_url(&versions);
    let envs = [
        (
            "MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL",
            versions_url.as_str(),
        ),
        (
            "MODSTAGE_DATA_HOME",
            data_home.to_str().expect("data path is not UTF-8"),
        ),
        (
            "MODSTAGE_CACHE_HOME",
            cache_home.to_str().expect("cache path is not UTF-8"),
        ),
    ];

    let resolve = run_in_with_env(&["resolve", "pack-26.1.2"], &project, &envs);
    assert!(
        resolve.status.success(),
        "pack resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    let lock_path = state_lock_path(&data_home, &project, "modrinth-pack-test", "pack-26.1.2");
    let lock = fs::read_to_string(&lock_path).expect("pack lock should exist");
    for expected in [
        "[pack]",
        r#"source = "modrinth:sample-pack:1.0.0""#,
        r#"version_id = "sample-pack-version""#,
        r#"index_version = "pack-index-1""#,
        "[[pack_file]]",
        r#"destination = "config/common.toml""#,
        r#"archive_entry = "overrides/config/common.toml""#,
    ] {
        assert!(
            lock.contains(expected),
            "lock should contain {expected:?}\n{lock}"
        );
    }

    let _ = run_in_with_env(
        &["run", "client", "pack-26.1.2", "--locked"],
        &project,
        &envs,
    );
    let _ = run_in_with_env(
        &["run", "server", "pack-26.1.2", "--locked"],
        &project,
        &envs,
    );
    let instance_root = lock_path.parent().expect("lock should have a parent");
    let client_game = instance_root.join("client/game");
    let server_game = instance_root.join("server/game");
    assert!(client_game.join("mods/common.jar").is_file());
    assert!(client_game.join("mods/client.jar").is_file());
    assert!(!client_game.join("mods/server.jar").exists());
    assert!(server_game.join("mods/common.jar").is_file());
    assert!(!server_game.join("mods/client.jar").exists());
    assert!(server_game.join("mods/server.jar").is_file());
    assert_eq!(
        fs::read_to_string(client_game.join("config/common.toml")).unwrap(),
        "common=true"
    );
    assert_eq!(
        fs::read_to_string(client_game.join("options.txt")).unwrap(),
        "client-option"
    );
    assert_eq!(
        fs::read_to_string(server_game.join("server.properties")).unwrap(),
        "server-option"
    );

    fs::remove_dir_all(cache_home.join("modstage")).expect("failed to clear pack cache");
    fs::remove_file(client_game.join("config/common.toml"))
        .expect("failed to remove staged override");
    let _ = run_in_with_env(
        &["run", "client", "pack-26.1.2", "--locked"],
        &project,
        &envs,
    );
    assert_eq!(
        fs::read_to_string(client_game.join("config/common.toml")).unwrap(),
        "common=true",
        "locked staging should restore an embedded override from the pack archive"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
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
            file_url_path(&jar)
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

    let versions_url = file_url(&versions);
    let output = run_in_with_env(
        &["resolve", "modrinth-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        "modrinth-test",
        "modrinth-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        "[[mod]]",
        r#"source = "modrinth:sample-mod""#,
        r#"provider = "modrinth""#,
        r#"project = "sample-mod""#,
        r#"version_id = "sample-version""#,
        r#"version_number = "1.0.0""#,
        r#"filename = "sample-mod-1.0.0.jar""#,
        &format!(r#"url = "{}""#, file_url(&jar)),
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
            file_url_path(&old_jar),
            file_url_path(&pinned_jar)
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

    let versions_url = file_url(&versions);
    let output = run_in_with_env(
        &["resolve", "modrinth-pinned-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        "modrinth-pinned-test",
        "modrinth-pinned-26.1.2",
    ))
    .expect("modstage.lock should exist");

    for expected in [
        r#"source = "modrinth:sample-mod:2.0.0""#,
        r#"project = "sample-mod""#,
        r#"version_id = "pinned-version""#,
        r#"version_number = "2.0.0""#,
        r#"filename = "sample-mod-2.0.0.jar""#,
        &format!(r#"url = "{}""#, file_url(&pinned_jar)),
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
            file_url_path(&latest_jar),
            file_url_path(&older_jar)
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

    let versions_url = file_url(&versions);
    let output = run_in_with_env(
        &["resolve", "modrinth-latest-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
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
        "modrinth-latest-test",
        "modrinth-latest-26.1.2",
    ))
    .expect("modstage.lock should exist");
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
            file_url_path(&jar)
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

    let versions_url = file_url(&versions);
    let envs = [
        (
            "MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL",
            versions_url.as_str(),
        ),
        (
            "MODSTAGE_DATA_HOME",
            data_home.to_str().expect("data path is not UTF-8"),
        ),
        (
            "MODSTAGE_CACHE_HOME",
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

    let lock = fs::read_to_string(state_lock_path(
        &data_home,
        &project,
        "modrinth-cache-test",
        "modrinth-cache-26.1.2",
    ))
    .expect("modstage.lock should exist");
    assert!(
        lock.contains(r#"version_id = "sample-version""#),
        "lockfile should be resolved from cached metadata\n{lock}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
