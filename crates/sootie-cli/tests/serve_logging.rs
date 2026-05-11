use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_home() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("sootie-log-test-{}-{}", std::process::id(), nanos))
}

fn candidate_log_dirs(home: &std::path::Path) -> [PathBuf; 3] {
    [
        home.join("Library")
            .join("Application Support")
            .join("sootie")
            .join("logs"),
        home.join(".local")
            .join("share")
            .join("sootie")
            .join("logs"),
        home.join("AppData")
            .join("Local")
            .join("sootie")
            .join("logs"),
    ]
}

fn find_log_file(home: &std::path::Path) -> Option<PathBuf> {
    candidate_log_dirs(home)
        .into_iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "log"))
}

#[test]
fn serve_creates_default_log_file() {
    let home = unique_temp_home();
    std::fs::create_dir_all(&home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sootie"))
        .arg("serve")
        .env("HOME", &home)
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("RUST_LOG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sootie serve exited with {:?}",
        output.status.code()
    );

    let log_path = find_log_file(&home).expect("default log file was not created");

    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        contents.contains("Sootie MCP server starting"),
        "startup log missing from {}",
        log_path.display()
    );

    std::fs::remove_dir_all(&home).unwrap();
}

#[test]
fn serve_log_level_overrides_rust_log_env() {
    let home = unique_temp_home();
    std::fs::create_dir_all(&home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_sootie"))
        .arg("serve")
        .arg("--log-level")
        .arg("info")
        .env("HOME", &home)
        .env("RUST_LOG", "error")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "sootie serve exited with {:?}",
        output.status.code()
    );

    let log_path = find_log_file(&home).expect("default log file was not created");

    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        contents.contains("Sootie MCP server starting"),
        "--log-level info should not be suppressed by RUST_LOG=error in {}",
        log_path.display()
    );

    std::fs::remove_dir_all(&home).unwrap();
}
