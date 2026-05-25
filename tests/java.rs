use std::path::Path;
use std::process::{Command, Output};

fn modstage() -> Command {
    Command::new(env!("CARGO_BIN_EXE_modstage"))
}

fn run(args: &[&str]) -> Output {
    modstage()
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run modstage {args:?}: {error}"))
}

#[test]
fn java_doctor_reports_the_configured_java_runtime() {
    let java = java_on_path().expect("test environment must have java on PATH");
    let output = run(&["java", "doctor", "--java", java.to_str().expect("java path is not UTF-8")]);

    assert!(
        output.status.success(),
        "java doctor should validate the configured runtime\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("java:")
            && text.contains("version:")
            && text.contains("major:")
            && text.contains("arch:"),
        "java doctor should report runtime details\n{text}"
    );
}

#[test]
fn java_list_reports_discovered_runtimes() {
    let output = run(&["java", "list"]);

    assert!(
        output.status.success(),
        "java list should succeed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("java:") && text.contains("major:"),
        "java list should report at least one runtime\n{text}"
    );
}

fn java_on_path() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;

    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(java_bin());
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }

    None
}

fn java_bin() -> &'static str {
    if cfg!(windows) {
        "java.exe"
    } else {
        "java"
    }
}

fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}
