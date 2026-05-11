use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::setup::python_env::sootie_venv_python;

const MANAGED_SIDECAR_IDLE_TIMEOUT_SECS: u64 = 0;

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

    install_sidecar_files_from(&src_dir, &dest_dir)?;

    println!("✓ Sidecar files installed to {}", dest_dir.display());
    Ok(())
}

fn install_sidecar_files_from(src_dir: &std::path::Path, dest_dir: &std::path::Path) -> Result<()> {
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

async fn warm_up_sidecar(port: u16, timeout_secs: u64) -> bool {
    let url = format!("http://127.0.0.1:{}/warmup", port);
    let client = reqwest::Client::new();

    client
        .get(&url)
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}

/// Launch sidecar with crash recovery
pub async fn launch_sidecar(port: u16) -> Result<SidecarGuard> {
    // Check if already running
    if is_sidecar_running(port).await {
        eprintln!("Sidecar already running on port {}", port);
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

    install_sidecar_files().context("Failed to sync sidecar files")?;

    let script = find_sidecar_script()?;

    eprintln!("Launching sidecar on port {}", port);

    // Retry loop with exponential backoff (max 3 attempts)
    for attempt in 0..3 {
        let delay = 2u64.pow(attempt);

        if attempt > 0 {
            eprintln!("Retry attempt {} (delay {}s)...", attempt + 1, delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }

        // Spawn process
        let mut child = Command::new(&venv_python)
            .arg(&script)
            .arg("--port")
            .arg(port.to_string())
            .arg("--idle-timeout")
            .arg(MANAGED_SIDECAR_IDLE_TIMEOUT_SECS.to_string())
            .stderr(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("Failed to spawn Python sidecar")?;

        // Wait for health check
        let healthy = wait_for_sidecar_health(port, 10).await;

        if healthy {
            eprintln!("✓ Sidecar started successfully");
            eprintln!("Warming vision model on sidecar thread...");
            if warm_up_sidecar(port, 180).await {
                eprintln!("✓ Vision model warmed");
            } else {
                eprintln!(
                    "Warning: vision model warmup failed or timed out; first grounding may be slow"
                );
            }
            return Ok(SidecarGuard::new(child));
        }

        // Kill failed child
        let _ = child.kill();
        let _ = child.wait();

        if attempt == 2 {
            return Err(anyhow::anyhow!(
                "Sidecar failed to start after 3 retries. Check sidecar logs for details"
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

/// Sidecar process guard
pub struct SidecarGuard {
    child: Option<Child>,
}

impl SidecarGuard {
    pub fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    pub fn empty() -> Self {
        Self { child: None }
    }
}

impl Drop for SidecarGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            // The CLI owns this process. Keep it alive for the server lifetime,
            // then stop it explicitly instead of relying on sidecar idle exit.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sidecar_install_dir_contains_sootie() {
        let dir = sidecar_install_dir();
        assert!(dir.to_string_lossy().contains("sootie"));
    }

    #[test]
    fn test_managed_sidecar_disables_idle_exit() {
        assert_eq!(MANAGED_SIDECAR_IDLE_TIMEOUT_SECS, 0);
    }

    #[test]
    fn test_install_sidecar_files_from_overwrites_stale_server() {
        let src_dir = tempdir().unwrap();
        let dest_dir = tempdir().unwrap();

        std::fs::write(src_dir.path().join("server.py"), "print('new sidecar')\n").unwrap();
        std::fs::write(src_dir.path().join("requirements.txt"), "pillow==10.4.0\n").unwrap();
        std::fs::write(dest_dir.path().join("server.py"), "print('old sidecar')\n").unwrap();

        install_sidecar_files_from(src_dir.path(), dest_dir.path()).unwrap();

        let server = std::fs::read_to_string(dest_dir.path().join("server.py")).unwrap();
        let requirements =
            std::fs::read_to_string(dest_dir.path().join("requirements.txt")).unwrap();

        assert_eq!(server, "print('new sidecar')\n");
        assert_eq!(requirements, "pillow==10.4.0\n");
    }
}
