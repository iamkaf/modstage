use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run_in_with_env(args: &[&str], cwd: &Path, envs: &[(&str, &Path)]) -> Output {
    let mut command = modstage();
    command.args(args).current_dir(cwd);

    for (key, value) in envs {
        command.env(key, value);
    }

    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

fn run_in_with_string_env(args: &[&str], cwd: &Path, envs: &[(&str, &str)]) -> Output {
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

#[test]
fn run_server_reconciles_mods_writes_eula_and_records_report_before_launch() {
    let project = temp_dir("run-stage-project");
    let data_home = temp_dir("run-stage-data");
    let cache_home = temp_dir("run-stage-cache");
    fs::create_dir_all(project.join("mods")).expect("failed to create mods dir");
    fs::write(project.join("mods").join("example.jar"), b"abc").expect("failed to write local jar");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-stage"

[[instance]]
name = "server-stage-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["server"]
mods = [
  "./mods/example.jar",
]
"#,
    )
    .expect("failed to write config");

    let output = run_in_with_env(
        &["run", "server", "server-stage-26.1.2", "--timeout", "0s"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );

    assert!(
        !output.status.success(),
        "run should fail clearly until Minecraft launch is implemented"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("launch is not implemented yet"),
        "unexpected run failure\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );

    let state_root = data_home.join("modstage").join("instances");
    let instance_dir = first_child(&state_root)
        .join("server-stage-26.1.2")
        .join("server")
        .join("game");
    assert!(
        instance_dir.join("mods").join("example.jar").is_file(),
        "run should stage the local mod into the instance mods directory"
    );
    assert_eq!(
        fs::read_to_string(instance_dir.join("eula.txt")).expect("eula.txt should exist"),
        "eula=true\n"
    );
    let launch_metadata = fs::read_to_string(instance_dir.join("modstage-launch.toml"))
        .expect("side-specific launcher metadata should be staged");
    for expected in [
        r#"instance = "server-stage-26.1.2""#,
        r#"side = "server""#,
        r#"loader = "fabric""#,
        r#"minecraft = "26.1.2""#,
        r#""./mods/example.jar""#,
    ] {
        assert!(
            launch_metadata.contains(expected),
            "launcher metadata should contain {expected:?}\n{launch_metadata}"
        );
    }

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let run_id = report_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .expect("run report should have a UTF-8 run id")
        .to_string();
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    assert!(
        report.contains(r#"instance = "server-stage-26.1.2""#)
            && report.contains(r#"side = "server""#)
            && report.contains(r#"status = "staged""#),
        "run report should describe staged run\n{report}"
    );

    let inspect = run_in_with_env(
        &["inspect", "run", &run_id],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        inspect.status.success(),
        "inspect run should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&inspect.stdout),
        String::from_utf8_lossy(&inspect.stderr)
    );
    let inspect_stdout = String::from_utf8_lossy(&inspect.stdout);
    assert!(
        inspect_stdout.contains(r#"instance = "server-stage-26.1.2""#)
            && inspect_stdout.contains(r#"status = "staged""#),
        "inspect run should print the run report\n{inspect_stdout}"
    );

    let inspect_instance = run_in_with_env(
        &[
            "inspect",
            "instance",
            "server-stage-26.1.2",
            "--side",
            "server",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        inspect_instance.status.success(),
        "inspect instance should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&inspect_instance.stdout),
        String::from_utf8_lossy(&inspect_instance.stderr)
    );
    let inspect_instance_stdout = String::from_utf8_lossy(&inspect_instance.stdout);
    for expected in [
        r#"instance = "server-stage-26.1.2""#,
        r#"side = "server""#,
        "game_dir = ",
        "mods_dir = ",
        "eula = true",
        r#""example.jar""#,
    ] {
        assert!(
            inspect_instance_stdout.contains(expected),
            "inspect instance should contain {expected:?}\n{inspect_instance_stdout}"
        );
    }

    let clean = run_in_with_env(
        &[
            "clean",
            "instance",
            "server-stage-26.1.2",
            "--side",
            "server",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        clean.status.success(),
        "clean instance should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&clean.stdout),
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(
        !instance_dir.exists(),
        "clean instance --side server should remove the staged server game directory"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn run_stages_modrinth_mods_from_the_resolved_lockfile() {
    let project = temp_dir("run-stage-modrinth-project");
    let metadata = temp_dir("run-stage-modrinth-metadata");
    let data_home = temp_dir("run-stage-modrinth-data");
    let cache_home = temp_dir("run-stage-modrinth-cache");
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
name = "run-stage-modrinth"

[[instance]]
name = "server-modrinth-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["server"]
mods = [
  "modrinth:sample-mod",
]
"#,
    )
    .expect("failed to write config");

    let versions_url = format!("file://{}", versions.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-modrinth-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MODRINTH_PROJECT_VERSIONS_URL", &versions_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let run = run_in_with_env(
        &["run", "server", "server-modrinth-26.1.2"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "run should fail clearly until Minecraft launch is implemented"
    );

    let state_root = data_home.join("modstage").join("instances");
    let instance_dir = first_child(&state_root)
        .join("server-modrinth-26.1.2")
        .join("server")
        .join("game");
    let staged = instance_dir.join("mods").join("sample-mod-1.0.0.jar");
    assert!(
        staged.is_file(),
        "run should stage the lockfile-resolved Modrinth jar into the instance mods directory"
    );
    assert_eq!(
        fs::read(&staged).expect("staged Modrinth jar should be readable"),
        b"abc"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn run_stages_maven_file_repository_mods_from_the_resolved_lockfile() {
    let project = temp_dir("run-stage-maven-project");
    let data_home = temp_dir("run-stage-maven-data");
    let cache_home = temp_dir("run-stage-maven-cache");
    let repo = project.join("repo");
    let artifact_dir = repo
        .join("com")
        .join("example")
        .join("example-mod")
        .join("1.0.0");
    fs::create_dir_all(&artifact_dir).expect("failed to create Maven artifact dir");
    fs::write(artifact_dir.join("example-mod-1.0.0.jar"), b"abc")
        .expect("failed to write Maven jar");
    fs::write(
        project.join("modstage.toml"),
        format!(
            r#"[project]
name = "run-stage-maven"

[repositories]
local = "file://{}"

[[instance]]
name = "server-maven-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["server"]
mods = [
  "maven:com.example:example-mod:1.0.0",
]
"#,
            repo.display()
        ),
    )
    .expect("failed to write config");

    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-maven-26.1.2"],
        &project,
        &[
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let run = run_in_with_env(
        &["run", "server", "server-maven-26.1.2"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "run should fail clearly until this fixture has a launch graph"
    );

    let state_root = data_home.join("modstage").join("instances");
    let instance_dir = first_child(&state_root)
        .join("server-maven-26.1.2")
        .join("server")
        .join("game");
    let staged = instance_dir.join("mods").join("example-mod-1.0.0.jar");
    assert!(
        staged.is_file(),
        "run should stage the lockfile-resolved Maven jar into the instance mods directory"
    );
    assert_eq!(
        fs::read(&staged).expect("staged Maven jar should be readable"),
        b"abc"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_run_restores_remote_maven_mods_after_cache_deletion() {
    let project = temp_dir("run-remote-maven-project");
    let metadata = temp_dir("run-remote-maven-metadata");
    let data_home = temp_dir("run-remote-maven-data");
    let cache_home = temp_dir("run-remote-maven-cache");
    let fake_bin = temp_dir("run-remote-maven-bin");
    let client = metadata.join("client.jar");
    let server = metadata.join("server.jar");
    let remote_mod = metadata.join("remote-mod-1.0.0.jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&remote_mod, b"remote mod").expect("failed to write remote Maven mod jar");
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
            "#!/bin/sh\nout=''\nurl=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--output' ]; then\n    shift\n    out=\"$1\"\n  else\n    url=\"$1\"\n  fi\n  shift\ndone\nprintf '%s\\n' \"$url\" >> {}/curl-urls.txt\ncase \"$url\" in\n  https://repo.maven.apache.org/maven2/com/example/remote-mod/1.0.0/remote-mod-1.0.0.jar) cp {} \"$out\" ;;\n  *) exit 64 ;;\nesac\n",
            metadata.display(),
            remote_mod.display()
        ),
    )
    .expect("failed to write fake curl");
    let mut permissions = fs::metadata(&curl)
        .expect("fake curl metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&curl, permissions).expect("failed to chmod fake curl");
    let fake_java = metadata.join("fake-java-remote-maven");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'remote Maven mod restored\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-remote-maven"

[[instance]]
name = "remote-maven-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
mods = [
  "maven:com.example:remote-mod:1.0.0",
]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "remote-maven-26.1.2"],
        &project,
        &[
            ("PATH", &path),
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should download the remote Maven mod\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    fs::remove_dir_all(cache_home.join("modstage").join("downloads").join("maven"))
        .expect("failed to remove Maven download cache");
    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_string_env(
        &[
            "run",
            "server",
            "remote-maven-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("PATH", &path),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        run.status.success(),
        "locked run should restore a remote Maven mod from the lockfile URL\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("remote-maven-26.1.2")
        .join("server")
        .join("game");
    let staged = game_dir.join("mods").join("remote-mod-1.0.0.jar");
    assert!(
        staged.is_file(),
        "locked run should stage the restored Maven mod"
    );
    assert_eq!(
        fs::read(&staged).expect("staged remote Maven mod should be readable"),
        b"remote mod"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
    fs::remove_dir_all(fake_bin).expect("failed to remove fake bin");
}

#[test]
fn run_applies_side_fixtures_without_replacing_existing_files_by_default() {
    let project = temp_dir("run-fixture-project");
    let data_home = temp_dir("run-fixture-data");
    let cache_home = temp_dir("run-fixture-cache");
    let fixture = project.join("fixtures").join("server");
    fs::create_dir_all(fixture.join("config")).expect("failed to create fixture dir");
    fs::write(
        fixture.join("config").join("server.properties"),
        b"fixture=true\n",
    )
    .expect("failed to write fixture config");
    fs::write(fixture.join("motd.txt"), b"fixture motd\n").expect("failed to write fixture file");
    fs::write(
        project.join("modstage.lock"),
        "# This file is generated by modstage. Do not edit by hand.\nversion = 1\n",
    )
    .expect("failed to write lockfile");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-fixture"

[[instance]]
name = "server-fixture-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]

[[instance.fixture]]
from = "fixtures/server"
to = "."
side = "server"
"#,
    )
    .expect("failed to write config");

    let first_run = run_in_with_env(
        &["run", "server", "server-fixture-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !first_run.status.success(),
        "fixture run should fail only because this fixture has no launch graph"
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("server-fixture-26.1.2")
        .join("server")
        .join("game");
    assert_eq!(
        fs::read_to_string(game_dir.join("config").join("server.properties"))
            .expect("fixture config should be staged"),
        "fixture=true\n"
    );
    assert_eq!(
        fs::read_to_string(game_dir.join("motd.txt")).expect("fixture file should be staged"),
        "fixture motd\n"
    );

    fs::write(game_dir.join("motd.txt"), b"user motd\n").expect("failed to edit staged fixture");
    let second_run = run_in_with_env(
        &["run", "server", "server-fixture-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !second_run.status.success(),
        "second fixture run should fail only because this fixture has no launch graph"
    );
    assert_eq!(
        fs::read_to_string(game_dir.join("motd.txt")).expect("fixture file should still exist"),
        "user motd\n",
        "fixtures should respect existing files unless replace = true"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn run_applies_fixture_replace_true_without_deleting_unrelated_files() {
    let project = temp_dir("run-fixture-replace-project");
    let data_home = temp_dir("run-fixture-replace-data");
    let cache_home = temp_dir("run-fixture-replace-cache");
    let fixture = project.join("fixtures").join("server");
    fs::create_dir_all(fixture.join("config")).expect("failed to create fixture dir");
    fs::write(
        fixture.join("config").join("server.properties"),
        b"fixture=true\n",
    )
    .expect("failed to write fixture config");
    fs::write(
        project.join("modstage.lock"),
        "# This file is generated by modstage. Do not edit by hand.\nversion = 1\n",
    )
    .expect("failed to write lockfile");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-fixture-replace"

[[instance]]
name = "server-fixture-replace-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]

[[instance.fixture]]
from = "fixtures/server"
to = "."
side = "server"
replace = true
"#,
    )
    .expect("failed to write config");

    let first_run = run_in_with_env(
        &["run", "server", "server-fixture-replace-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !first_run.status.success(),
        "fixture run should fail only because this fixture has no launch graph"
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("server-fixture-replace-26.1.2")
        .join("server")
        .join("game");
    fs::write(
        game_dir.join("config").join("server.properties"),
        b"user edit\n",
    )
    .expect("failed to edit staged fixture");
    fs::write(game_dir.join("unrelated.txt"), b"keep me\n")
        .expect("failed to write unrelated file");

    let second_run = run_in_with_env(
        &["run", "server", "server-fixture-replace-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !second_run.status.success(),
        "second fixture run should fail only because this fixture has no launch graph"
    );
    assert_eq!(
        fs::read_to_string(game_dir.join("config").join("server.properties"))
            .expect("fixture-owned file should exist"),
        "fixture=true\n",
        "replace = true should overwrite files present in the fixture"
    );
    assert_eq!(
        fs::read_to_string(game_dir.join("unrelated.txt"))
            .expect("unrelated file should still exist"),
        "keep me\n",
        "replace = true should not delete unrelated destination files"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn run_locked_requires_an_existing_lockfile_before_staging() {
    let project = temp_dir("run-locked-project");
    let data_home = temp_dir("run-locked-data");
    let cache_home = temp_dir("run-locked-cache");
    fs::create_dir_all(project.join("mods")).expect("failed to create mods dir");
    fs::write(project.join("mods").join("example.jar"), b"abc").expect("failed to write local jar");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-locked"

[[instance]]
name = "locked-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["server"]
mods = [
  "./mods/example.jar",
]
"#,
    )
    .expect("failed to write config");

    let output = run_in_with_env(
        &["run", "server", "locked-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );

    assert!(
        !output.status.success(),
        "locked run should fail when no lockfile exists"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("locked run requires modstage.lock"),
        "locked run should explain the missing lockfile\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr
    );
    assert!(
        !data_home.join("modstage").join("instances").exists(),
        "locked run without a lockfile must not stage instance state"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn locked_run_rejects_mods_that_no_longer_match_the_lockfile_hash() {
    let project = temp_dir("run-locked-hash-project");
    let data_home = temp_dir("run-locked-hash-data");
    let cache_home = temp_dir("run-locked-hash-cache");
    fs::create_dir_all(project.join("mods")).expect("failed to create mods dir");
    fs::write(project.join("mods").join("example.jar"), b"abc").expect("failed to write local jar");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-locked-hash"

[[instance]]
name = "locked-hash-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["server"]
mods = [
  "./mods/example.jar",
]
"#,
    )
    .expect("failed to write config");

    let resolve = run_in_with_env(
        &["resolve", "locked-hash-26.1.2"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    fs::write(project.join("mods").join("example.jar"), b"changed")
        .expect("failed to mutate local jar after resolve");

    let run = run_in_with_env(
        &["run", "server", "locked-hash-26.1.2", "--locked"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "locked run should fail when a mod hash no longer matches"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("hash mismatch") && stderr.contains("./mods/example.jar"),
        "locked run should explain the mismatched mod hash\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        stderr
    );
    assert!(
        !data_home.join("modstage").join("instances").exists(),
        "locked hash mismatch must not stage instance state"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_run_rejects_minecraft_artifacts_that_no_longer_match_the_lockfile_hash() {
    let project = temp_dir("run-locked-artifact-project");
    let metadata = temp_dir("run-locked-artifact-metadata");
    let data_home = temp_dir("run-locked-artifact-data");
    let cache_home = temp_dir("run-locked-artifact-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-locked-artifact");
    fs::write(&fake_java, "#!/bin/sh\nprintf 'should not launch\\n'\n")
        .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-locked-artifact"

[[instance]]
name = "locked-artifact-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "locked-artifact-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );
    fs::write(&server, b"changed-server").expect("failed to mutate server jar after resolve");

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "locked-artifact-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "locked run should fail when the server artifact hash no longer matches"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("server artifact hash mismatch"),
        "locked run should explain the mismatched server artifact hash\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        stderr
    );
    assert!(
        !String::from_utf8_lossy(&run.stdout).contains("should not launch"),
        "locked artifact mismatch must not execute Java"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_executes_resolved_minecraft_artifact_with_configured_java() {
    let project = temp_dir("run-exec-project");
    let metadata = temp_dir("run-exec-metadata");
    let data_home = temp_dir("run-exec-data");
    let cache_home = temp_dir("run-exec-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf 'fake java stdout\\n'\nprintf 'fake java stderr\\n' >&2\nprintf '%s\\n' \"$@\" > java-args.txt\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-exec"

[[instance]]
name = "server-exec-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-exec-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "server-exec-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should execute the configured Java command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("fake java stdout"),
        "run should stream process stdout"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("fake java stderr"),
        "run should stream process stderr"
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("server-exec-26.1.2")
        .join("server")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record its launch args");
    assert!(
        java_args.contains("-jar")
            && java_args.contains("server.jar")
            && java_args.contains("nogui"),
        "server launch should execute java -jar <server.jar> nogui\n{java_args}"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report_dir = report_path
        .parent()
        .expect("run report should have a parent");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    for expected in [
        r#"instance = "server-exec-26.1.2""#,
        r#"side = "server""#,
        r#"status = "passed""#,
        "launch_plan = ",
        "exit_code = 0",
        "timed_out = false",
    ] {
        assert!(
            report.contains(expected),
            "run report should contain {expected:?}\n{report}"
        );
    }
    let launch_plan = fs::read_to_string(report_dir.join("launch-plan.toml"))
        .expect("launch plan snapshot should exist");
    for expected in [
        r#"instance = "server-exec-26.1.2""#,
        r#"side = "server""#,
        "java = ",
        "artifact = ",
        r#"arg = "-jar""#,
        r#"arg = "nogui""#,
    ] {
        assert!(
            launch_plan.contains(expected),
            "launch plan should contain {expected:?}\n{launch_plan}"
        );
    }
    assert_eq!(
        fs::read_to_string(report_dir.join("stdout.log")).expect("stdout log should exist"),
        "fake java stdout\n"
    );
    assert_eq!(
        fs::read_to_string(report_dir.join("stderr.log")).expect("stderr log should exist"),
        "fake java stderr\n"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_stages_scenario_and_exposes_it_to_the_process() {
    let project = temp_dir("run-scenario-project");
    let metadata = temp_dir("run-scenario-metadata");
    let data_home = temp_dir("run-scenario-data");
    let cache_home = temp_dir("run-scenario-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let scenario = project.join("scenario.toml");
    fs::write(&scenario, "name = \"smoke\"\n").expect("failed to write scenario");
    let fake_java = metadata.join("fake-java-scenario");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'MODSTAGE_RUN_DIR=%s\\nMODSTAGE_ARTIFACT_DIR=%s\\n' \"$MODSTAGE_RUN_DIR\" \"$MODSTAGE_ARTIFACT_DIR\" > java-env.txt\nprintf 'scenario stdout\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-scenario"

[[instance]]
name = "server-scenario-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-scenario-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let scenario_str = scenario.to_str().expect("scenario path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "server-scenario-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--scenario",
            scenario_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should stage the scenario and launch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("server-scenario-26.1.2")
        .join("server")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record scenario launch args");
    assert!(
        java_args.contains("--modstageScenario") && java_args.contains("scenario.toml"),
        "scenario path should be passed as a stable launch arg\n{java_args}"
    );
    let java_env = fs::read_to_string(game_dir.join("java-env.txt"))
        .expect("fake java should record scenario environment");
    assert!(
        java_env.contains("MODSTAGE_RUN_DIR=") && java_env.contains("MODSTAGE_ARTIFACT_DIR="),
        "scenario run should expose Modstage run directories\n{java_env}"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report_dir = report_path
        .parent()
        .expect("run report should have a parent");
    assert_eq!(
        fs::read_to_string(report_dir.join("scenario.toml"))
            .expect("scenario should be copied into the run record"),
        "name = \"smoke\"\n"
    );
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    assert!(
        report.contains("scenario = ") && report.contains("scenario.toml"),
        "run report should reference the staged scenario\n{report}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_auto_resolves_missing_lockfile_before_launching() {
    let project = temp_dir("run-auto-resolve-project");
    let metadata = temp_dir("run-auto-resolve-metadata");
    let data_home = temp_dir("run-auto-resolve-data");
    let cache_home = temp_dir("run-auto-resolve-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-auto-resolve");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf 'auto resolve stdout\\n'\nprintf '%s\\n' \"$@\" > java-args.txt\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-auto-resolve"

[[instance]]
name = "server-auto-resolve-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let run = run_in_with_string_env(
        &[
            "run",
            "server",
            "server-auto-resolve-26.1.2",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        run.status.success(),
        "run should auto-resolve then launch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        project.join("modstage.lock").is_file(),
        "run should create modstage.lock before launching"
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("auto resolve stdout"),
        "run should stream output from the auto-resolved launch"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_auto_resolves_stale_lockfile_before_launching() {
    let project = temp_dir("run-stale-lock-project");
    let metadata = temp_dir("run-stale-lock-metadata");
    let data_home = temp_dir("run-stale-lock-data");
    let cache_home = temp_dir("run-stale-lock-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-stale-lock");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf 'stale lock stdout\\n'\nprintf '%s\\n' \"$@\" > java-args.txt\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-stale-lock"

[[instance]]
name = "server-stale-lock-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");
    fs::write(
        project.join("modstage.lock"),
        "# This file is generated by modstage. Do not edit by hand.\nversion = 1\n\n[[instance]]\ninstance = \"old-instance\"\n",
    )
    .expect("failed to write stale lockfile");

    let manifest_url = format!("file://{}", manifest.display());
    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let run = run_in_with_string_env(
        &[
            "run",
            "server",
            "server-stale-lock-26.1.2",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        run.status.success(),
        "run should refresh a stale lockfile then launch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let lock =
        fs::read_to_string(project.join("modstage.lock")).expect("lockfile should be readable");
    assert!(
        lock.contains(r#"instance = "server-stale-lock-26.1.2""#),
        "run should replace the stale lockfile with the selected instance\n{lock}"
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("stale lock stdout"),
        "run should stream output from the refreshed launch"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_client_executes_resolved_minecraft_artifact_with_configured_java() {
    let project = temp_dir("run-client-exec-project");
    let metadata = temp_dir("run-client-exec-metadata");
    let data_home = temp_dir("run-client-exec-data");
    let cache_home = temp_dir("run-client-exec-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-client");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf 'fake client stdout\\n'\nprintf '%s\\n' \"$@\" > java-args.txt\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-client-exec"

[[instance]]
name = "client-exec-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "client-exec-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "client",
            "client-exec-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should execute the configured Java command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("fake client stdout"),
        "run should stream client stdout"
    );
    let run_stdout = String::from_utf8_lossy(&run.stdout);
    for expected in [
        "run summary:",
        r#"instance = "client-exec-26.1.2""#,
        r#"side = "client""#,
        r#"minecraft = "26.1.2""#,
        r#"loader = "vanilla""#,
        "exit_code = 0",
        "report = ",
    ] {
        assert!(
            run_stdout.contains(expected),
            "run stdout should contain final summary field {expected:?}\n{run_stdout}"
        );
    }

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("client-exec-26.1.2")
        .join("client")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record its launch args");
    assert!(
        java_args.contains("-jar") && java_args.contains("client.jar"),
        "client launch should execute java -jar <client.jar>\n{java_args}"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    for expected in [
        r#"instance = "client-exec-26.1.2""#,
        r#"side = "client""#,
        r#"status = "passed""#,
        "exit_code = 0",
        "timed_out = false",
    ] {
        assert!(
            report.contains(expected),
            "run report should contain {expected:?}\n{report}"
        );
    }

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_run_uses_managed_java_matching_the_lockfile_major() {
    let project = temp_dir("run-managed-java-project");
    let metadata = temp_dir("run-managed-java-metadata");
    let data_home = temp_dir("run-managed-java-data");
    let cache_home = temp_dir("run-managed-java-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let managed_java = metadata.join("managed-java-25");
    fs::write(
        &managed_java,
        "#!/bin/sh\nif [ \"$1\" = '-XshowSettings:properties' ]; then\n  printf '    java.version = 25\\n    os.arch = x86_64\\n' >&2\n  exit 0\nfi\nprintf 'managed java stdout\\n'\nprintf '%s\\n' \"$@\" > java-args.txt\n",
    )
    .expect("failed to write managed java");
    let mut permissions = fs::metadata(&managed_java)
        .expect("managed java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&managed_java, permissions).expect("failed to chmod managed java");
    let managed_dir = data_home.join("modstage").join("java").join("25");
    fs::create_dir_all(&managed_dir).expect("failed to create managed Java dir");
    fs::write(
        managed_dir.join("runtime.toml"),
        format!("major = 25\njava = \"{}\"\n", managed_java.display()),
    )
    .expect("failed to write managed Java record");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-managed-java"

[[instance]]
name = "managed-java-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "managed-java-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let run = run_in_with_env(
        &[
            "run",
            "client",
            "managed-java-26.1.2",
            "--locked",
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "locked run should use managed Java for the lockfile major\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("managed java stdout"),
        "managed Java stdout should stream to the terminal"
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("managed-java-26.1.2")
        .join("client")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("managed Java should record its launch args");
    assert!(
        java_args.contains("-jar") && java_args.contains("client.jar"),
        "managed Java should receive the Minecraft launch args\n{java_args}"
    );
    let report_path = first_descendant_file(&data_home.join("modstage").join("runs"), "run.toml");
    let report = fs::read_to_string(report_path).expect("run report should be readable");
    assert!(
        report.contains(&format!(r#"java = "{}""#, managed_java.display())),
        "run report should record the managed Java path\n{report}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_run_rejects_managed_java_with_the_wrong_major_before_launch() {
    let project = temp_dir("run-managed-java-wrong-project");
    let metadata = temp_dir("run-managed-java-wrong-metadata");
    let data_home = temp_dir("run-managed-java-wrong-data");
    let cache_home = temp_dir("run-managed-java-wrong-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let managed_java = metadata.join("managed-java-24");
    fs::write(
        &managed_java,
        "#!/bin/sh\nif [ \"$1\" = '-XshowSettings:properties' ]; then\n  printf '    java.version = 24\\n    os.arch = x86_64\\n' >&2\n  exit 0\nfi\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'wrong managed java launched\\n'\n",
    )
    .expect("failed to write managed java");
    let mut permissions = fs::metadata(&managed_java)
        .expect("managed java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&managed_java, permissions).expect("failed to chmod managed java");
    let managed_dir = data_home.join("modstage").join("java").join("25");
    fs::create_dir_all(&managed_dir).expect("failed to create managed Java dir");
    fs::write(
        managed_dir.join("runtime.toml"),
        format!("major = 25\njava = \"{}\"\n", managed_java.display()),
    )
    .expect("failed to write managed Java record");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-managed-java-wrong"

[[instance]]
name = "managed-java-wrong-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "managed-java-wrong-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let run = run_in_with_env(
        &[
            "run",
            "client",
            "managed-java-wrong-26.1.2",
            "--locked",
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "locked run should reject a managed Java runtime with the wrong major"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("managed Java")
            && stderr.contains("requires Java 25")
            && stderr.contains("reported Java 24"),
        "locked run should explain the Java major mismatch\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        stderr
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("managed-java-wrong-26.1.2")
        .join("client")
        .join("game");
    assert!(
        !game_dir.join("java-args.txt").exists(),
        "wrong managed Java must not launch Minecraft"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_client_uses_mojang_main_class_and_libraries_from_lockfile() {
    let project = temp_dir("run-client-classpath-project");
    let metadata = temp_dir("run-client-classpath-metadata");
    let data_home = temp_dir("run-client-classpath-data");
    let cache_home = temp_dir("run-client-classpath-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    let library = metadata.join("example-lib-1.0.0.jar");
    let asset = metadata.join("asset.ogg");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&library, b"library").expect("failed to write library jar");
    fs::write(&asset, b"asset").expect("failed to write asset object");
    let asset_index = metadata.join("assets-26.json");
    fs::write(
        &asset_index,
        format!(
            r#"{{
  "objects": {{
    "minecraft/sounds/example.ogg": {{
      "hash": "07073e89283a7b4c254e22b82c08a83738c6a1f0",
      "size": 5,
      "url": "file://{}"
    }}
  }}
}}"#,
            asset.display()
        ),
    )
    .expect("failed to write asset index");
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "mainClass": "net.minecraft.client.main.Main",
  "assetIndex": {{
    "id": "26",
    "url": "file://{}"
  }},
  "javaVersion": {{ "majorVersion": 25 }},
  "downloads": {{
    "client": {{ "url": "file://{}" }},
    "server": {{ "url": "file://{}" }}
  }},
  "libraries": [{{
    "name": "com.example:example-lib:1.0.0",
    "downloads": {{
      "artifact": {{
        "path": "com/example/example-lib/1.0.0/example-lib-1.0.0.jar",
        "url": "file://{}"
      }}
    }}
  }}]
}}"#,
            asset_index.display(),
            client.display(),
            server.display(),
            library.display()
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
    let fake_java = metadata.join("fake-java-client-classpath");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'classpath client stdout\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-client-classpath"

[[instance]]
name = "client-classpath-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "client-classpath-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "client",
            "client-classpath-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should execute the configured Java command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("client-classpath-26.1.2")
        .join("client")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record its launch args");
    assert!(
        java_args.contains("-cp")
            && java_args.contains("client.jar")
            && java_args.contains("example-lib-1.0.0.jar")
            && java_args.contains("net.minecraft.client.main.Main")
            && java_args.contains("--assetIndex")
            && java_args.contains("26")
            && java_args.contains("--assetsDir")
            && java_args.contains("assets")
            && !java_args.contains("-jar"),
        "client launch should execute a classpath main-class launch\n{java_args}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_run_rejects_libraries_that_no_longer_match_the_lockfile_hash() {
    let project = temp_dir("run-locked-library-project");
    let metadata = temp_dir("run-locked-library-metadata");
    let data_home = temp_dir("run-locked-library-data");
    let cache_home = temp_dir("run-locked-library-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    let library = metadata.join("example-lib-1.0.0.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    "downloads": {{
      "artifact": {{
        "path": "com/example/example-lib/1.0.0/example-lib-1.0.0.jar",
        "url": "file://{}"
      }}
    }}
  }}]
}}"#,
            client.display(),
            server.display(),
            library.display()
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
    let fake_java = metadata.join("fake-java-locked-library");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'should not launch\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-locked-library"

[[instance]]
name = "locked-library-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "locked-library-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    fs::write(&library, b"changed library").expect("failed to mutate locked library source");
    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "client",
            "locked-library-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "locked run should fail when a library hash no longer matches"
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("library")
            && stderr.contains("hash mismatch")
            && stderr.contains("com.example:example-lib:1.0.0"),
        "locked run should explain the mismatched library hash\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        stderr
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("locked-library-26.1.2")
        .join("client")
        .join("game");
    assert!(
        !game_dir.join("java-args.txt").exists(),
        "locked library mismatch must not execute Java"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn locked_client_run_restores_assets_from_the_lockfile_before_launch() {
    let project = temp_dir("run-locked-assets-project");
    let metadata = temp_dir("run-locked-assets-metadata");
    let data_home = temp_dir("run-locked-assets-data");
    let cache_home = temp_dir("run-locked-assets-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    let asset = metadata.join("asset.ogg");
    let asset_hash = "07073e89283a7b4c254e22b82c08a83738c6a1f0";
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
    fs::write(&asset, b"asset").expect("failed to write asset object");
    let asset_index = metadata.join("assets-26.json");
    fs::write(
        &asset_index,
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
            asset.display()
        ),
    )
    .expect("failed to write asset index");
    let version_json = metadata.join("26.1.2.json");
    fs::write(
        &version_json,
        format!(
            r#"{{
  "id": "26.1.2",
  "mainClass": "net.minecraft.client.main.Main",
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
            asset_index.display(),
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
    let fake_java = metadata.join("fake-java-locked-assets");
    fs::write(
        &fake_java,
        format!(
            "#!/bin/sh\nassets_dir=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--assetsDir' ]; then\n    shift\n    assets_dir=\"$1\"\n  fi\n  shift\ndone\ntest -f \"$assets_dir/indexes/26.json\" || exit 12\ntest -f \"$assets_dir/objects/07/{asset_hash}\" || exit 13\nprintf 'assets restored\\n'\n"
        ),
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-locked-assets"

[[instance]]
name = "locked-assets-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "locked-assets-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    fs::remove_dir_all(cache_home.join("modstage")).expect("failed to clear redownloadable cache");
    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "client",
            "locked-assets-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "locked client run should restore assets before launching Java\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert!(
        String::from_utf8_lossy(&run.stdout).contains("assets restored"),
        "fake Java should confirm the restored asset layout"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_fabric_client_uses_loader_main_class_and_libraries() {
    let project = temp_dir("run-fabric-client-project");
    let metadata = temp_dir("run-fabric-client-metadata");
    let data_home = temp_dir("run-fabric-client-data");
    let cache_home = temp_dir("run-fabric-client-cache");
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
    let common_lib_dir = repo
        .join("com")
        .join("example")
        .join("common-lib")
        .join("1.0.0");
    let client_lib_dir = repo
        .join("com")
        .join("example")
        .join("client-lib")
        .join("1.0.0");
    let server_lib_dir = repo
        .join("com")
        .join("example")
        .join("server-lib")
        .join("1.0.0");
    fs::create_dir_all(&loader_dir).expect("failed to create loader artifact dir");
    fs::create_dir_all(&intermediary_dir).expect("failed to create intermediary artifact dir");
    fs::create_dir_all(&common_lib_dir).expect("failed to create common library dir");
    fs::create_dir_all(&client_lib_dir).expect("failed to create client library dir");
    fs::create_dir_all(&server_lib_dir).expect("failed to create server library dir");
    fs::write(loader_dir.join("fabric-loader-0.16.14.jar"), b"loader")
        .expect("failed to write loader jar");
    fs::write(
        intermediary_dir.join("intermediary-26.1.2.jar"),
        b"intermediary",
    )
    .expect("failed to write intermediary jar");
    fs::write(common_lib_dir.join("common-lib-1.0.0.jar"), b"common")
        .expect("failed to write common library jar");
    fs::write(client_lib_dir.join("client-lib-1.0.0.jar"), b"client lib")
        .expect("failed to write client library jar");
    fs::write(server_lib_dir.join("server-lib-1.0.0.jar"), b"server lib")
        .expect("failed to write server library jar");
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
  "mainClass": "net.minecraft.client.main.Main",
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
    "libraries": {
      "common": [
        {
          "name": "com.example:common-lib:1.0.0",
          "url": "file://REPO"
        }
      ],
      "client": [
        {
          "name": "com.example:client-lib:1.0.0",
          "url": "file://REPO"
        }
      ],
      "server": [
        {
          "name": "com.example:server-lib:1.0.0",
          "url": "file://REPO"
        }
      ]
    },
    "mainClass": {
      "client": "net.fabricmc.loader.impl.launch.knot.KnotClient",
      "server": "net.fabricmc.loader.impl.launch.knot.KnotServer"
    }
  }
}]"#
        .replace("file://REPO", &format!("file://{}", repo.display())),
    )
    .expect("failed to write fabric metadata");
    let fake_java = metadata.join("fake-java-fabric-client");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'fabric client stdout\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        format!(
            r#"[project]
name = "run-fabric-client"

[repositories]
fabric = "file://{}"

[[instance]]
name = "fabric-client-26.1.2"
minecraft = "26.1.2"
loader = "fabric"
loader_version = "latest"
sides = ["client"]
"#,
            repo.display()
        ),
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let fabric_url = format!("file://{}", fabric_metadata.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "fabric-client-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("MODSTAGE_FABRIC_META_URL", &fabric_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "client",
            "fabric-client-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should execute the configured Java command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("fabric-client-26.1.2")
        .join("client")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record its launch args");
    assert!(
        java_args.contains("-cp")
            && java_args.contains("client.jar")
            && java_args.contains("fabric-loader-0.16.14.jar")
            && java_args.contains("intermediary-26.1.2.jar")
            && java_args.contains("common-lib-1.0.0.jar")
            && java_args.contains("client-lib-1.0.0.jar")
            && !java_args.contains("server-lib-1.0.0.jar")
            && java_args.contains("net.fabricmc.loader.impl.launch.knot.KnotClient")
            && !java_args.contains("net.minecraft.client.main.Main"),
        "Fabric client launch should use the side-specific loader main class and libraries\n{java_args}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_neoforge_server_uses_loader_main_class_and_libraries() {
    let project = temp_dir("run-neoforge-server-project");
    let metadata = temp_dir("run-neoforge-server-metadata");
    let data_home = temp_dir("run-neoforge-server-data");
    let cache_home = temp_dir("run-neoforge-server-cache");
    let repo = metadata.join("repo");
    let installer_dir = repo
        .join("net")
        .join("neoforged")
        .join("neoforge")
        .join("4.0.0");
    fs::create_dir_all(&installer_dir).expect("failed to create NeoForge artifact dir");
    fs::write(installer_dir.join("neoforge-4.0.0.jar"), b"installer")
        .expect("failed to write NeoForge installer jar");
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
    let fake_java = metadata.join("fake-java-neoforge-server");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > java-args.txt\nprintf 'neoforge server stdout\\n'\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        format!(
            r#"[project]
name = "run-neoforge-server"

[repositories]
neoforge = "file://{}"

[[instance]]
name = "neoforge-server-26.1.2"
minecraft = "26.1.2"
loader = "neoforge"
loader_version = "latest"
sides = ["server"]
"#,
            repo.display()
        ),
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let neoforge_url = format!("file://{}", neoforge_metadata.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "neoforge-server-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("MODSTAGE_NEOFORGE_META_URL", &neoforge_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "neoforge-server-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        run.status.success(),
        "run should execute the configured Java command\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let state_root = data_home.join("modstage").join("instances");
    let game_dir = first_child(&state_root)
        .join("neoforge-server-26.1.2")
        .join("server")
        .join("game");
    let java_args = fs::read_to_string(game_dir.join("java-args.txt"))
        .expect("fake java should record its launch args");
    assert!(
        java_args.contains("-cp")
            && java_args.contains("server.jar")
            && java_args.contains("neoforge-4.0.0.jar")
            && java_args.contains("cpw.mods.bootstraplauncher.BootstrapLauncher")
            && !java_args.contains("-jar"),
        "NeoForge server launch should use the loader main class and loader libraries\n{java_args}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_enforces_timeout_and_records_it() {
    let project = temp_dir("run-timeout-project");
    let metadata = temp_dir("run-timeout-metadata");
    let data_home = temp_dir("run-timeout-data");
    let cache_home = temp_dir("run-timeout-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-timeout");
    fs::write(
        &fake_java,
        "#!/bin/sh\nprintf 'before timeout\\n'\nexec sleep 5\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-timeout"

[[instance]]
name = "server-timeout-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-timeout-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "server-timeout-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "10ms",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "run should fail when the process times out"
    );
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("server run timed out"),
        "timeout should be reported clearly\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report_dir = report_path
        .parent()
        .expect("run report should have a parent");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    for expected in [
        r#"instance = "server-timeout-26.1.2""#,
        r#"status = "timed_out""#,
        "timed_out = true",
        r#"timeout = "10ms""#,
    ] {
        assert!(
            report.contains(expected),
            "timeout report should contain {expected:?}\n{report}"
        );
    }
    assert_eq!(
        fs::read_to_string(report_dir.join("stdout.log")).expect("stdout log should exist"),
        "before timeout\n"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_copies_minecraft_logs_and_crash_reports() {
    let project = temp_dir("run-artifacts-project");
    let metadata = temp_dir("run-artifacts-metadata");
    let data_home = temp_dir("run-artifacts-data");
    let cache_home = temp_dir("run-artifacts-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-artifacts");
    fs::write(
        &fake_java,
        "#!/bin/sh\nmkdir -p logs crash-reports\nprintf 'minecraft latest log\\n' > logs/latest.log\nprintf 'crash details\\n' > crash-reports/crash-test.txt\nprintf 'process stdout\\n'\nprintf 'process stderr\\n' >&2\nexit 42\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-artifacts"

[[instance]]
name = "server-artifacts-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-artifacts-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "server-artifacts-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "run should fail when the process exits non-zero"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report_dir = report_path
        .parent()
        .expect("run report should have a parent");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    for expected in [
        r#"instance = "server-artifacts-26.1.2""#,
        r#"status = "failed""#,
        "exit_code = 42",
        r#"failure_class = "crash_report""#,
        r#"minecraft_log = ""#,
        r#"crash_report = ""#,
    ] {
        assert!(
            report.contains(expected),
            "artifact report should contain {expected:?}\n{report}"
        );
    }
    assert_eq!(
        fs::read_to_string(report_dir.join("minecraft-latest.log"))
            .expect("Minecraft latest log should be copied"),
        "minecraft latest log\n"
    );
    assert_eq!(
        fs::read_to_string(report_dir.join("crash-test.txt"))
            .expect("crash report should be copied"),
        "crash details\n"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
#[cfg(unix)]
fn run_server_classifies_mixin_failures_from_minecraft_log() {
    let project = temp_dir("run-mixin-project");
    let metadata = temp_dir("run-mixin-metadata");
    let data_home = temp_dir("run-mixin-data");
    let cache_home = temp_dir("run-mixin-cache");
    let server = metadata.join("server.jar");
    let client = metadata.join("client.jar");
    fs::write(&server, b"server").expect("failed to write server jar");
    fs::write(&client, b"client").expect("failed to write client jar");
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
    let fake_java = metadata.join("fake-java-mixin");
    fs::write(
        &fake_java,
        "#!/bin/sh\nmkdir -p logs\nprintf 'Mixin apply failed for mod liteminer\\n' > logs/latest.log\nexit 1\n",
    )
    .expect("failed to write fake java");
    let mut permissions = fs::metadata(&fake_java)
        .expect("fake java metadata should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_java, permissions).expect("failed to chmod fake java");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "run-mixin"

[[instance]]
name = "server-mixin-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["server"]
"#,
    )
    .expect("failed to write config");

    let manifest_url = format!("file://{}", manifest.display());
    let data_home_str = data_home.to_str().expect("data path is not UTF-8");
    let cache_home_str = cache_home.to_str().expect("cache path is not UTF-8");
    let resolve = run_in_with_string_env(
        &["resolve", "server-mixin-26.1.2"],
        &project,
        &[
            ("MODSTAGE_MOJANG_MANIFEST_URL", &manifest_url),
            ("XDG_DATA_HOME", data_home_str),
            ("XDG_CACHE_HOME", cache_home_str),
        ],
    );
    assert!(
        resolve.status.success(),
        "resolve should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&resolve.stdout),
        String::from_utf8_lossy(&resolve.stderr)
    );

    let fake_java_str = fake_java.to_str().expect("fake java path is not UTF-8");
    let run = run_in_with_env(
        &[
            "run",
            "server",
            "server-mixin-26.1.2",
            "--locked",
            "--java",
            fake_java_str,
            "--timeout",
            "5s",
        ],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        !run.status.success(),
        "run should fail when the process exits non-zero"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    assert!(
        report.contains(r#"failure_class = "mixin""#),
        "run report should classify obvious mixin failures\n{report}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(metadata).expect("failed to remove metadata");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

fn first_child(path: &Path) -> PathBuf {
    fs::read_dir(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .next()
        .expect("expected at least one child")
        .expect("failed to read child")
        .path()
}

fn first_descendant_file(root: &Path, file_name: &str) -> PathBuf {
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        for entry in fs::read_dir(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        {
            let path = entry.expect("failed to read entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(file_name) {
                return path;
            }
        }
    }

    panic!("could not find {file_name} under {}", root.display());
}
