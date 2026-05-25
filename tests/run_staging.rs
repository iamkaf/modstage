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
    let root = std::env::temp_dir().join(format!(
        "modstage-{name}-{}-{nanos}",
        std::process::id()
    ));

    fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}

#[test]
fn run_server_reconciles_mods_writes_eula_and_records_report_before_launch() {
    let project = temp_dir("run-stage-project");
    let data_home = temp_dir("run-stage-data");
    let cache_home = temp_dir("run-stage-cache");
    fs::create_dir_all(project.join("mods")).expect("failed to create mods dir");
    fs::write(project.join("mods").join("example.jar"), b"abc")
        .expect("failed to write local jar");
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let run_id = report_path
        .parent()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .expect("run report should have a UTF-8 run id")
        .to_string();
    let report = fs::read_to_string(&report_path)
        .expect("run report should be readable");
    assert!(
        report.contains(r#"instance = "server-stage-26.1.2""#)
            && report.contains(r#"side = "server""#)
            && report.contains(r#"status = "staged""#),
        "run report should describe staged run\n{report}"
    );

    let inspect = run_in_with_env(
        &["inspect", "run", &run_id],
        &project,
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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

    let clean = run_in_with_env(
        &["clean", "instance", "server-stage-26.1.2", "--side", "server"],
        &project,
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
fn run_locked_requires_an_existing_lockfile_before_staging() {
    let project = temp_dir("run-locked-project");
    let data_home = temp_dir("run-locked-data");
    let cache_home = temp_dir("run-locked-cache");
    fs::create_dir_all(project.join("mods")).expect("failed to create mods dir");
    fs::write(project.join("mods").join("example.jar"), b"abc")
        .expect("failed to write local jar");
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
        java_args.contains("-jar") && java_args.contains("server.jar") && java_args.contains("nogui"),
        "server launch should execute java -jar <server.jar> nogui\n{java_args}"
    );

    let reports_root = data_home.join("modstage").join("runs");
    let report_path = first_descendant_file(&reports_root, "run.toml");
    let report_dir = report_path.parent().expect("run report should have a parent");
    let report = fs::read_to_string(&report_path).expect("run report should be readable");
    for expected in [
        r#"instance = "server-exec-26.1.2""#,
        r#"side = "server""#,
        r#"status = "passed""#,
        "exit_code = 0",
        "timed_out = false",
    ] {
        assert!(
            report.contains(expected),
            "run report should contain {expected:?}\n{report}"
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
        &[("XDG_DATA_HOME", &data_home), ("XDG_CACHE_HOME", &cache_home)],
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
    let report_dir = report_path.parent().expect("run report should have a parent");
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
