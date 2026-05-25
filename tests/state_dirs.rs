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
    let root = std::env::temp_dir().join(format!("modstage-{name}-{}-{nanos}", std::process::id()));

    fs::create_dir_all(&root).expect("failed to create temp dir");
    root
}

#[test]
fn inspect_config_reports_project_scoped_state_directories() {
    let project = temp_dir("state-project");
    let data_home = temp_dir("state-data");
    let cache_home = temp_dir("state-cache");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "state-test"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");

    let output = run_in_with_env(
        &["inspect", "config"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        output.status.success(),
        "inspect config should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("project: state-test")
            && text.contains("state-id: state-test-")
            && text.contains(&format!("data: {}", data_home.join("modstage").display()))
            && text.contains(&format!("cache: {}", cache_home.join("modstage").display())),
        "inspect config should include project state dirs\n{text}"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}

#[test]
fn clean_cache_removes_redownloadable_modstage_cache() {
    let project = temp_dir("clean-cache-project");
    let data_home = temp_dir("clean-cache-data");
    let cache_home = temp_dir("clean-cache-cache");
    fs::write(
        project.join("modstage.toml"),
        r#"[project]
name = "clean-cache"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write config");
    let cache_file = cache_home
        .join("modstage")
        .join("downloads")
        .join("mojang")
        .join("server.jar");
    fs::create_dir_all(cache_file.parent().expect("cache file should have parent"))
        .expect("failed to create cache dir");
    fs::write(&cache_file, b"cached").expect("failed to write cache file");

    let output = run_in_with_env(
        &["clean", "cache"],
        &project,
        &[
            ("XDG_DATA_HOME", &data_home),
            ("XDG_CACHE_HOME", &cache_home),
        ],
    );
    assert!(
        output.status.success(),
        "clean cache should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cache_home.join("modstage").exists(),
        "clean cache should remove the modstage cache directory"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("removed"),
        "clean cache should report what it removed"
    );

    fs::remove_dir_all(project).expect("failed to remove project");
    fs::remove_dir_all(data_home).expect("failed to remove data home");
    fs::remove_dir_all(cache_home).expect("failed to remove cache home");
}
