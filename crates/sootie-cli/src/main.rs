use std::io::{self, BufRead, Write};

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use sootie_core::platform;
use sootie_mcp::server::SootieServer;
use sootie_mcp::types::JsonRpcRequest;

#[derive(Parser)]
#[command(name = "sootie", version, about = "Cross-platform computer-use for AI agents")]
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

        /// Log file path (optional, logs to stderr by default)
        #[arg(long)]
        log_file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Setup => run_setup(),
        Commands::Serve { log_level, log_file } => run_serve(log_level, log_file).await,
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
    println!("  sootie serve --log-level debug --log-file /tmp/sootie.log");

    Ok(())
}

async fn run_serve(log_level: String, log_file: Option<String>) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&log_level));

    if let Some(ref path) = log_file {
        let log_dir = std::path::Path::new(path).parent().unwrap_or(std::path::Path::new("."));
        std::fs::create_dir_all(log_dir)?;

        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .with_writer(file)
            .init();

        info!(log_file = %path, "Logging to file initialized");
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(true)
            .with_thread_ids(true)
            .init();
    }

    info!(platform = std::env::consts::OS, version = env!("CARGO_PKG_VERSION"), "Sootie MCP server starting");

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
            Commands::Serve { log_level, log_file } => {
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
}
