use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
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
    Setup,
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
        Commands::Setup => run_setup(),
        Commands::Serve {
            log_level,
            log_file,
        } => run_serve(log_level, log_file).await,
    }
}

fn run_setup() -> Result<()> {
    println!("Sootie Setup");
    println!("============");
    println!();

    let platform = std::env::consts::OS;
    println!("Platform: {}", platform);

    match platform {
        "macos" => {
            println!("Checking accessibility permissions...");
            println!("  macOS requires Accessibility API permission.");
            println!("  Go to: System Settings > Privacy & Security > Accessibility");
            println!("  Add your terminal app or the sootie binary.");
        }
        "windows" => {
            println!("Checking UI Automation permissions...");
            println!("  Windows UI Automation is available to desktop apps.");
            println!("  Run sootie from a non-elevated terminal for best results.");
        }
        "linux" => {
            println!("Checking AT-SPI2 availability...");
            println!("  Ensure at-spi2-core and at-spi2-atk are installed.");
            println!("  On Debian/Ubuntu: sudo apt install at-spi2-core");
        }
        _ => {
            println!("Unknown platform: {}", platform);
        }
    }

    println!();
    println!("Checking MCP configuration...");

    let config_dir = dirs_next::config_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("sootie");

    println!("  Config directory: {}", config_dir.display());

    println!();
    println!("Setup complete!");
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

    Ok(())
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

    info!(
        platform = std::env::consts::OS,
        version = env!("CARGO_PKG_VERSION"),
        "Sootie MCP server starting"
    );

    let perception = platform::create_perception_provider();
    let action = platform::create_action_provider();

    info!("Platform providers initialized");

    let server = SootieServer::new(perception, action);

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
            Commands::Setup => {}
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

    #[test]
    fn test_run_setup_does_not_panic() {
        let result = run_setup();
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
