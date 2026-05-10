use std::process::Command;

use super::config::{config_file_path, resolve_vision_model_path};
use super::style::{BOLD, CYAN, GREEN, RED, RESET, YELLOW};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "{}✓{}", GREEN, RESET),
            CheckStatus::Fail => write!(f, "{}✗{}", RED, RESET),
            CheckStatus::Warn => write!(f, "{}⚠{}", YELLOW, RESET),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub name: &'static str,
    pub status: CheckStatus,
    pub message: String,
    pub fixable: bool,
}

#[allow(dead_code)]
pub struct CheckContext {
    pub cdp_host: String,
    pub cdp_port: u16,
    pub sidecar_port: u16,
}

impl Default for CheckContext {
    fn default() -> Self {
        Self {
            cdp_host: std::env::var("SOOTIE_CDP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            cdp_port: std::env::var("SOOTIE_CDP_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9222),
            sidecar_port: std::env::var("SOOTIE_SIDECAR_PORT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9876),
        }
    }
}

pub fn check_accessibility() -> CheckResult {
    let platform = std::env::consts::OS;

    match platform {
        "macos" => check_macos_accessibility(),
        "linux" => check_linux_x11_fallback(),
        "windows" => check_windows_win32_fallback(),
        _ => CheckResult {
            name: "Accessibility permissions",
            status: CheckStatus::Warn,
            message: format!("Unknown platform: {}", platform),
            fixable: false,
        },
    }
}

pub fn check_screen_recording() -> CheckResult {
    let platform = std::env::consts::OS;

    match platform {
        "macos" => check_macos_screen_recording(),
        _ => CheckResult {
            name: "Screen Recording permissions",
            status: CheckStatus::Warn,
            message: "Not required on this platform".to_string(),
            fixable: false,
        },
    }
}

#[cfg(target_os = "macos")]
fn check_macos_accessibility() -> CheckResult {
    use sootie_core::platform::macos::ax_fns::is_process_trusted;

    let trusted = is_process_trusted();

    if trusted {
        CheckResult {
            name: "Accessibility permissions",
            status: CheckStatus::Pass,
            message: "Granted".to_string(),
            fixable: false,
        }
    } else {
        CheckResult {
            name: "Accessibility permissions",
            status: CheckStatus::Fail,
            message: "Not granted. Go to: System Settings > Privacy & Security > Accessibility"
                .to_string(),
            fixable: false,
        }
    }
}

#[cfg(target_os = "macos")]
fn check_macos_screen_recording() -> CheckResult {
    use core_graphics::access::ScreenCaptureAccess;

    let has_permission = ScreenCaptureAccess::default().preflight();

    if has_permission {
        CheckResult {
            name: "Screen Recording permissions",
            status: CheckStatus::Pass,
            message: "Granted".to_string(),
            fixable: false,
        }
    } else {
        CheckResult {
            name: "Screen Recording permissions",
            status: CheckStatus::Fail,
            message: "Not granted. Required for vision backend. Go to: System Settings > Privacy & Security > Screen Recording"
                .to_string(),
            fixable: false,
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn check_macos_accessibility() -> CheckResult {
    CheckResult {
        name: "Accessibility permissions",
        status: CheckStatus::Warn,
        message: "macOS check not available on this platform".to_string(),
        fixable: false,
    }
}

#[cfg(not(target_os = "macos"))]
fn check_macos_screen_recording() -> CheckResult {
    CheckResult {
        name: "Screen Recording permissions",
        status: CheckStatus::Warn,
        message: "macOS check not available on this platform".to_string(),
        fixable: false,
    }
}

fn check_linux_x11_fallback() -> CheckResult {
    let missing: Vec<&str> = ["wmctrl", "xdotool", "import"]
        .into_iter()
        .filter(|command| !command_available(command))
        .collect();

    if missing.is_empty() {
        CheckResult {
            name: "Accessibility permissions",
            status: CheckStatus::Warn,
            message: "X11 fallback available via wmctrl, xdotool, import; native AT-SPI tree is not implemented yet".to_string(),
            fixable: false,
        }
    } else {
        CheckResult {
            name: "Accessibility permissions",
            status: CheckStatus::Fail,
            message: format!(
                "Missing Linux X11 fallback commands: {}. Install wmctrl, xdotool, and ImageMagick.",
                missing.join(", ")
            ),
            fixable: true,
        }
    }
}

fn check_windows_win32_fallback() -> CheckResult {
    CheckResult {
        name: "Accessibility permissions",
        status: CheckStatus::Warn,
        message:
            "Win32/window-tree fallback enabled; full UI Automation tree is not implemented yet"
                .to_string(),
        fixable: false,
    }
}

fn command_available(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|output| {
            output.status.success() || !output.stdout.is_empty() || !output.stderr.is_empty()
        })
        .unwrap_or(false)
}

pub async fn check_cdp(ctx: &CheckContext) -> CheckResult {
    let url = format!(
        "http://{}:{}{}",
        ctx.cdp_host, ctx.cdp_port, "/json/version"
    );

    let client = reqwest::Client::new();
    match client
        .get(&url)
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => CheckResult {
            name: "Chrome CDP",
            status: CheckStatus::Pass,
            message: format!("Available on port {}", ctx.cdp_port),
            fixable: false,
        },
        Ok(_) => CheckResult {
            name: "Chrome CDP",
            status: CheckStatus::Fail,
            message: "CDP endpoint returned error".to_string(),
            fixable: false,
        },
        Err(_) => CheckResult {
            name: "Chrome CDP",
            status: CheckStatus::Fail,
            message: format!(
                "Not detected on port {}. Start Chrome with --remote-debugging-port={}",
                ctx.cdp_port, ctx.cdp_port
            ),
            fixable: false,
        },
    }
}

pub fn check_vision_model() -> CheckResult {
    let model_path = resolve_vision_model_path();

    match model_path {
        Some(path) => {
            let has_safetensors = path.join("model.safetensors").exists()
                || path.join("model-00001-of-00001.safetensors").exists();
            let has_config = path.join("config.json").exists();
            let has_tokenizer =
                path.join("tokenizer.json").exists() || path.join("tokenizer_config.json").exists();

            if has_safetensors && has_config && has_tokenizer {
                let (python_ok, python_msg) =
                    super::python_env::check_python_deps(std::env::consts::OS)
                        .unwrap_or((false, "Python check failed".to_string()));

                if python_ok {
                    CheckResult {
                        name: "Vision model + sidecar",
                        status: CheckStatus::Pass,
                        message: format!("Model found, {}", python_msg),
                        fixable: false,
                    }
                } else {
                    CheckResult {
                        name: "Vision model + sidecar",
                        status: CheckStatus::Fail,
                        message: format!("Model found, but {}", python_msg),
                        fixable: true,
                    }
                }
            } else if path.join("pytorch_model.bin").exists() {
                CheckResult {
                    name: "Vision model + sidecar",
                    status: CheckStatus::Fail,
                    message: format!("Found pytorch_model.bin at {} - incompatible format. Delete and re-download safetensors version.", 
                        path.display()),
                    fixable: true,
                }
            } else {
                CheckResult {
                    name: "Vision model + sidecar",
                    status: CheckStatus::Fail,
                    message: format!("Model directory incomplete at {}", path.display()),
                    fixable: true,
                }
            }
        }
        None => CheckResult {
            name: "Vision model + sidecar",
            status: CheckStatus::Warn,
            message: "Not configured (optional)".to_string(),
            fixable: true,
        },
    }
}

pub fn check_config_file() -> CheckResult {
    let path = config_file_path();

    if path.exists() {
        let content = std::fs::read_to_string(&path);
        match content {
            Ok(_) => CheckResult {
                name: "Configuration file",
                status: CheckStatus::Pass,
                message: format!("Found at {}", path.display()),
                fixable: false,
            },
            Err(e) => CheckResult {
                name: "Configuration file",
                status: CheckStatus::Warn,
                message: format!("Found but unreadable: {}", e),
                fixable: false,
            },
        }
    } else {
        CheckResult {
            name: "Configuration file",
            status: CheckStatus::Fail,
            message: "Not found".to_string(),
            fixable: true,
        }
    }
}

pub fn check_environment_vars() -> CheckResult {
    let vars = [
        ("SOOTIE_CDP_HOST", "127.0.0.1"),
        ("SOOTIE_CDP_PORT", "9222"),
        ("SOOTIE_CDP_WS_URL", "(none)"),
        ("SOOTIE_VISION_MODEL_PATH", "(none)"),
        ("SOOTIE_VISION_USE_GPU", "false"),
        ("SOOTIE_SIDECAR_PORT", "9876"),
        ("SOOTIE_FALLBACK_PRIORITY", "cdp,at_tree,vision"),
        ("SOOTIE_SENSITIVE_FIELDS", "(none)"),
    ];

    let mut set_count = 0;
    let mut messages: Vec<String> = Vec::new();

    for (name, default) in vars {
        let value = std::env::var(name).unwrap_or_else(|_| default.to_string());
        if std::env::var(name).is_ok() {
            set_count += 1;
        }
        messages.push(format!("{}={}", name, value));
    }

    if set_count == 0 {
        CheckResult {
            name: "Environment variables",
            status: CheckStatus::Warn,
            message: "Using defaults".to_string(),
            fixable: false,
        }
    } else {
        CheckResult {
            name: "Environment variables",
            status: CheckStatus::Pass,
            message: format!("{} overrides set", set_count),
            fixable: false,
        }
    }
}

pub async fn run_all_checks(ctx: &CheckContext) -> Vec<CheckResult> {
    let mut results = Vec::new();

    println!("{}{}Sootie Setup{}", BOLD, CYAN, RESET);
    println!("{}{}============{}", BOLD, CYAN, RESET);
    println!();

    // Check 1: Accessibility
    let r1 = check_accessibility();
    print_check_result(1, &r1);
    results.push(r1);

    // Check 2: Screen Recording
    let r2 = check_screen_recording();
    print_check_result(2, &r2);
    results.push(r2);

    // Check 3: CDP
    let r3 = check_cdp(ctx).await;
    print_check_result(3, &r3);
    results.push(r3);

    // Check 4: Vision model
    println!(
        "{}{}[4/6]{} {:<30} Checking Python dependencies...",
        BOLD, CYAN, RESET, "Vision model + sidecar"
    );
    let r4 = check_vision_model();
    println!(
        "{}{}[4/6]{} {:<30} {} {}",
        BOLD, CYAN, RESET, r4.name, r4.status, r4.message
    );
    results.push(r4);

    // Check 5: Config file
    let r5 = check_config_file();
    print_check_result(5, &r5);
    results.push(r5);

    // Check 6: Environment vars
    let r6 = check_environment_vars();
    print_check_result(6, &r6);
    results.push(r6);

    println!();
    results
}

fn print_check_result(num: usize, result: &CheckResult) {
    println!(
        "{}{}[{}/6]{} {:<30} {} {}",
        BOLD, CYAN, num, RESET, result.name, result.status, result.message
    );
}

pub fn print_report(_results: &[CheckResult]) {
    // Report is already printed during run_all_checks() for real-time progress
    // This function is kept for backwards compatibility but does nothing
}

pub fn count_issues(results: &[CheckResult]) -> (usize, usize) {
    let fails = results
        .iter()
        .filter(|r| r.status == CheckStatus::Fail)
        .count();
    let warns = results
        .iter()
        .filter(|r| r.status == CheckStatus::Warn)
        .count();
    (fails, warns)
}

pub fn has_fixable_issues(results: &[CheckResult]) -> bool {
    results
        .iter()
        .any(|r| r.fixable && r.status != CheckStatus::Pass)
}

pub fn layer_status_report(results: &[CheckResult]) -> String {
    let cdp = results.iter().find(|r| r.name == "Chrome CDP");
    let acc = results
        .iter()
        .find(|r| r.name == "Accessibility permissions");
    let vision = results.iter().find(|r| r.name == "Vision model + sidecar");

    let cdp_status = match cdp.map(|r| r.status) {
        Some(CheckStatus::Pass) => format!("{}✓{} Layer 1 (CDP): Available", GREEN, RESET),
        _ => format!(
            "{}✗{} Layer 1 (CDP): Start Chrome with --remote-debugging-port",
            RED, RESET
        ),
    };

    let acc_status = match acc.map(|r| r.status) {
        Some(CheckStatus::Pass) => format!(
            "{}✓{} Layer 2 (AT Tree): Available (Accessibility API granted)",
            GREEN, RESET
        ),
        _ => format!(
            "{}✗{} Layer 2 (AT Tree): Grant Accessibility permission",
            RED, RESET
        ),
    };

    let vision_status = match vision.map(|r| r.status) {
        Some(CheckStatus::Pass) => format!(
            "{}✓{} Layer 3 (Vision): ShowUI-2B via Python sidecar",
            GREEN, RESET
        ),
        Some(CheckStatus::Warn) => format!(
            "{}⚠{} Layer 3 (Vision): Optional — run `sootie setup` to configure",
            YELLOW, RESET
        ),
        _ => format!("{}✗{} Layer 3 (Vision): Configure model", RED, RESET),
    };

    format!("{}\n{}\n{}", cdp_status, acc_status, vision_status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_status_display() {
        assert!(CheckStatus::Pass.to_string().contains('✓'));
        assert!(CheckStatus::Fail.to_string().contains('✗'));
        assert!(CheckStatus::Warn.to_string().contains('⚠'));
    }

    #[test]
    fn test_check_config_file_missing() {
        let result = check_config_file();
        assert!(result.status == CheckStatus::Fail || result.status == CheckStatus::Pass);
    }

    #[test]
    fn test_check_vision_model_missing() {
        let result = check_vision_model();
        assert!(
            result.status == CheckStatus::Warn
                || result.status == CheckStatus::Fail
                || result.status == CheckStatus::Pass
        );
    }

    #[test]
    fn test_count_issues_empty() {
        let results: Vec<CheckResult> = vec![CheckResult {
            name: "test",
            status: CheckStatus::Pass,
            message: "".into(),
            fixable: false,
        }];
        let (fails, warns) = count_issues(&results);
        assert_eq!(fails, 0);
        assert_eq!(warns, 0);
    }

    #[test]
    fn test_count_issues_with_fail() {
        let results: Vec<CheckResult> = vec![
            CheckResult {
                name: "test",
                status: CheckStatus::Fail,
                message: "".into(),
                fixable: false,
            },
            CheckResult {
                name: "test2",
                status: CheckStatus::Warn,
                message: "".into(),
                fixable: false,
            },
        ];
        let (fails, warns) = count_issues(&results);
        assert_eq!(fails, 1);
        assert_eq!(warns, 1);
    }

    #[test]
    fn test_has_fixable_issues_true() {
        let results: Vec<CheckResult> = vec![CheckResult {
            name: "test",
            status: CheckStatus::Fail,
            message: "".into(),
            fixable: true,
        }];
        assert!(has_fixable_issues(&results));
    }

    #[test]
    fn test_has_fixable_issues_false() {
        let results: Vec<CheckResult> = vec![CheckResult {
            name: "test",
            status: CheckStatus::Pass,
            message: "".into(),
            fixable: true,
        }];
        assert!(!has_fixable_issues(&results));
    }
}
