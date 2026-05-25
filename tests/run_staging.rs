use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

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
