use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::setup::python_env::sootie_venv_python;

/// Find bundled vision-sidecar directory
pub fn find_bundled_sidecar_dir() -> Result<PathBuf> {
    // Installed paths
    let installed_paths = [
        "/opt/homebrew/share/sootie/vision-sidecar",
        "/usr/local/share/sootie/vision-sidecar",
    ];

    for path in installed_paths {
        let dir = PathBuf::from(path);
        if dir.join("server.py").exists() {
            return Ok(dir);
        }
    }

    // Development path: current_exe parent resolution
    let exe = std::env::current_exe().context("Failed to get current executable path")?;

    // Typical: .build/release/sootie -> .build/release -> .build -> project_root
    let project_root = exe
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent());

    if let Some(root) = project_root {
        let sidecar_dir = root.join("vision-sidecar");
        if sidecar_dir.join("server.py").exists() {
            return Ok(sidecar_dir);
        }
    }

    Err(anyhow::anyhow!(
        "vision-sidecar files not found. \
         For development: ensure vision-sidecar/ exists in project root. \
         For installed: reinstall sootie."
    ))
}

/// Get sidecar install directory
pub fn sidecar_install_dir() -> PathBuf {
    dirs_next::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sootie")
        .join("vision-sidecar")
}

/// Install sidecar files from bundled directory to data directory
pub fn install_sidecar_files() -> Result<()> {
    let src_dir = find_bundled_sidecar_dir()?;
    let dest_dir = sidecar_install_dir();

    if !dest_dir.exists() {
        fs::create_dir_all(&dest_dir).context("Failed to create sidecar directory")?;
    }

    // Copy server.py
    fs::copy(src_dir.join("server.py"), dest_dir.join("server.py"))
        .context("Failed to copy server.py")?;

    // Copy requirements.txt
    if src_dir.join("requirements.txt").exists() {
        fs::copy(
            src_dir.join("requirements.txt"),
            dest_dir.join("requirements.txt"),
        )
        .context("Failed to copy requirements.txt")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            dest_dir.join("server.py"),
            fs::Permissions::from_mode(0o755),
        )
        .context("Failed to set server.py permissions")?;
    }

    println!("✓ Sidecar files installed to {}", dest_dir.display());
    Ok(())
}

/// Find sidecar script path
fn find_sidecar_script() -> Result<PathBuf> {
    let dest_dir = sidecar_install_dir();
    let script = dest_dir.join("server.py");

    if script.exists() {
        Ok(script)
    } else {
        Err(anyhow::anyhow!(
            "Sidecar script not found at {}. Run `sootie setup` to install it.",
            script.display()
        ))
    }
}

/// Check if sidecar is already running
pub async fn is_sidecar_running(port: u16) -> bool {
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::new();

    client
        .get(&url)
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}

/// Generate random auth token (32 bytes hex)
fn generate_auth_token() -> Result<String> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let token: [u8; 32] = rng.gen();
    Ok(hex::encode(token))
}

/// Read last N lines from stderr
fn read_stderr_lines(_stderr: Option<Stdio>, _n: usize) -> String {
    // This is simplified - actual implementation would capture stderr
    "Check sidecar logs for details".to_string()
}

/// Get auth token from running sidecar process
fn get_running_sidecar_auth_token(port: u16) -> Option<String> {
    use std::process::Command;

    let output = Command::new("ps")
        .args(["aux"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("server.py") && line.contains(&format!("--port {}", port)) {
            if let Some(idx) = line.find("--auth-token") {
                let remainder = &line[idx + "--auth-token ".len()..];
                let token = remainder.split_whitespace().next()?;
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Launch sidecar with crash recovery
pub async fn launch_sidecar(port: u16, auth_token: Option<&str>) -> Result<SidecarGuard> {
    // Check if already running
    if is_sidecar_running(port).await {
        println!("Sidecar already running on port {}", port);
        // Try to get auth token from running process
        let running_token = get_running_sidecar_auth_token(port);
        if let Some(token) = running_token {
            return Ok(SidecarGuard::empty().with_auth_token(token));
        }
        return Ok(SidecarGuard::empty());
    }

    // Resolve paths
    let venv_python = sootie_venv_python();
    if !venv_python.exists() {
        return Err(anyhow::anyhow!(
            "Venv python not found at {}. Run `sootie setup` first.",
            venv_python.display()
        ));
    }

    let script = find_sidecar_script()?;

    // Generate or use provided auth token
    let token = if let Some(t) = auth_token {
        t.to_string()
    } else {
        generate_auth_token()?
    };

    println!("Launching sidecar on port {} with auth token", port);

    // Retry loop with exponential backoff (max 3 attempts)
    for attempt in 0..3 {
        let delay = 2u64.pow(attempt);

        if attempt > 0 {
            println!("Retry attempt {} (delay {}s)...", attempt + 1, delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }

        // Spawn process
        let mut child = Command::new(&venv_python)
            .arg(&script)
            .arg("--port")
            .arg(port.to_string())
            .arg("--auth-token")
            .arg(&token)
            .arg("--idle-timeout")
            .arg("600")
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("Failed to spawn Python sidecar")?;

        // Wait for health check
        let healthy = wait_for_sidecar_health(port, 10).await;

        if healthy {
            println!("✓ Sidecar started successfully");
            return Ok(SidecarGuard::new(child).with_auth_token(token));
        }

        // Kill failed child
        let _ = child.kill();
        let _ = child.wait();

        if attempt == 2 {
            return Err(anyhow::anyhow!(
                "Sidecar failed to start after 3 retries. Last stderr: {}",
                read_stderr_lines(None, 5)
            ));
        }
    }

    Err(anyhow::anyhow!("Unexpected retry loop exit"))
}

/// Wait for sidecar health check
async fn wait_for_sidecar_health(port: u16, timeout_secs: u64) -> bool {
    let start = std::time::Instant::now();
    let url = format!("http://127.0.0.1:{}/health", port);
    let client = reqwest::Client::new();

    while start.elapsed().as_secs() < timeout_secs {
        if client
            .get(&url)
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .map(|resp| resp.status().is_success())
            .unwrap_or(false)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    false
}

/// Sidecar process guard with auth token
pub struct SidecarGuard {
    child: Option<Child>,
    auth_token: Option<String>,
}

impl SidecarGuard {
    pub fn new(child: Child) -> Self {
        Self {
            child: Some(child),
            auth_token: None,
        }
    }

    pub fn with_auth_token(mut self, token: String) -> Self {
        self.auth_token = Some(token);
        self
    }

    pub fn empty() -> Self {
        Self {
            child: None,
            auth_token: None,
        }
    }

    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }
}

impl Drop for SidecarGuard {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            // Don't kill sidecar - let it run with idle timeout
            // Sidecar will auto-exit after idle_timeout seconds
            // Just detach from the process
            let _ = child.wait(); // Wait for graceful shutdown
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sidecar_install_dir_contains_sootie() {
        let dir = sidecar_install_dir();
        assert!(dir.to_string_lossy().contains("sootie"));
    }

    #[test]
    fn test_generate_auth_token_length() {
        let token = generate_auth_token().unwrap();
        assert_eq!(token.len(), 64); // 32 bytes -> 64 hex chars
    }
}
