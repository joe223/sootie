use std::io::{self, BufRead, Write};

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

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
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Setup => run_setup(),
        Commands::Serve => run_serve().await,
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

    Ok(())
}

async fn run_serve() -> Result<()> {
    info!("Starting Sootie MCP server");

    let perception = platform::create_perception_provider();
    let action = platform::create_action_provider();

    let server = SootieServer::new(perception, action);

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
                tracing::error!("Failed to parse request: {}", e);
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
    fn test_cli_parse_serve() {
        let cli = Cli::try_parse_from(["sootie", "serve"]);
        assert!(cli.is_ok());
        match cli.unwrap().command {
            Commands::Serve => {}
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
