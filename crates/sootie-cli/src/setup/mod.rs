mod check;
mod config;
mod model_download;
mod python_env;
mod sidecar;
mod style;

use anyhow::Result;

pub use check::{
    count_issues, has_fixable_issues, layer_status_report, print_report, run_all_checks,
    CheckContext, CheckResult,
};
pub use config::{generate_default_config, load_config, resolve_vision_model_path, SootieConfig};
pub use model_download::{
    download_showui_model, estimate_model_size, update_config_with_model_path,
};
pub use python_env::{
    check_python_deps, create_venv, find_python, fix_python_deps, install_deps, sootie_venv_path,
    sootie_venv_python,
};
pub use sidecar::{install_sidecar_files, is_sidecar_running, launch_sidecar};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetupMode {
    Interactive,
    CheckOnly,
    AutoFix,
}

pub async fn run_setup(mode: SetupMode) -> Result<()> {
    let ctx = CheckContext::default();
    let results = run_all_checks(&ctx).await;

    print_report(&results);
    println!();

    let (fails, warns) = count_issues(&results);

    if fails > 0 || warns > 0 {
        println!(
            "{} issues found, {} optional features available.",
            fails, warns
        );
    } else {
        println!("All checks passed!");
    }

    if mode == SetupMode::CheckOnly {
        println!();
        println!("{}", layer_status_report(&results));
        return Ok(());
    }

    if !has_fixable_issues(&results) {
        println!();
        println!("{}", layer_status_report(&results));
        return Ok(());
    }

    let fix_all = if mode == SetupMode::AutoFix {
        true
    } else {
        ask_yes_no("Fix issues?", true)?
    };

    if !fix_all {
        println!();
        println!("{}", layer_status_report(&results));
        return Ok(());
    }

    println!();

    for result in &results {
        if result.fixable && result.status != check::CheckStatus::Pass {
            if let Err(e) = fix_issue(result, mode).await {
                println!("  ✗ Fix failed: {}", e);
            }
        }
    }

    println!();
    println!("Setup complete!");

    let final_results = run_all_checks(&ctx).await;
    println!("{}", layer_status_report(&final_results));

    Ok(())
}

async fn fix_issue(result: &CheckResult, mode: SetupMode) -> Result<()> {
    match result.name {
        "Configuration file" => {
            let do_fix = if mode == SetupMode::AutoFix {
                true
            } else {
                ask_yes_no("[Config] Generate config.toml?", true)?
            };

            if do_fix {
                let path = generate_default_config()?;
                println!("  ✓ Created {}", path.display());
            }
        }

        "Vision model + sidecar" => {
            let model_path = resolve_vision_model_path();
            let model_exists = model_path.is_some()
                && model_path
                    .as_ref()
                    .unwrap()
                    .join("model.safetensors")
                    .exists();

            if !model_exists {
                let size_gb = estimate_model_size() / 1_000_000_000;
                let do_download = if mode == SetupMode::AutoFix {
                    true
                } else {
                    ask_yes_no(
                        format!(
                            "[Vision] Download ShowUI-2B model (~{}GB from Hugging Face)?",
                            size_gb
                        ),
                        false,
                    )?
                };

                if do_download {
                    let path = download_showui_model().await?;
                    update_config_with_model_path(&path)?;
                }
            }

            let platform = std::env::consts::OS;
            let (deps_ok, _) = check_python_deps(platform)?;

            if !deps_ok {
                let deps_name = if platform == "macos" {
                    "mlx-vlm, transformers"
                } else {
                    "transformers, torch"
                };
                let do_deps = if mode == SetupMode::AutoFix {
                    true
                } else {
                    ask_yes_no(
                        format!("[Vision] Install Python dependencies ({})?", deps_name),
                        true,
                    )?
                };

                if do_deps {
                    let crate_version = env!("CARGO_PKG_VERSION");
                    fix_python_deps(platform, crate_version)?;
                }
            }

            let sidecar_dir = dirs_next::data_local_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join("sootie")
                .join("vision-sidecar");

            if !sidecar_dir.join("server.py").exists() {
                install_sidecar_files()?;
            }
        }

        _ => {
            println!("  No automatic fix available for {}", result.name);
        }
    }

    Ok(())
}

fn ask_yes_no(prompt: impl AsRef<str>, default_yes: bool) -> Result<bool> {
    dialoguer::Confirm::new()
        .with_prompt(prompt.as_ref())
        .default(default_yes)
        .interact()
        .map_err(|e| anyhow::anyhow!("Failed to read user input: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_setup_mode_values() {
        assert_ne!(SetupMode::Interactive, SetupMode::CheckOnly);
        assert_ne!(SetupMode::AutoFix, SetupMode::CheckOnly);
    }
}
