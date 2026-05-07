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

fn candidate_log_paths(home: &std::path::Path) -> [PathBuf; 3] {
    [
        home.join("Library")
            .join("Application Support")
            .join("sootie")
            .join("logs")
            .join("sootie.log"),
        home.join(".local")
            .join("share")
            .join("sootie")
            .join("logs")
            .join("sootie.log"),
        home.join("AppData")
            .join("Local")
            .join("sootie")
            .join("logs")
            .join("sootie.log"),
    ]
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

    let log_path = candidate_log_paths(&home)
        .into_iter()
        .find(|path| path.exists())
        .expect("default log file was not created");

    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        contents.contains("Sootie MCP server starting"),
        "startup log missing from {}",
        log_path.display()
    );

    std::fs::remove_dir_all(&home).unwrap();
}
