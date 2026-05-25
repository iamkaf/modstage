use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run(args: &[&str]) -> Output {
    modstage()
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

fn run_in(args: &[&str], cwd: &Path) -> Output {
    modstage()
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

fn assert_success(output: Output, context: &str) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "{context} failed\nstatus: {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );

    format!("{stdout}{stderr}")
}

fn temp_project(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX_EPOCH")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("modstage-{name}-{}-{nanos}", std::process::id()));

    fs::create_dir_all(root.join("nested").join("deeper")).expect("failed to create temp project");
    fs::write(
        root.join("modstage.toml"),
        r#"[project]
name = "cli-foundation"

[[instance]]
name = "vanilla-26.1.2"
minecraft = "26.1.2"
loader = "vanilla"
sides = ["client", "server"]
"#,
    )
    .expect("failed to write modstage.toml");

    root
}

#[test]
fn cli_foundation_dispatches_all_planned_commands() {
    let root_help = assert_success(run(&["--help"]), "root help");

    for expected in [
        "--config", "init", "resolve", "run", "inspect", "clean", "java",
    ] {
        assert!(
            root_help.contains(expected),
            "root help should mention {expected:?}\n{root_help}"
        );
    }

    for args in [
        &["init", "--help"][..],
        &["resolve", "--help"],
        &["run", "--help"],
        &["inspect", "config", "--help"],
        &["inspect", "lock", "--help"],
        &["inspect", "instance", "--help"],
        &["inspect", "run", "--help"],
        &["clean", "instance", "--help"],
        &["clean", "cache", "--help"],
        &["java", "list", "--help"],
        &["java", "install", "--help"],
        &["java", "doctor", "--help"],
    ] {
        assert_success(run(args), &format!("help for {args:?}"));
    }
}

#[test]
fn cli_foundation_finds_config_by_flag_or_parent_search() {
    let project = temp_project("config-discovery");
    let config = project.join("modstage.toml");
    let nested = project.join("nested").join("deeper");

    let explicit_config = assert_success(
        run(&[
            "--config",
            config.to_str().expect("temp config path is not UTF-8"),
            "inspect",
            "config",
        ]),
        "explicit --config inspect config",
    );
    assert!(
        explicit_config.contains("cli-foundation")
            && explicit_config.contains(config.to_str().expect("temp config path is not UTF-8")),
        "inspect config should report the explicit config path and project name\n{explicit_config}"
    );

    let discovered_config = assert_success(
        run_in(&["inspect", "config"], &nested),
        "parent-search inspect config",
    );
    assert!(
        discovered_config.contains("cli-foundation")
            && discovered_config.contains(config.to_str().expect("temp config path is not UTF-8")),
        "inspect config should discover the parent modstage.toml and report project name\n{discovered_config}"
    );

    fs::remove_dir_all(project).expect("failed to remove temp project");
}
