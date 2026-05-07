use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Find Python executable in version range 3.10-3.13
/// Prioritizes versioned paths over bare python3
pub fn find_python() -> Result<PathBuf> {
    let candidates = [
        "/opt/homebrew/bin/python3.13",
        "/opt/homebrew/bin/python3.12",
        "/opt/homebrew/bin/python3.11",
        "/opt/homebrew/bin/python3.10",
        "/usr/local/bin/python3.13",
        "/usr/local/bin/python3.12",
        "/usr/local/bin/python3.11",
        "/usr/local/bin/python3.10",
        "/usr/bin/python3.13",
        "/usr/bin/python3.12",
        "/usr/bin/python3.11",
        "/usr/bin/python3.10",
        "python3",
    ];

    for candidate in candidates {
        let path = PathBuf::from(candidate);

        if let Ok(output) = Command::new(&path).arg("--version").output() {
            if output.status.success() {
                let version_str = String::from_utf8_lossy(&output.stdout);
                if let Some((major, minor)) = parse_python_version(&version_str) {
                    // Accept 3.10-3.13
                    if major == 3 && minor >= 10 && minor <= 13 {
                        println!("✓ Found Python {}.{} at {}", major, minor, path.display());
                        return Ok(path);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "Python 3.10-3.13 required for MLX + scipy wheel availability. \
         Install with: brew install python@3.13 (macOS) or apt install python3.12 (Linux)"
    ))
}

/// Parse Python version from version string like "Python 3.13.1"
fn parse_python_version(version_str: &str) -> Option<(u8, u8)> {
    let parts: Vec<&str> = version_str.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let version = parts[1];
    let nums: Vec<&str> = version.split('.').collect();

    if nums.len() < 2 {
        return None;
    }

    let major = nums[0].parse::<u8>().ok()?;
    let minor = nums[1].parse::<u8>().ok()?;

    Some((major, minor))
}

/// Get sootie venv path
pub fn sootie_venv_path() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("sootie")
        .join("venv")
}

/// Get venv python executable path
pub fn sootie_venv_python() -> PathBuf {
    let base = sootie_venv_path();
    if cfg!(target_os = "windows") {
        base.join("Scripts").join("python.exe")
    } else {
        base.join("bin").join("python3")
    }
}

/// Create or recreate venv with version stamp
/// Returns venv python path
pub fn create_venv(system_python: &Path, crate_version: &str) -> Result<PathBuf> {
    let venv_dir = sootie_venv_path();
    let venv_python = sootie_venv_python();
    let version_file = venv_dir.join(".sootie-version");

    // Check existing venv
    if venv_python.exists() {
        // Check version stamp
        if version_file.exists() {
            let existing_version = fs::read_to_string(&version_file)
                .context("Failed to read .sootie-version")?;

            if existing_version.trim() == crate_version {
                println!("✓ Existing venv matches sootie version {}", crate_version);
                return Ok(venv_python);
            }

            println!("Version mismatch ({} vs {}), recreating venv...", existing_version.trim(), crate_version);
            fs::remove_dir_all(&venv_dir)
                .context("Failed to remove stale venv")?;
        } else {
            // Legacy venv without stamp
            println!("Legacy venv without version stamp, recreating...");
            fs::remove_dir_all(&venv_dir)
                .context("Failed to remove legacy venv")?;
        }
    }

    // Create venv
    let parent = venv_dir.parent().unwrap();
    if !parent.exists() {
        fs::create_dir_all(parent)
            .context("Failed to create venv parent directory")?;
    }

    println!("Creating Python virtual environment...");
    let status = Command::new(system_python)
        .args(["-m", "venv", venv_dir.to_str().unwrap()])
        .status()
        .context("Failed to create venv")?;

    if !status.success() {
        return Err(anyhow::anyhow!("venv creation failed"));
    }

    // Write version stamp
    fs::write(&version_file, crate_version)
        .context("Failed to write .sootie-version")?;

    println!("✓ Virtual environment created at {}", venv_dir.display());
    Ok(venv_python)
}

/// Install Python dependencies for macOS (--no-deps mlx-vlm hack) or Linux
pub fn install_deps(venv_python: &Path, platform: &str) -> Result<()> {
    if platform == "macos" {
        install_macos_deps(venv_python)?;
    } else {
        install_other_deps(venv_python)?;
    }

    // Verify installation
    verify_deps(venv_python, platform)?;

    Ok(())
}

/// Install macOS deps: --no-deps mlx-vlm + pinned transformers
fn install_macos_deps(venv_python: &Path) -> Result<()> {
    println!("Installing Python dependencies (mlx-vlm with --no-deps hack)...");

    // Install mlx-vlm without deps to avoid PyTorch
    let status = Command::new(venv_python)
        .args([
            "-m", "pip", "install", "--quiet", "--no-deps",
            "mlx-vlm==0.1.15"
        ])
        .status()
        .context("Failed to install mlx-vlm")?;

    if !status.success() {
        println!("mlx-vlm --no-deps failed, falling back to normal install (may pull PyTorch)");
        let fallback_status = Command::new(venv_python)
            .args(["-m", "pip", "install", "--quiet", "mlx-vlm"])
            .status()
            .context("Fallback mlx-vlm install failed")?;

        if !fallback_status.success() {
            return Err(anyhow::anyhow!("mlx-vlm installation failed"));
        }
    }

    // Install remaining deps
    let status = Command::new(venv_python)
        .args([
            "-m", "pip", "install", "--quiet",
            "transformers==4.48.3",
            "mlx-lm>=0.21.5,<0.30.0",
            "mlx>=0.21.0,<1.0.0",
            "Pillow>=10.0.0,<12.0.0",
            "numpy>=1.23.4"
        ])
        .status()
        .context("Failed to install transformers")?;

    if !status.success() {
        return Err(anyhow::anyhow!("transformers installation failed"));
    }

    println!("✓ Python dependencies installed");
    Ok(())
}

/// Install Linux deps: transformers + torch
fn install_other_deps(venv_python: &Path) -> Result<()> {
    println!("Installing Python dependencies (transformers, torch)...");

    let status = Command::new(venv_python)
        .args([
            "-m", "pip", "install", "--quiet",
            "transformers<4.49",
            "torch",
            "Pillow>=10.0.0,<12.0.0",
            "numpy>=1.23.4"
        ])
        .status()
        .context("Failed to install transformers")?;

    if !status.success() {
        return Err(anyhow::anyhow!("transformers installation failed"));
    }

    println!("✓ Python dependencies installed");
    Ok(())
}

/// Verify deps installation by checking imports
fn verify_deps(venv_python: &Path, platform: &str) -> Result<()> {
    if platform == "macos" {
        // Verify mlx_vlm import
        let output = Command::new(venv_python)
            .args(["-c", "import mlx_vlm; print('OK')"])
            .output()
            .context("Failed to verify mlx_vlm import")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "mlx_vlm import verification failed:\n{}", stderr
            ));
        }

        // Verify transformers version < 4.49
        let output = Command::new(venv_python)
            .args(["-c", "import transformers; v = transformers.__version__; print(v)"])
            .output()
            .context("Failed to check transformers version")?;

        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if version.as_str() >= "4.49" {
                println!("Warning: transformers version {} >= 4.49, may have issues with mlx-vlm", version);
            }
        }
    } else {
        // Verify transformers import
        let output = Command::new(venv_python)
            .args(["-c", "import transformers; print('OK')"])
            .output()
            .context("Failed to verify transformers import")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "transformers import verification failed:\n{}", stderr
            ));
        }
    }

    println!("✓ Dependencies verified");
    Ok(())
}

/// Check Python dependencies status (fast check using pip show, not import)
pub fn check_python_deps(platform: &str) -> Result<(bool, String)> {
    let venv_python = sootie_venv_python();
    let python = if venv_python.exists() {
        venv_python
    } else {
        match find_python() {
            Ok(p) => p,
            Err(_) => return Ok((false, "Python 3.10-3.13 not found".to_string())),
        }
    };

    let version_output = Command::new(&python)
        .arg("--version")
        .output()
        .context("Failed to check Python version")?;

    let version_str = String::from_utf8_lossy(&version_output.stdout).trim().to_string();

    if platform == "macos" {
        // Fast check: use pip show instead of import (10x faster)
        let mlx_installed = Command::new(&python)
            .args(["-m", "pip", "show", "mlx-vlm"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let transformers_version = Command::new(&python)
            .args(["-m", "pip", "show", "transformers"])
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8(o.stdout).ok()?;
                // Parse version from "Version: X.Y.Z"
                stdout.lines()
                    .find(|l| l.starts_with("Version:"))
                    .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
            })
            .unwrap_or_default();

        if mlx_installed {
            Ok((true, format!("{} with mlx-vlm, transformers {}", version_str, transformers_version)))
        } else {
            Ok((false, format!("{} found but mlx-vlm not installed", version_str)))
        }
    } else {
        let transformers_version = Command::new(&python)
            .args(["-m", "pip", "show", "transformers"])
            .output()
            .ok()
            .and_then(|o| {
                let stdout = String::from_utf8(o.stdout).ok()?;
                stdout.lines()
                    .find(|l| l.starts_with("Version:"))
                    .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
            })
            .unwrap_or_default();

        Ok((true, format!("{} with transformers {}", version_str, transformers_version)))
    }
}

/// Fix Python deps by creating venv and installing deps
pub fn fix_python_deps(platform: &str, crate_version: &str) -> Result<()> {
    let system_python = find_python()?;
    let venv_python = create_venv(&system_python, crate_version)?;
    install_deps(&venv_python, platform)?;
    Ok(())
}