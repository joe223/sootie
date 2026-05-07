mod setup;

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use sootie_core::platform;
use sootie_mcp::server::SootieServer;
use sootie_mcp::types::JsonRpcRequest;

#[derive(Parser)]
#[command(
    name = "sootie",
    version,
    about = "Cross-platform computer-use for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set up permissions, MCP configuration, and optional vision model
    Setup {
        /// Only check and report, do not fix issues
        #[arg(long)]
        check: bool,

        /// Automatically fix all fixable issues (non-interactive)
        #[arg(long)]
        fix: bool,
    },
    /// Start the MCP server (stdio mode)
    Serve {
        /// Log level (trace, debug, info, warn, error)
        #[arg(long, default_value = "info")]
        log_level: String,

        /// Log file path (optional, defaults to the platform data directory)
        #[arg(long)]
        log_file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup { check, fix } => run_setup(check, fix).await,
        Commands::Serve {
            log_level,
            log_file,
        } => run_serve(log_level, log_file).await,
    }
}

async fn run_setup(check_only: bool, auto_fix: bool) -> Result<()> {
    let mode = if check_only {
        setup::SetupMode::CheckOnly
    } else if auto_fix {
        setup::SetupMode::AutoFix
    } else {
        setup::SetupMode::Interactive
    };

    setup::run_setup(mode).await?;

    if mode == setup::SetupMode::CheckOnly {
        print_usage_instructions();
    }

    Ok(())
}

fn print_usage_instructions() {
    println!();
    println!("Configuration file: ~/.config/sootie/config.toml");
    println!();
    println!("Fallback priority can be configured in config.toml:");
    println!("  [fallback]");
    println!("  priority = [\"cdp\", \"at_tree\", \"vision\"]");
    println!();
    println!("Or via environment: SOOTIE_FALLBACK_PRIORITY=cdp,at_tree,vision");
    println!();
    println!("To use Sootie with Claude Code or other MCP clients:");
    println!("  Add the following to your MCP configuration:");
    println!();
    println!("  {{");
    println!("    \"mcpServers\": {{");
    println!("      \"sootie\": {{");
    println!("        \"command\": \"sootie\",");
    println!("        \"args\": [\"serve\"]");
    println!("      }}");
    println!("    }}");
    println!("  }}");
    println!();
    println!("Logging options:");
    println!("  sootie serve --log-level debug");
    println!("  default log file: {}", default_log_file_path().display());
    println!("  sootie serve --log-level debug --log-file /tmp/sootie.log");
    println!();
    println!("Log sanitization:");
    println!("  Sensitive data (passwords, API keys, emails) is automatically redacted in logs.");
    println!("  To disable sanitization for debugging:");
    println!("    Set log config: sanitize_logs: false");
    println!();
    println!("  Custom sensitive fields via environment:");
    println!("    SOOTIE_SENSITIVE_FIELDS=[\"custom_field1\",\"custom_field2\"]");
}

fn default_log_file_path() -> PathBuf {
    dirs_next::data_local_dir()
        .or_else(dirs_next::config_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sootie")
        .join("logs")
        .join("sootie.log")
}

fn resolve_log_file_path(log_file: Option<String>) -> PathBuf {
    log_file
        .map(PathBuf::from)
        .unwrap_or_else(default_log_file_path)
}

fn init_logging(log_level: &str, log_path: &Path) -> Result<()> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    let log_dir = log_path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(log_dir)?;

    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_writer(file)
        .init();

    Ok(())
}

async fn run_serve(log_level: String, log_file: Option<String>) -> Result<()> {
    let log_path = resolve_log_file_path(log_file);
    init_logging(&log_level, &log_path)?;
    info!(log_file = %log_path.display(), "Logging to file initialized");

    let config = setup::load_config().unwrap_or_else(|e| {
        warn!(error = %e, "Failed to load config file, using defaults");
        setup::SootieConfig::default()
    });

    if std::env::var("SOOTIE_FALLBACK_PRIORITY").is_err() {
        let priority_str = config
            .fallback
            .priority
            .iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",");
        std::env::set_var("SOOTIE_FALLBACK_PRIORITY", &priority_str);
        info!(fallback_priority = %priority_str, "Fallback priority configured");
    }

    if std::env::var("SOOTIE_VISION_MODEL_PATH").is_err() {
        if !config.vision.model_path.is_empty() {
            std::env::set_var("SOOTIE_VISION_MODEL_PATH", &config.vision.model_path);
            info!(model_path = %config.vision.model_path, "Vision model path configured");
        }
    }

    if std::env::var("SOOTIE_SIDECAR_PORT").is_err() {
        std::env::set_var(
            "SOOTIE_SIDECAR_PORT",
            config.vision.sidecar_port.to_string(),
        );
        info!(sidecar_port = %config.vision.sidecar_port, "Sidecar port configured");
    }

    info!(
        platform = std::env::consts::OS,
        version = env!("CARGO_PKG_VERSION"),
        "Sootie MCP server starting"
    );

    let _sidecar = if !config.vision.model_path.is_empty() || config.vision.auto_start {
        match setup::launch_sidecar(config.vision.sidecar_port, None).await {
            Ok(guard) => {
                info!(port = %config.vision.sidecar_port, auto_start = %config.vision.auto_start, "Python sidecar launched");
                Some(guard)
            }
            Err(e) => {
                warn!(error = %e, "Failed to launch Python sidecar, vision will be unavailable");
                None
            }
        }
    } else {
        None
    };

    let perception = platform::create_perception_provider();
    let action = platform::create_action_provider();

    info!("Platform providers initialized");

    let server = SootieServer::new(perception, action);

    // Print startup message to stderr (stdout is used for MCP)
    eprintln!("✓ Sootie MCP server started successfully");
    eprintln!("  Protocol: JSON-RPC 2.0 over stdio");
    eprintln!("  Logs: {}", log_path.display());
    eprintln!("  Ready to accept requests on stdin");
    if _sidecar.is_some() {
        eprintln!(
            "  Vision: Sidecar running on port {}",
            config.vision.sidecar_port
        );
    } else {
        eprintln!("  Vision: Disabled (run 'sootie setup' to enable)");
    }
    eprintln!();

    info!("MCP server ready, waiting for requests on stdin");

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match serde_json::from_str::<JsonRpcRequest>(line) {
            Ok(request) => {
                let response = server.handle_request(request).await;
                let response_json = serde_json::to_string(&response)?;
                writeln!(stdout, "{}", response_json)?;
                stdout.flush()?;
            }
            Err(e) => {
                tracing::error!(error = %e, raw_input = %line, "Failed to parse MCP request");
                let error_resp = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {
                        "code": -32700,
                        "message": format!("Parse error: {}", e)
                    }
                });
                writeln!(stdout, "{}", error_resp)?;
                stdout.flush()?;
            }
        }
    }

    info!("Sootie MCP server shutting down");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sootie_core::action::StubActionProvider;
    use sootie_core::perception::StubPerceptionProvider;

    #[test]
    fn test_cli_parse_setup() {
        let cli = Cli::try_parse_from(["sootie", "setup"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Setup { check, fix } => {
                assert!(!check);
                assert!(!fix);
            }
            _ => panic!("expected Setup command"),
        }
    }

    #[test]
    fn test_cli_parse_setup_check() {
        let cli = Cli::try_parse_from(["sootie", "setup", "--check"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Setup { check, fix } => {
                assert!(check);
                assert!(!fix);
            }
            _ => panic!("expected Setup command"),
        }
    }

    #[test]
    fn test_cli_parse_setup_fix() {
        let cli = Cli::try_parse_from(["sootie", "setup", "--fix"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Setup { check, fix } => {
                assert!(!check);
                assert!(fix);
            }
            _ => panic!("expected Setup command"),
        }
    }

    #[test]
    fn test_cli_parse_setup_check_and_fix_conflict() {
        let cli = Cli::try_parse_from(["sootie", "setup", "--check", "--fix"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Setup { check, fix } => {
                assert!(check);
                assert!(fix);
            }
            _ => panic!("expected Setup command"),
        }
    }

    #[test]
    fn test_cli_parse_serve_default() {
        let cli = Cli::try_parse_from(["sootie", "serve"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Serve {
                log_level,
                log_file,
            } => {
                assert_eq!(log_level, "info");
                assert!(log_file.is_none());
            }
            _ => panic!("expected Serve command"),
        }
    }

    #[test]
    fn test_cli_parse_serve_with_log_level() {
        let cli = Cli::try_parse_from(["sootie", "serve", "--log-level", "debug"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Serve { log_level, .. } => {
                assert_eq!(log_level, "debug");
            }
            _ => panic!("expected Serve command"),
        }
    }

    #[test]
    fn test_cli_parse_serve_with_log_file() {
        let cli = Cli::try_parse_from(["sootie", "serve", "--log-file", "/tmp/sootie.log"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Serve { log_file, .. } => {
                assert_eq!(log_file, Some("/tmp/sootie.log".to_string()));
            }
            _ => panic!("expected Serve command"),
        }
    }

    #[test]
    fn test_cli_parse_no_command() {
        let cli = Cli::try_parse_from(["sootie"]);
        assert!(cli.is_err());
    }

    #[tokio::test]
    async fn test_serve_handles_initialize() {
        let server = SootieServer::new(
            Box::new(StubPerceptionProvider),
            Box::new(StubActionProvider),
        );
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::Number(1.into())),
            method: "initialize".to_string(),
            params: None,
        };

        let response = server.handle_request(request).await;
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "sootie");
    }

    #[test]
    fn test_cli_parse_serve_with_all_options() {
        let cli = Cli::try_parse_from([
            "sootie",
            "serve",
            "--log-level",
            "trace",
            "--log-file",
            "/tmp/test.log",
        ]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Serve {
                log_level,
                log_file,
            } => {
                assert_eq!(log_level, "trace");
                assert_eq!(log_file, Some("/tmp/test.log".to_string()));
            }
            _ => panic!("expected Serve command"),
        }
    }

    #[tokio::test]
    async fn test_run_setup_does_not_panic() {
        let result = run_setup(true, false).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_default_log_file_path_uses_sootie_log_suffix() {
        let path = default_log_file_path();
        let suffix = Path::new("sootie").join("logs").join("sootie.log");
        assert!(
            path.ends_with(&suffix),
            "unexpected default log path: {}",
            path.display()
        );
    }

    #[test]
    fn test_resolve_log_file_path_falls_back_to_default() {
        assert_eq!(resolve_log_file_path(None), default_log_file_path());
    }

    #[test]
    fn test_resolve_log_file_path_preserves_explicit_value() {
        let path = resolve_log_file_path(Some("/tmp/custom.log".to_string()));
        assert_eq!(path, PathBuf::from("/tmp/custom.log"));
    }
}
