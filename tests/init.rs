use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run_in(args: &[&str], cwd: &Path) -> Output {
    modstage()
        .args(args)
        .current_dir(cwd)
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
fn init_creates_a_config_template_and_does_not_overwrite_it() {
    let project = temp_dir("init");
    let config = project.join("modstage.toml");

    let output = run_in(&["init"], &project);
    assert!(
        output.status.success(),
        "init should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let contents = fs::read_to_string(&config).expect("modstage.toml should exist");
    let project_name = project
        .file_name()
        .and_then(|name| name.to_str())
        .expect("temp project path should have a UTF-8 file name");
    assert!(
        contents.contains("[project]")
            && contents.contains(&format!(r#"name = "{project_name}""#))
            && contents.contains("[[instance]]")
            && contents.contains(r#"loader = "vanilla""#)
            && contents.contains(r#"sides = ["client", "server"]"#),
        "unexpected init template:\n{contents}"
    );

    let second = run_in(&["init"], &project);
    assert!(
        !second.status.success(),
        "second init should fail instead of overwriting modstage.toml"
    );

    let after_second = fs::read_to_string(&config).expect("modstage.toml should still exist");
    assert_eq!(
        contents, after_second,
        "failed init should leave modstage.toml unchanged"
    );

    fs::remove_dir_all(project).expect("failed to remove temp dir");
}
