use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand};
use sootie_core::{create_backend, JsonRpcRequest, McpServer, RuntimeDiagnostic};
use tracing_subscriber::EnvFilter;

const DEFAULT_VISION_URL: &str = "http://127.0.0.1:9876";
const DEFAULT_VISION_PORT: u16 = 9876;
const DEFAULT_VISION_MODEL_ID: &str = "showlab/ShowUI-2B";
const SIDECAR_SERVER_PY: &str = include_str!("../../../vision-sidecar/server.py");
const SIDECAR_REQUIREMENTS_TXT: &str = include_str!("../../../vision-sidecar/requirements.txt");
const SIDECAR_DOWNLOAD_MODEL_PY: &str = include_str!("../../../vision-sidecar/download_model.py");

#[derive(Debug, Parser)]
#[command(
    name = "sootie",
    version,
    about = "Rust computer-use runtime for AI agents"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,

    #[arg(long, global = true, default_value = "info", hide = true)]
    log_level: String,

    #[arg(long, global = true, hide = true)]
    log_file: Option<PathBuf>,

    /// Print machine-readable raw output instead of the default readable summary.
    #[arg(long, global = true)]
    raw: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run MCP JSON-RPC over stdio.
    Serve,
    /// Create the user config file used by Sootie.
    Setup {
        /// Overwrite an existing config file.
        #[arg(long, hide = true)]
        force: bool,
        /// Skip installing local vision sidecar assets.
        #[arg(long, hide = true)]
        skip_sidecar: bool,
        /// Configure target resolution to use vision directly for described targets.
        #[arg(long, hide = true)]
        vision_only: bool,
        /// Deprecated alias kept for old scripts; setup now always provisions vision.
        #[arg(long, hide = true)]
        full: bool,
        /// Local vision sidecar URL implementing POST /ground.
        #[arg(long, default_value = DEFAULT_VISION_URL, hide = true)]
        vision_url: String,
        /// Existing local model directory for the vision sidecar.
        #[arg(long, hide = true)]
        vision_model_path: Option<PathBuf>,
        /// Hugging Face model id to download.
        #[arg(long, default_value = DEFAULT_VISION_MODEL_ID, hide = true)]
        vision_model_id: String,
        /// Deprecated alias kept for old scripts; setup now always installs dependencies.
        #[arg(long, hide = true)]
        install_vision_deps: bool,
        /// Deprecated alias kept for old scripts; setup now downloads the model when missing.
        #[arg(long, hide = true)]
        download_vision_model: bool,
    },
    /// Run the local vision grounding sidecar.
    Sidecar {
        /// HTTP port to listen on.
        #[arg(long, default_value_t = DEFAULT_VISION_PORT)]
        port: u16,
        /// Local model directory. Defaults to Sootie's managed model directory.
        #[arg(long)]
        model_path: Option<PathBuf>,
        /// Ask the sidecar to validate model files and exit.
        #[arg(long)]
        health_check: bool,
        /// Load the model before accepting requests.
        #[arg(long)]
        preload: bool,
    },
    /// Print platform/backend diagnostics.
    Doctor {
        /// Exit non-zero when the current runtime is not ready.
        #[arg(long)]
        check: bool,
    },
    /// List static MCP tool definitions.
    Tools,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => {
            init_serve_logging(&cli.log_level, cli.log_file.as_deref())?;
            let backend = create_backend();
            tracing::info!(
                platform = backend.platform(),
                version = env!("CARGO_PKG_VERSION"),
                "Sootie MCP server starting"
            );
            let mut server = McpServer::new(backend);
            server.serve_stdio()
        }
        Command::Setup {
            force,
            skip_sidecar,
            vision_only,
            full: _,
            vision_url,
            vision_model_path,
            vision_model_id,
            install_vision_deps: _,
            download_vision_model: _,
        } => {
            init_logging(&cli.log_level, cli.log_file.as_deref())?;
            run_setup(
                SetupOptions {
                    force,
                    skip_sidecar,
                    vision_only,
                    vision_url,
                    vision_model_path,
                    vision_model_id,
                },
                cli.raw,
            )
        }
        Command::Sidecar {
            port,
            model_path,
            health_check,
            preload,
        } => {
            init_logging(&cli.log_level, cli.log_file.as_deref())?;
            run_sidecar(port, model_path, health_check, preload, cli.raw)
        }
        Command::Doctor { check } => {
            init_logging(&cli.log_level, cli.log_file.as_deref())?;
            run_doctor(check, cli.raw)
        }
        Command::Tools => {
            init_logging(&cli.log_level, cli.log_file.as_deref())?;
            let payload = tools_json()?;
            if cli.raw {
                println!("{payload}");
            } else {
                let parsed: serde_json::Value = serde_json::from_str(&payload)?;
                print_tools_summary(&parsed);
            }
            Ok(())
        }
    }
}

struct RuntimeReadinessReport {
    payload: serde_json::Value,
    ready: bool,
    blocker_message: String,
}

fn runtime_readiness_report() -> RuntimeReadinessReport {
    let backend = create_backend();
    let state = backend.state(None);
    let context = backend.context(None);
    let screenshot = backend.screenshot(None, None, false);
    let diagnostics = backend.diagnostics();
    let context_app = context
        .as_ref()
        .ok()
        .and_then(|context| context.app.clone());
    let context_window = context
        .as_ref()
        .ok()
        .and_then(|context| context.window.clone());
    let context_element_count = context
        .as_ref()
        .map(|context| context.interactive_elements.len())
        .unwrap_or(0);
    let screenshot_available = screenshot.is_ok();
    let blockers = runtime_blockers(
        context_app.as_deref(),
        context_window.as_deref(),
        context_element_count,
        screenshot_available,
        &diagnostics,
    );
    let runtime_ready = blockers.is_empty();
    let blocker_message = blockers.join(", ");
    let payload = serde_json::json!({
        "platform": backend.platform(),
        "launch_context": launch_context_payload(),
        "state_available": state.is_ok(),
        "app_count": state.as_ref().map(|apps| apps.len()).unwrap_or(0),
        "state_error": state.err().map(|err| err.to_string()),
        "context_available": context.is_ok(),
        "context_app": context_app,
        "context_window": context_window,
        "context_element_count": context_element_count,
        "context_error": context.err().map(|err| err.to_string()),
        "screenshot_available": screenshot_available,
        "screenshot_size": screenshot.as_ref().ok().map(|screenshot| serde_json::json!({
            "width": screenshot.width,
            "height": screenshot.height,
        })),
        "screenshot_error": screenshot.err().map(|err| err.to_string()),
        "runtime_ready": runtime_ready,
        "runtime_blockers": blockers,
        "runtime_diagnostics": diagnostics,
        "notes": platform_notes(backend.platform()),
    });
    RuntimeReadinessReport {
        payload,
        ready: runtime_ready,
        blocker_message,
    }
}

fn run_doctor(check: bool, raw: bool) -> anyhow::Result<()> {
    let report = runtime_readiness_report();
    if raw {
        println!("{}", serde_json::to_string_pretty(&report.payload)?);
    } else {
        print_doctor_summary(&report.payload);
    }
    std::io::stdout().flush()?;
    if check && !report.ready {
        anyhow::bail!("runtime not ready: {}", report.blocker_message);
    }
    Ok(())
}

fn print_setup_summary(payload: &serde_json::Value) {
    println!("Sootie setup");
    println!(
        "Status: {} serve and sidecar readiness",
        status_label(json_bool(payload, "ready").unwrap_or(false))
    );
    println!(
        "Config: {} ({})",
        json_str(payload, "config_path").unwrap_or("<unknown>"),
        setup_config_action(payload)
    );
    println!(
        "Resolution: {}",
        json_str(payload, "resolution_strategy").unwrap_or("unknown")
    );
    println!(
        "Vision endpoint: {}",
        json_str(payload, "vision_url").unwrap_or("unknown")
    );

    let Some(sidecar) = payload.get("sidecar") else {
        println!("Sidecar: [not ready] missing setup report");
        return;
    };
    println!();
    println!("Vision sidecar:");
    println!(
        "  Files: {} {}",
        status_label(json_bool(sidecar, "installed").unwrap_or(false)),
        json_str(sidecar, "dir").unwrap_or("<unknown>")
    );
    println!(
        "  Python: {}",
        json_str(sidecar, "python").unwrap_or("<not found>")
    );
    println!(
        "  Dependencies: {} {}",
        status_label(json_bool(sidecar, "dependencies_ready").unwrap_or(false)),
        json_str(sidecar, "dependencies_status").unwrap_or("")
    );
    if json_bool(sidecar, "dependencies_installed").unwrap_or(false) {
        println!("  Dependencies action: installed during this setup run");
    }
    if let Some(model) = sidecar.get("model") {
        println!(
            "  Model: {} {}",
            status_label(json_bool(model, "ready").unwrap_or(false)),
            json_str(model, "path").unwrap_or("<unknown>")
        );
        println!(
            "  Model status: {}",
            json_str(model, "status").unwrap_or("")
        );
        if json_bool(model, "downloaded").unwrap_or(false) {
            println!("  Model action: downloaded during this setup run");
        }
    }
    if let Some(preload) = sidecar.get("preload") {
        let preload_status = json_str(preload, "status").unwrap_or("");
        println!(
            "  Preload: {} {}",
            status_label(json_bool(preload, "ready").unwrap_or(false)),
            readable_sidecar_health(preload_status).unwrap_or_else(|| preload_status.to_string())
        );
    }
    if let Some(sidecar_server) = payload.get("sidecar_server") {
        println!(
            "  Command: {} {}",
            status_label(json_bool(sidecar_server, "ready").unwrap_or(false)),
            json_str(sidecar_server, "status").unwrap_or("")
        );
    }

    if let Some(serve) = payload.get("serve") {
        println!();
        println!(
            "MCP serve: {} {}",
            status_label(json_bool(serve, "ready").unwrap_or(false)),
            json_str(serve, "status").unwrap_or("")
        );
    }

    if let Some(runtime) = payload.get("runtime") {
        println!();
        println!(
            "Runtime: {}",
            status_label(json_bool(runtime, "runtime_ready").unwrap_or(false))
        );
        if !json_bool(runtime, "runtime_ready").unwrap_or(false) {
            print_string_array(runtime, "runtime_blockers", "Blockers");
        }
    }

    if let Some(next_steps) = payload.get("next_steps").and_then(|value| value.as_array()) {
        if !next_steps.is_empty() {
            println!();
            println!("Next steps:");
            for step in next_steps.iter().filter_map(|value| value.as_str()) {
                println!("  - {step}");
            }
        }
    }
    println!();
    println!("Use `sootie setup --raw` for the full setup JSON.");
}

fn print_doctor_summary(payload: &serde_json::Value) {
    println!("Sootie doctor");
    println!(
        "Platform: {}",
        json_str(payload, "platform").unwrap_or("unknown")
    );
    println!(
        "Runtime: {}",
        status_label(json_bool(payload, "runtime_ready").unwrap_or(false))
    );
    println!();
    println!(
        "Desktop state: {}",
        status_label(json_bool(payload, "state_available").unwrap_or(false))
    );
    println!(
        "Apps visible: {}",
        json_u64(payload, "app_count").unwrap_or(0)
    );
    if let Some(error) = json_str(payload, "state_error") {
        println!("State error: {error}");
    }
    println!();
    println!(
        "Context: {}",
        status_label(json_bool(payload, "context_available").unwrap_or(false))
    );
    if let Some(app) = json_str(payload, "context_app") {
        println!("Front app: {app}");
    }
    if let Some(window) = json_str(payload, "context_window") {
        println!("Window: {window}");
    }
    println!(
        "Interactive elements: {}",
        json_u64(payload, "context_element_count").unwrap_or(0)
    );
    if let Some(error) = json_str(payload, "context_error") {
        println!("Context error: {error}");
    }
    println!();
    println!(
        "Screenshot: {}",
        status_label(json_bool(payload, "screenshot_available").unwrap_or(false))
    );
    if let Some(size) = payload
        .get("screenshot_size")
        .and_then(|value| value.as_object())
    {
        let width = size
            .get("width")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        let height = size
            .get("height")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        println!("Screenshot size: {width}x{height}");
    }
    if let Some(error) = json_str(payload, "screenshot_error") {
        println!("Screenshot error: {error}");
    }
    print_string_array(payload, "runtime_blockers", "Blockers");
    print_diagnostics(payload);
    print_string_array(payload, "notes", "Notes");
    println!();
    println!("Use `sootie doctor --raw` for the full diagnostic JSON.");
}

fn print_tools_summary(payload: &serde_json::Value) {
    let tools = payload
        .as_array()
        .map(|items| items.as_slice())
        .unwrap_or(&[]);
    let read_only = tools
        .iter()
        .filter(|tool| {
            tool.get("annotations")
                .and_then(|annotations| annotations.get("readOnlyHint"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        })
        .count();
    let action_capable = tools.len().saturating_sub(read_only);
    println!("Sootie tools");
    println!("Status: [ok] {} tools available", tools.len());
    println!("Read-only: {read_only}");
    println!("Action-capable: {action_capable}");
    println!();
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("<unknown>");
        let description = tool
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        println!("  {name} - {description}");
    }
    println!();
    println!("Use `sootie tools --raw` for the full MCP tool schema.");
}

fn print_sidecar_health_summary(payload: &serde_json::Value) {
    println!("Sootie vision sidecar");
    println!(
        "Model files: {}",
        status_label(json_bool(payload, "model_ready").unwrap_or(false))
    );
    println!(
        "Model loaded: {}",
        status_label(json_bool(payload, "model_loaded").unwrap_or(false))
    );
    if let Some(path) = json_str(payload, "model_path") {
        println!("Model path: {path}");
    }
    if let Some(error) = json_str(payload, "error") {
        println!("Error: {error}");
    }
    println!();
    println!("Use `sootie sidecar --health-check --raw` for the raw health JSON.");
}

fn readable_sidecar_health(status: &str) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_str(status).ok()?;
    let model_ready = json_bool(&payload, "model_ready").unwrap_or(false);
    let model_loaded = json_bool(&payload, "model_loaded").unwrap_or(false);
    if let Some(error) = json_str(&payload, "error") {
        return Some(format!("failed: {error}"));
    }
    Some(match (model_ready, model_loaded) {
        (true, true) => "model loaded successfully".to_string(),
        (true, false) => "model files found; model not loaded".to_string(),
        (false, _) => "model files are not ready".to_string(),
    })
}

fn print_string_array(payload: &serde_json::Value, key: &str, title: &str) {
    let Some(values) = payload.get(key).and_then(|value| value.as_array()) else {
        return;
    };
    if values.is_empty() {
        return;
    }
    println!();
    println!("{title}:");
    for value in values.iter().filter_map(|value| value.as_str()) {
        println!("  - {value}");
    }
}

fn print_diagnostics(payload: &serde_json::Value) {
    let Some(diagnostics) = payload
        .get("runtime_diagnostics")
        .and_then(|value| value.as_array())
    else {
        return;
    };
    if diagnostics.is_empty() {
        return;
    }
    println!();
    println!("Diagnostics:");
    for diagnostic in diagnostics {
        let name = diagnostic
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("diagnostic");
        let success = diagnostic
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let message = diagnostic
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        println!("  - {name}: {} {message}", status_label(success));
    }
}

fn setup_config_action(payload: &serde_json::Value) -> &'static str {
    if json_bool(payload, "created").unwrap_or(false) {
        "created"
    } else if json_bool(payload, "overwritten").unwrap_or(false) {
        "updated"
    } else if json_bool(payload, "unchanged").unwrap_or(false) {
        "unchanged"
    } else {
        "checked"
    }
}

fn status_label(ok: bool) -> &'static str {
    if ok {
        "[ok]"
    } else {
        "[not ready]"
    }
}

fn json_bool(value: &serde_json::Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|value| value.as_bool())
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|value| value.as_str())
}

fn json_u64(value: &serde_json::Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|value| value.as_u64())
}

#[derive(Clone, Copy)]
struct SetupProgress {
    enabled: bool,
}

impl SetupProgress {
    fn new(raw: bool) -> Self {
        Self { enabled: !raw }
    }

    fn begin(self) {
        self.line("Starting Sootie setup...");
        self.line("Vision setup can take a while when Python packages or model files need work.");
    }

    fn step(self, message: impl AsRef<str>) {
        self.line(format!("  - {}", message.as_ref()));
    }

    fn ok(self, message: impl AsRef<str>) {
        self.line(format!("    ok {}", message.as_ref()));
    }

    fn line(self, message: impl AsRef<str>) {
        if !self.enabled {
            return;
        }
        println!("{}", message.as_ref());
        let _ = std::io::stdout().flush();
    }
}

struct SetupOptions {
    force: bool,
    skip_sidecar: bool,
    vision_only: bool,
    vision_url: String,
    vision_model_path: Option<PathBuf>,
    vision_model_id: String,
}

struct SetupServeReport {
    ready: bool,
    tool_count: usize,
    status: String,
}

impl SetupServeReport {
    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "ready": self.ready,
            "tool_count": self.tool_count,
            "status": self.status,
        })
    }
}

struct SetupSidecarServerReport {
    ready: bool,
    port: u16,
    already_running: bool,
    model_ready: bool,
    model_loaded: bool,
    status: String,
}

impl SetupSidecarServerReport {
    fn skipped(port: u16) -> Self {
        Self {
            ready: true,
            port,
            already_running: false,
            model_ready: false,
            model_loaded: false,
            status: "sidecar server check skipped".to_string(),
        }
    }

    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "ready": self.ready,
            "port": self.port,
            "already_running": self.already_running,
            "model_ready": self.model_ready,
            "model_loaded": self.model_loaded,
            "status": self.status,
        })
    }
}

fn run_setup(options: SetupOptions, raw: bool) -> anyhow::Result<()> {
    let path = setup_config_path()?;
    let sidecar_dir = sidecar_install_dir()?;
    let model_path = options
        .vision_model_path
        .clone()
        .unwrap_or(default_vision_model_path()?);
    let progress = SetupProgress::new(raw);
    progress.begin();
    let existed = path.exists();
    let mut written = false;
    progress.step(format!("Checking config file: {}", path.display()));
    if !existed || options.force {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &path,
            setup_config_text(
                options.vision_only,
                &options.vision_url,
                &sidecar_dir,
                &model_path,
            ),
        )?;
        written = true;
    }
    if written && existed {
        progress.ok("config updated");
    } else if written {
        progress.ok("config created");
    } else {
        progress.ok("config already exists");
    }

    let sidecar = if options.skip_sidecar {
        progress.step("Skipping vision sidecar setup");
        SidecarSetupReport::skipped(sidecar_dir.clone())
    } else {
        setup_sidecar(
            &sidecar_dir,
            &model_path,
            &options.vision_model_id,
            progress,
        )?
    };

    let sidecar_port = port_from_vision_url(&options.vision_url).unwrap_or(DEFAULT_VISION_PORT);
    let sidecar_server = if options.skip_sidecar {
        progress.step("Skipping vision sidecar server check");
        SetupSidecarServerReport::skipped(sidecar_port)
    } else {
        progress.step(format!(
            "Checking sidecar command can serve HTTP on 127.0.0.1:{sidecar_port}"
        ));
        let report = verify_sidecar_server_ready(&sidecar_dir, &model_path, sidecar_port)?;
        progress.ok(&report.status);
        report
    };

    progress.step("Checking desktop runtime permissions and screenshot access");
    let runtime = runtime_readiness_report();
    if runtime.ready {
        progress.ok("desktop runtime ready");
    } else {
        progress.step(format!(
            "desktop runtime is not ready: {}",
            runtime.blocker_message
        ));
    }

    progress.step("Checking MCP serve startup and tool list");
    let serve = verify_serve_ready()?;
    progress.ok(&serve.status);

    let strategy = if options.vision_only {
        "vision-only"
    } else {
        "platform-first"
    };
    let vision_ready = options.skip_sidecar || (sidecar.ready() && sidecar_server.ready);
    let ready = vision_ready && runtime.ready && serve.ready;
    let payload = serde_json::json!({
        "config_path": path.display().to_string(),
        "created": written && !existed,
        "overwritten": written && existed,
        "unchanged": existed && !options.force,
        "ready": ready,
        "resolution_strategy": strategy,
        "vision_url": options.vision_url,
        "sidecar": sidecar.payload(),
        "sidecar_server": sidecar_server.payload(),
        "runtime": runtime.payload,
        "serve": serve.payload(),
        "next_steps": [
            "Run `sootie sidecar --preload` when vision grounding is needed.",
            "Run `sootie serve` from your MCP client."
        ]
    });
    if raw {
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!();
        print_setup_summary(&payload);
    }
    if !ready {
        anyhow::bail!(
            "setup did not complete: {}",
            setup_failure_message(&payload)
        );
    }
    Ok(())
}

fn verify_serve_ready() -> anyhow::Result<SetupServeReport> {
    let mut server = McpServer::new(create_backend());
    let initialize = server.handle_request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!("setup-initialize")),
        method: "initialize".to_string(),
        params: serde_json::json!({}),
    });
    if let Some(error) = initialize.error {
        anyhow::bail!("MCP initialize failed: {}", error.message);
    }
    let server_name = initialize
        .result
        .as_ref()
        .and_then(|result| result.pointer("/serverInfo/name"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if server_name != "sootie" {
        anyhow::bail!("MCP initialize returned unexpected server name: {server_name}");
    }

    let tools = server.handle_request(JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::json!("setup-tools-list")),
        method: "tools/list".to_string(),
        params: serde_json::json!({}),
    });
    if let Some(error) = tools.error {
        anyhow::bail!("MCP tools/list failed: {}", error.message);
    }
    let tool_count = tools
        .result
        .as_ref()
        .and_then(|result| result.get("tools"))
        .and_then(|value| value.as_array())
        .map(Vec::len)
        .unwrap_or(0);
    if tool_count == 0 {
        anyhow::bail!("MCP tools/list returned no tools");
    }

    Ok(SetupServeReport {
        ready: true,
        tool_count,
        status: format!("MCP serve initialized and listed {tool_count} tools"),
    })
}

fn verify_sidecar_server_ready(
    sidecar_dir: &Path,
    model_path: &Path,
    port: u16,
) -> anyhow::Result<SetupSidecarServerReport> {
    if let Ok(health) = sidecar_http_health(port) {
        return sidecar_server_report_from_health(port, true, health);
    }

    let python = find_sidecar_python().ok_or_else(|| {
        anyhow::anyhow!("Python was not found. Install Python 3 or set SOOTIE_PYTHON.")
    })?;
    let mut child = std::process::Command::new(python)
        .arg(sidecar_dir.join("server.py"))
        .arg("--port")
        .arg(port.to_string())
        .arg("--model-path")
        .arg(model_path)
        .env("SOOTIE_VISION_MODEL_PATH", model_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut last_error = String::new();
    while std::time::Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("vision sidecar exited before becoming ready: {status}");
        }
        match sidecar_http_health(port) {
            Ok(health) => {
                let _ = child.kill();
                let _ = child.wait();
                return sidecar_server_report_from_health(port, false, health);
            }
            Err(error) => {
                last_error = error.to_string();
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    anyhow::bail!("vision sidecar did not become ready on port {port}: {last_error}");
}

fn sidecar_server_report_from_health(
    port: u16,
    already_running: bool,
    health: serde_json::Value,
) -> anyhow::Result<SetupSidecarServerReport> {
    let model_ready = json_bool(&health, "model_ready").unwrap_or(false);
    let model_loaded = json_bool(&health, "model_loaded").unwrap_or(false);
    let model_error = json_str(&health, "model_error").unwrap_or("");
    if !model_ready {
        anyhow::bail!("vision sidecar HTTP health failed: {model_error}");
    }
    let status = if already_running {
        format!("sidecar already running and healthy on 127.0.0.1:{port}")
    } else {
        format!("sidecar command starts and serves health on 127.0.0.1:{port}")
    };
    Ok(SetupSidecarServerReport {
        ready: true,
        port,
        already_running,
        model_ready,
        model_loaded,
        status,
    })
}

fn sidecar_http_health(port: u16) -> anyhow::Result<serde_json::Value> {
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if !response.starts_with("HTTP/1.0 200") && !response.starts_with("HTTP/1.1 200") {
        anyhow::bail!("unexpected HTTP health response");
    }
    let Some((_, body)) = response.split_once("\r\n\r\n") else {
        anyhow::bail!("HTTP health response did not include a body");
    };
    Ok(serde_json::from_str(body.trim())?)
}

fn setup_failure_message(payload: &serde_json::Value) -> String {
    let mut failures = Vec::new();
    let sidecar = payload.get("sidecar").unwrap_or(&serde_json::Value::Null);
    let sidecar_skipped = json_bool(sidecar, "skipped").unwrap_or(false);
    if !sidecar_skipped && !json_bool(sidecar, "ready").unwrap_or(false) {
        failures.push("vision sidecar setup is not ready".to_string());
    }
    if !json_bool(
        payload
            .get("sidecar_server")
            .unwrap_or(&serde_json::Value::Null),
        "ready",
    )
    .unwrap_or(false)
    {
        failures.push("vision sidecar command cannot be verified".to_string());
    }
    if !json_bool(
        payload.get("serve").unwrap_or(&serde_json::Value::Null),
        "ready",
    )
    .unwrap_or(false)
    {
        failures.push("MCP serve cannot be verified".to_string());
    }
    if !json_bool(
        payload.get("runtime").unwrap_or(&serde_json::Value::Null),
        "runtime_ready",
    )
    .unwrap_or(false)
    {
        let blockers = payload
            .get("runtime")
            .and_then(|runtime| runtime.get("runtime_blockers"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "desktop runtime is not ready".to_string());
        failures.push(blockers);
    }
    if failures.is_empty() {
        "unknown setup failure".to_string()
    } else {
        failures.join("; ")
    }
}

fn setup_config_path() -> anyhow::Result<PathBuf> {
    if let Ok(path) = std::env::var("SOOTIE_CONFIG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    dirs_next::home_dir()
        .map(|home| home.join(".config").join("sootie.config.toml"))
        .ok_or_else(|| anyhow::anyhow!("could not resolve home directory for Sootie config"))
}

#[derive(Debug, Default)]
struct SetupConfigValues {
    vision_url: Option<String>,
    sidecar_dir: Option<PathBuf>,
    model_path: Option<PathBuf>,
}

fn load_setup_config_values() -> SetupConfigValues {
    let Ok(path) = setup_config_path() else {
        return SetupConfigValues::default();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return SetupConfigValues::default();
    };
    SetupConfigValues {
        vision_url: config_string_value(&text, "vision", "url"),
        sidecar_dir: config_string_value(&text, "vision", "sidecar_dir").map(PathBuf::from),
        model_path: config_string_value(&text, "vision", "model_path").map(PathBuf::from),
    }
}

fn config_string_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current_section = String::new();
    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_ascii_lowercase();
            continue;
        }
        if current_section != section {
            continue;
        }
        let Some((raw_key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if raw_key.trim().eq_ignore_ascii_case(key) {
            return parse_setup_string_value(raw_value.trim());
        }
    }
    None
}

fn parse_setup_string_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if !trimmed.starts_with('"') {
        return Some(trimmed.to_string()).filter(|value| !value.is_empty());
    }
    let mut output = String::new();
    let mut escaped = false;
    for character in trimmed.trim_start_matches('"').chars() {
        if escaped {
            output.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '"' {
            return Some(output);
        } else {
            output.push(character);
        }
    }
    None
}

fn port_from_vision_url(value: &str) -> Option<u16> {
    let after_scheme = value
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(value);
    let authority = after_scheme.split('/').next().unwrap_or(after_scheme);
    let port = authority.rsplit_once(':')?.1;
    port.parse::<u16>().ok()
}

fn setup_config_text(
    vision_only: bool,
    vision_url: &str,
    sidecar_dir: &Path,
    model_path: &Path,
) -> String {
    let strategy = if vision_only {
        "vision-only"
    } else {
        "platform-first"
    };
    format!(
        r#"# Sootie user config.
# The default strategy uses CDP/native desktop lookup first and vision as fallback.

[resolution]
strategy = "{strategy}"

[vision]
url = "{}"
enabled = true
confidence_threshold = 0.5
timeout_ms = 60000
sidecar_dir = "{}"
model_path = "{}"
"#,
        toml_string_value(vision_url),
        toml_string_value(&sidecar_dir.to_string_lossy()),
        toml_string_value(&model_path.to_string_lossy())
    )
}

fn toml_string_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn setup_data_dir() -> anyhow::Result<PathBuf> {
    dirs_next::data_local_dir()
        .or_else(dirs_next::data_dir)
        .map(|dir| dir.join("sootie"))
        .ok_or_else(|| anyhow::anyhow!("could not resolve Sootie data directory"))
}

fn vision_venv_dir() -> anyhow::Result<PathBuf> {
    Ok(setup_data_dir()?.join("vision-venv"))
}

fn venv_python_path(venv_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        venv_dir.join("Scripts").join("python.exe")
    } else {
        venv_dir.join("bin").join("python")
    }
}

fn sidecar_install_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = load_setup_config_values().sidecar_dir {
        return Ok(path);
    }
    Ok(setup_data_dir()?.join("vision-sidecar"))
}

fn default_vision_model_path() -> anyhow::Result<PathBuf> {
    if let Some(path) = load_setup_config_values().model_path {
        return Ok(path);
    }
    Ok(setup_data_dir()?.join("models").join("ShowUI-2B"))
}

struct SidecarSetupReport {
    dir: PathBuf,
    installed: bool,
    skipped: bool,
    python: Option<PathBuf>,
    dependencies_installed: bool,
    dependencies_ready: bool,
    dependencies_status: String,
    model_downloaded: bool,
    model_id: String,
    model_path: PathBuf,
    model_ready: bool,
    model_status: String,
    preload_checked: bool,
    preload_ready: bool,
    preload_status: String,
}

impl SidecarSetupReport {
    fn skipped(dir: PathBuf) -> Self {
        Self {
            dir,
            installed: false,
            skipped: true,
            python: find_sidecar_python(),
            dependencies_installed: false,
            dependencies_ready: false,
            dependencies_status: "sidecar setup skipped".to_string(),
            model_downloaded: false,
            model_id: DEFAULT_VISION_MODEL_ID.to_string(),
            model_path: PathBuf::new(),
            model_ready: false,
            model_status: "sidecar setup skipped".to_string(),
            preload_checked: false,
            preload_ready: false,
            preload_status: "sidecar setup skipped".to_string(),
        }
    }

    fn ready(&self) -> bool {
        self.installed && self.dependencies_ready && self.model_ready && self.preload_ready
    }

    fn payload(&self) -> serde_json::Value {
        serde_json::json!({
            "dir": self.dir.display().to_string(),
            "server": self.dir.join("server.py").display().to_string(),
            "requirements": self.dir.join("requirements.txt").display().to_string(),
            "installed": self.installed,
            "skipped": self.skipped,
            "python": self.python.as_ref().map(|path| path.display().to_string()),
            "venv_dir": vision_venv_dir().ok().map(|path| path.display().to_string()),
            "dependencies_installed": self.dependencies_installed,
            "dependencies_ready": self.dependencies_ready,
            "dependencies_status": self.dependencies_status,
            "model": {
                "id": self.model_id,
                "path": self.model_path.display().to_string(),
                "ready": self.model_ready,
                "downloaded": self.model_downloaded,
                "status": self.model_status,
            },
            "preload": {
                "checked": self.preload_checked,
                "ready": self.preload_ready,
                "status": self.preload_status,
            },
            "ready": self.ready(),
            "start_command": format!("sootie sidecar --model-path {}", self.model_path.display()),
        })
    }
}

fn setup_sidecar(
    dir: &Path,
    model_path: &Path,
    model_id: &str,
    progress: SetupProgress,
) -> anyhow::Result<SidecarSetupReport> {
    progress.step(format!(
        "Installing vision sidecar files: {}",
        dir.display()
    ));
    install_sidecar_assets(dir)?;
    progress.ok("sidecar files ready");

    progress.step("Looking for Python 3.10-3.13");
    let base_python = find_vision_dependency_python().ok_or_else(|| {
        anyhow::anyhow!(
            "A Python 3.10-3.13 interpreter was not found. Install one before running `sootie setup`."
        )
    })?;
    progress.ok(format!("using {}", base_python.display()));

    let venv_dir = vision_venv_dir()?;
    progress.step(format!(
        "Checking Python environment: {}",
        venv_dir.display()
    ));
    ensure_python_venv(&base_python, &venv_dir)?;
    let python = venv_python_path(&venv_dir);
    progress.ok(format!("Python environment ready at {}", python.display()));
    let mut dependencies_installed = false;
    let mut model_downloaded = false;

    progress.step("Checking vision sidecar Python dependencies");
    let mut dependencies = sidecar_dependency_status(&python);
    if !dependencies.ready {
        progress.step("Installing Python dependencies; this can take several minutes");
        install_python_requirements(&python, &dir.join("requirements.txt"), progress.enabled)?;
        dependencies_installed = true;
        progress.ok("Python dependencies installed");
        progress.step("Rechecking Python dependencies");
        dependencies = sidecar_dependency_status(&python);
        if !dependencies.ready {
            anyhow::bail!(
                "vision sidecar dependencies are still not importable after installation: {}",
                dependencies.status
            );
        }
    }
    progress.ok(&dependencies.status);

    progress.step(format!(
        "Checking vision model files: {}",
        model_path.display()
    ));
    let mut model = vision_model_status(model_path);
    if !model.ready {
        progress.step(format!(
            "Downloading vision model {model_id}; the first run can be large"
        ));
        download_vision_model(
            &python,
            &dir.join("download_model.py"),
            model_id,
            model_path,
            progress.enabled,
        )?;
        model_downloaded = true;
        progress.ok("vision model downloaded");
        progress.step("Rechecking vision model files");
        model = vision_model_status(model_path);
        if !model.ready {
            anyhow::bail!(
                "vision model is still not ready after download: {}",
                model.status
            );
        }
    }
    progress.ok(&model.status);

    progress.step("Preloading vision model; this can take a while on CPU");
    let preload = sidecar_preload_status(&python, dir, model_path);
    if !preload.ready {
        anyhow::bail!("vision sidecar preload failed: {}", preload.status);
    }
    progress.ok(readable_sidecar_health(&preload.status).unwrap_or(preload.status.clone()));

    Ok(SidecarSetupReport {
        dir: dir.to_path_buf(),
        installed: true,
        skipped: false,
        python: Some(python),
        dependencies_installed,
        dependencies_ready: dependencies.ready,
        dependencies_status: dependencies.status,
        model_downloaded,
        model_id: model_id.to_string(),
        model_path: model_path.to_path_buf(),
        model_ready: model.ready,
        model_status: model.status,
        preload_checked: true,
        preload_ready: preload.ready,
        preload_status: preload.status,
    })
}

fn install_sidecar_assets(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    write_setup_file(&dir.join("server.py"), SIDECAR_SERVER_PY)?;
    write_setup_file(&dir.join("requirements.txt"), SIDECAR_REQUIREMENTS_TXT)?;
    write_setup_file(&dir.join("download_model.py"), SIDECAR_DOWNLOAD_MODEL_PY)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            dir.join("server.py"),
            std::fs::Permissions::from_mode(0o755),
        )?;
        std::fs::set_permissions(
            dir.join("download_model.py"),
            std::fs::Permissions::from_mode(0o755),
        )?;
    }
    Ok(())
}

fn write_setup_file(path: &Path, contents: &str) -> anyhow::Result<()> {
    std::fs::write(path, contents)?;
    Ok(())
}

fn find_sidecar_python() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SOOTIE_PYTHON") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if python_works(&candidate) {
                return Some(candidate);
            }
        }
    }
    if let Ok(venv_dir) = vision_venv_dir() {
        let candidate = venv_python_path(&venv_dir);
        if python_works(&candidate) {
            return Some(candidate);
        }
    }
    find_system_python()
}

fn find_system_python() -> Option<PathBuf> {
    [
        "python3.12",
        "python3.11",
        "python3.10",
        "python3.13",
        "python3",
        "python",
        "py",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|candidate| python_works(candidate))
}

fn find_vision_dependency_python() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SOOTIE_PYTHON") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let candidate = PathBuf::from(trimmed);
            if python_supports_vision_deps(&candidate) {
                return Some(candidate);
            }
        }
    }
    [
        "python3.12",
        "python3.11",
        "python3.10",
        "python3.13",
        "python3",
        "python",
        "py",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|candidate| python_supports_vision_deps(candidate))
}

fn python_works(candidate: &Path) -> bool {
    std::process::Command::new(candidate)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn python_supports_vision_deps(candidate: &Path) -> bool {
    python_version(candidate).is_some_and(|(major, minor)| major == 3 && (10..=13).contains(&minor))
}

fn python_version(candidate: &Path) -> Option<(u32, u32)> {
    let output = std::process::Command::new(candidate)
        .arg("--version")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    parse_python_version(&text)
}

fn parse_python_version(text: &str) -> Option<(u32, u32)> {
    let version = text.split_whitespace().find(|part| {
        part.chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    Some((major, minor))
}

fn install_python_requirements(
    python: &Path,
    requirements: &Path,
    stream_output: bool,
) -> anyhow::Result<()> {
    let mut command = std::process::Command::new(python);
    command
        .args(["-m", "pip", "install", "-r"])
        .arg(requirements);
    run_setup_command(
        &mut command,
        stream_output,
        "failed to install vision sidecar Python requirements",
    )
}

fn ensure_python_venv(base_python: &Path, venv_dir: &Path) -> anyhow::Result<()> {
    let python = venv_python_path(venv_dir);
    if python_works(&python) {
        return Ok(());
    }
    if let Some(parent) = venv_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = std::process::Command::new(base_python)
        .args(["-m", "venv"])
        .arg(venv_dir)
        .status()?;
    if !status.success() {
        anyhow::bail!("failed to create Sootie vision Python environment");
    }
    if !python_works(&python) {
        anyhow::bail!("Sootie vision Python environment was created but python is not runnable");
    }
    Ok(())
}

fn download_vision_model(
    python: &Path,
    script: &Path,
    model_id: &str,
    model_path: &Path,
    stream_output: bool,
) -> anyhow::Result<()> {
    let mut command = std::process::Command::new(python);
    command
        .arg(script)
        .arg("--model-id")
        .arg(model_id)
        .arg("--dest")
        .arg(model_path);
    run_setup_command(
        &mut command,
        stream_output,
        &format!("failed to download vision model {model_id}"),
    )
}

fn run_setup_command(
    command: &mut std::process::Command,
    stream_output: bool,
    failure_message: &str,
) -> anyhow::Result<()> {
    if stream_output {
        let status = command.status()?;
        if !status.success() {
            anyhow::bail!("{failure_message}");
        }
        return Ok(());
    }

    let output = command.output()?;
    if !output.status.success() {
        let details = command_output_text(&output);
        if details.is_empty() {
            anyhow::bail!("{failure_message}");
        }
        anyhow::bail!("{failure_message}: {details}");
    }
    Ok(())
}

fn command_output_text(output: &std::process::Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        if !text.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&stderr);
    }
    text
}

struct DependencyStatus {
    ready: bool,
    status: String,
}

fn sidecar_dependency_status(python: &Path) -> DependencyStatus {
    let script = r#"
import importlib
required = [
    ("PIL", "Pillow"),
    ("accelerate", "accelerate"),
    ("huggingface_hub", "huggingface-hub"),
    ("numpy", "numpy"),
    ("qwen_vl_utils", "qwen-vl-utils"),
    ("safetensors", "safetensors"),
    ("torch", "torch"),
    ("torchvision", "torchvision"),
    ("transformers", "transformers"),
]
missing = []
for module, package in required:
    try:
        importlib.import_module(module)
    except Exception as error:
        missing.append(f"{package}: {type(error).__name__}: {error}")
if missing:
    print("; ".join(missing))
    raise SystemExit(1)
from transformers import AutoProcessor, Qwen2VLForConditionalGeneration
print("all required Python packages import successfully")
"#;
    match std::process::Command::new(python)
        .arg("-c")
        .arg(script)
        .output()
    {
        Ok(output) if output.status.success() => DependencyStatus {
            ready: true,
            status: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        },
        Ok(output) => {
            let mut status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                if !status.is_empty() {
                    status.push_str("; ");
                }
                status.push_str(&stderr);
            }
            DependencyStatus {
                ready: false,
                status: if status.is_empty() {
                    "required Python packages are not importable".to_string()
                } else {
                    status
                },
            }
        }
        Err(error) => DependencyStatus {
            ready: false,
            status: format!("failed to run Python dependency check: {error}"),
        },
    }
}

struct PreloadStatus {
    ready: bool,
    status: String,
}

fn sidecar_preload_status(python: &Path, sidecar_dir: &Path, model_path: &Path) -> PreloadStatus {
    match std::process::Command::new(python)
        .arg(sidecar_dir.join("server.py"))
        .arg("--model-path")
        .arg(model_path)
        .arg("--health-check")
        .arg("--preload")
        .output()
    {
        Ok(output) if output.status.success() => PreloadStatus {
            ready: true,
            status: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        },
        Ok(output) => {
            let mut status = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if !stderr.is_empty() {
                if !status.is_empty() {
                    status.push_str("; ");
                }
                status.push_str(&stderr);
            }
            PreloadStatus {
                ready: false,
                status: if status.is_empty() {
                    "sidecar preload check failed".to_string()
                } else {
                    status
                },
            }
        }
        Err(error) => PreloadStatus {
            ready: false,
            status: format!("failed to run sidecar preload check: {error}"),
        },
    }
}

struct ModelStatus {
    ready: bool,
    status: String,
}

fn vision_model_status(path: &Path) -> ModelStatus {
    if !path.exists() {
        return ModelStatus {
            ready: false,
            status: format!("model directory not found: {}", path.display()),
        };
    }
    let has_config = path.join("config.json").exists();
    let has_weights = path_has_file_with_extension(path, "safetensors")
        || path_has_named_prefix(path, "pytorch_model")
        || path_has_file_with_extension(path, "bin")
        || path_has_file_with_extension(path, "gguf");
    match (has_config, has_weights) {
        (true, true) => ModelStatus {
            ready: true,
            status: "model files found".to_string(),
        },
        (false, true) => ModelStatus {
            ready: false,
            status: "config.json missing".to_string(),
        },
        (true, false) => ModelStatus {
            ready: false,
            status: "model weights missing".to_string(),
        },
        (false, false) => ModelStatus {
            ready: false,
            status: "config.json and model weights missing".to_string(),
        },
    }
}

fn path_has_file_with_extension(path: &Path, extension: &str) -> bool {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .any(|entry| entry.path().extension().is_some_and(|ext| ext == extension))
}

fn path_has_named_prefix(path: &Path, prefix: &str) -> bool {
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .any(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
}

fn run_sidecar(
    port: u16,
    model_path: Option<PathBuf>,
    health_check: bool,
    preload: bool,
    raw: bool,
) -> anyhow::Result<()> {
    let config = load_setup_config_values();
    let port = if port == DEFAULT_VISION_PORT {
        config
            .vision_url
            .as_deref()
            .and_then(port_from_vision_url)
            .unwrap_or(port)
    } else {
        port
    };
    let sidecar_dir = sidecar_install_dir()?;
    if !sidecar_dir.join("server.py").exists() {
        install_sidecar_assets(&sidecar_dir)?;
    }
    let python = find_sidecar_python().ok_or_else(|| {
        anyhow::anyhow!("Python was not found. Install Python 3 or set SOOTIE_PYTHON.")
    })?;
    let model_path = model_path
        .or(config.model_path)
        .unwrap_or(default_vision_model_path()?);
    let mut command = std::process::Command::new(python);
    command
        .arg(sidecar_dir.join("server.py"))
        .arg("--port")
        .arg(port.to_string())
        .arg("--model-path")
        .arg(&model_path);
    if health_check {
        command.arg("--health-check");
    }
    if preload {
        command.arg("--preload");
    }
    command.env("SOOTIE_VISION_MODEL_PATH", &model_path);
    if health_check {
        let output = command.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        if raw {
            print!("{stdout}");
        } else {
            let payload: serde_json::Value = serde_json::from_str(stdout.trim())?;
            print_sidecar_health_summary(&payload);
        }
        if !stderr.trim().is_empty() {
            eprintln!("{}", stderr.trim());
        }
        std::io::stdout().flush()?;
        if !output.status.success() {
            anyhow::bail!("vision sidecar exited with status {}", output.status);
        }
        return Ok(());
    }
    let status = command.status()?;
    if !status.success() {
        anyhow::bail!("vision sidecar exited with status {status}");
    }
    Ok(())
}

fn tools_json() -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(
        &sootie_core::tools::tool_definitions(),
    )?)
}

fn resolve_log_file_path(explicit: Option<&Path>) -> anyhow::Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path.to_path_buf()),
        None => default_log_file_path(),
    }
}

fn init_serve_logging(level: &str, explicit_log_file: Option<&Path>) -> anyhow::Result<()> {
    let Ok(log_file) = resolve_log_file_path(explicit_log_file) else {
        return init_logging(level, None);
    };
    let result = init_logging(level, Some(log_file.as_path()));
    if explicit_log_file.is_some() {
        result
    } else {
        result.or_else(|_| init_logging(level, None))
    }
}

fn default_log_file_path() -> anyhow::Result<PathBuf> {
    Ok(default_log_dir().join(format!("{}.log", format_log_timestamp(SystemTime::now()))))
}

fn default_log_dir() -> PathBuf {
    dirs_next::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("sootie")
        .join("logs")
}

fn init_logging(level: &str, log_file: Option<&Path>) -> anyhow::Result<()> {
    let filter = EnvFilter::new(level);
    if let Some(path) = log_file {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file)
            .with_ansi(false)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
    Ok(())
}

fn format_log_timestamp(time: SystemTime) -> String {
    let elapsed = time.duration_since(UNIX_EPOCH).unwrap_or_default();
    let total_seconds = elapsed.as_secs() as i64;
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}-{hour:02}-{minute:02}-{second:02}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month, day)
}

fn platform_notes(platform: &str) -> Vec<&'static str> {
    match platform {
        "linux" => vec![
            "Install xprop, wmctrl, and xdotool for Linux X11 desktop automation.",
            "Install Python 3 AT-SPI bindings such as python3-pyatspi or python3-gi plus gir1.2-atspi-2.0.",
            "Install gnome-screenshot, ImageMagick import, or scrot for screenshots.",
        ],
        "windows" => vec![
            "The Windows backend uses PowerShell, User32, UI Automation, Windows Forms, and System.Drawing.",
            "Run from an interactive desktop session with UI Automation access and a visible top-level window.",
        ],
        "macos" => vec![
            "The macOS backend uses AppKit, Accessibility, CoreGraphics, browser Apple Events where needed, and screencapture.",
            "Grant Accessibility and Screen Recording permissions to the app or terminal that launches Sootie.",
        ],
        _ => vec!["This platform currently exposes only the null backend."],
    }
}

fn runtime_blockers(
    context_app: Option<&str>,
    context_window: Option<&str>,
    context_element_count: usize,
    screenshot_available: bool,
    diagnostics: &[RuntimeDiagnostic],
) -> Vec<String> {
    let mut blockers = Vec::new();
    if context_app.is_none_or(|app| app.trim().is_empty() || app == "unknown") {
        blockers.push("frontmost app is unavailable".to_string());
    }
    if context_window.is_none() && context_element_count == 0 {
        blockers.push("no accessible window or elements".to_string());
    }
    if !screenshot_available {
        blockers.push("screenshot is unavailable".to_string());
    }
    blockers.extend(
        diagnostics
            .iter()
            .filter(|diagnostic| !diagnostic.success)
            .map(|diagnostic| diagnostic.message.clone()),
    );
    blockers
}

fn launch_context_payload() -> serde_json::Value {
    let env = [
        "TERM_PROGRAM",
        "TERM",
        "SHELL",
        "SSH_CONNECTION",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "SESSIONNAME",
    ]
    .into_iter()
    .filter_map(|name| {
        std::env::var(name)
            .ok()
            .map(|value| (name.to_string(), serde_json::Value::String(value)))
    })
    .collect::<serde_json::Map<_, _>>();

    let payload = serde_json::json!({
        "pid": std::process::id(),
        "executable": std::env::current_exe().ok().map(|path| path.display().to_string()),
        "current_dir": std::env::current_dir().ok().map(|path| path.display().to_string()),
        "env": env,
    });

    launch_context_with_platform_details(payload)
}

#[cfg(unix)]
fn launch_context_with_platform_details(mut payload: serde_json::Value) -> serde_json::Value {
    if let Some(parent) = unix_parent_process(std::process::id()) {
        payload["parent_process"] = parent;
    }
    payload
}

#[cfg(not(unix))]
fn launch_context_with_platform_details(payload: serde_json::Value) -> serde_json::Value {
    payload
}

#[cfg(unix)]
fn unix_parent_process(pid: u32) -> Option<serde_json::Value> {
    let ppid_output = std::process::Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !ppid_output.status.success() {
        return None;
    }
    let ppid = String::from_utf8_lossy(&ppid_output.stdout)
        .trim()
        .parse::<u32>()
        .ok()?;
    let command_output = std::process::Command::new("ps")
        .args(["-o", "comm=", "-p", &ppid.to_string()])
        .output()
        .ok();
    let command = command_output.and_then(|output| {
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|value| !value.is_empty())
    });
    Some(serde_json::json!({
        "pid": ppid,
        "command": command,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn formats_default_log_names_as_sortable_timestamps() {
        assert_eq!(format_log_timestamp(UNIX_EPOCH), "1970-01-01-00-00-00");
        assert_eq!(
            format_log_timestamp(UNIX_EPOCH + Duration::from_secs(86_400 + 3_661)),
            "1970-01-02-01-01-01"
        );
    }

    #[test]
    fn default_log_path_uses_platform_data_logs_dir() {
        let path = default_log_file_path().unwrap();
        assert_eq!(path.file_name().unwrap().to_string_lossy().len(), 23);
        assert_eq!(path.extension().unwrap(), "log");
        assert!(path.ends_with(
            Path::new("sootie")
                .join("logs")
                .join(path.file_name().unwrap())
        ));
    }

    #[test]
    fn explicit_log_path_is_preserved() {
        let explicit = Path::new("/tmp/sootie-test.log");
        assert_eq!(resolve_log_file_path(Some(explicit)).unwrap(), explicit);
    }

    #[test]
    fn cli_accepts_documented_commands() {
        let serve = Cli::try_parse_from(["sootie", "serve"]).unwrap();
        assert!(matches!(serve.command, Command::Serve));

        let setup = Cli::try_parse_from(["sootie", "setup"]).unwrap();
        assert!(matches!(
            setup.command,
            Command::Setup {
                force: false,
                vision_only: false,
                full: false,
                ..
            }
        ));

        let sidecar = Cli::try_parse_from(["sootie", "sidecar", "--health-check"]).unwrap();
        assert!(matches!(
            sidecar.command,
            Command::Sidecar {
                port: DEFAULT_VISION_PORT,
                health_check: true,
                ..
            }
        ));

        let doctor = Cli::try_parse_from(["sootie", "doctor"]).unwrap();
        assert!(matches!(doctor.command, Command::Doctor { check: false }));

        let doctor_check = Cli::try_parse_from(["sootie", "doctor", "--check"]).unwrap();
        assert!(matches!(
            doctor_check.command,
            Command::Doctor { check: true }
        ));

        let tools = Cli::try_parse_from(["sootie", "tools"]).unwrap();
        assert!(matches!(tools.command, Command::Tools));

        let raw_tools_before = Cli::try_parse_from(["sootie", "--raw", "tools"]).unwrap();
        assert!(raw_tools_before.raw);
        assert!(matches!(raw_tools_before.command, Command::Tools));

        let raw_tools_after = Cli::try_parse_from(["sootie", "tools", "--raw"]).unwrap();
        assert!(raw_tools_after.raw);
        assert!(matches!(raw_tools_after.command, Command::Tools));
    }

    #[test]
    fn setup_config_defaults_to_platform_first_with_vision_fallback() {
        let text = setup_config_text(
            false,
            "http://127.0.0.1:9876",
            Path::new("/tmp/sidecar"),
            Path::new("/tmp/model"),
        );

        assert!(text.contains("strategy = \"platform-first\""));
        assert!(text.contains("url = \"http://127.0.0.1:9876\""));
        assert!(text.contains("enabled = true"));
        assert!(text.contains("sidecar_dir = \"/tmp/sidecar\""));
        assert!(text.contains("model_path = \"/tmp/model\""));
    }

    #[test]
    fn setup_config_can_force_vision_only() {
        let text = setup_config_text(
            true,
            "http://localhost:9999/vision",
            Path::new("/tmp/sidecar"),
            Path::new("/tmp/model"),
        );

        assert!(text.contains("strategy = \"vision-only\""));
        assert!(text.contains("url = \"http://localhost:9999/vision\""));
    }

    #[test]
    fn setup_failure_message_ignores_skipped_sidecar() {
        let payload = serde_json::json!({
            "sidecar": { "ready": false, "skipped": true },
            "sidecar_server": { "ready": true },
            "serve": { "ready": true },
            "runtime": {
                "runtime_ready": false,
                "runtime_blockers": ["desktop runtime is not ready"]
            }
        });

        let message = setup_failure_message(&payload);

        assert!(!message.contains("vision sidecar setup is not ready"));
        assert_eq!(message, "desktop runtime is not ready");
    }

    #[test]
    fn model_status_detects_ready_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();
        std::fs::write(dir.path().join("model.safetensors"), "weights").unwrap();

        let status = vision_model_status(dir.path());

        assert!(status.ready);
        assert_eq!(status.status, "model files found");
    }

    #[test]
    fn model_status_reports_missing_weights() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), "{}").unwrap();

        let status = vision_model_status(dir.path());

        assert!(!status.ready);
        assert_eq!(status.status, "model weights missing");
    }

    #[test]
    fn toml_string_value_escapes_quotes_and_backslashes() {
        assert_eq!(
            toml_string_value(r#"http://localhost/"model"\path"#),
            r#"http://localhost/\"model\"\\path"#
        );
    }

    #[test]
    fn setup_config_parser_reads_vision_paths_and_url() {
        let text = r#"
            [vision]
            url = "http://127.0.0.1:9888"
            sidecar_dir = "/tmp/sidecar"
            model_path = "/tmp/model"
        "#;

        assert_eq!(
            config_string_value(text, "vision", "url").as_deref(),
            Some("http://127.0.0.1:9888")
        );
        assert_eq!(
            config_string_value(text, "vision", "sidecar_dir").as_deref(),
            Some("/tmp/sidecar")
        );
        assert_eq!(
            config_string_value(text, "vision", "model_path").as_deref(),
            Some("/tmp/model")
        );
    }

    #[test]
    fn port_from_vision_url_reads_authority_port() {
        assert_eq!(port_from_vision_url("http://127.0.0.1:9888"), Some(9888));
        assert_eq!(
            port_from_vision_url("http://127.0.0.1:9888/path"),
            Some(9888)
        );
        assert_eq!(port_from_vision_url("http://127.0.0.1"), None);
    }

    #[test]
    fn python_version_parser_reads_major_minor() {
        assert_eq!(parse_python_version("Python 3.12.7"), Some((3, 12)));
        assert_eq!(parse_python_version("Python 3.14.4"), Some((3, 14)));
        assert_eq!(parse_python_version("not python"), None);
    }

    #[test]
    fn tools_json_preserves_mcp_annotations() {
        let payload: serde_json::Value = serde_json::from_str(&tools_json().unwrap()).unwrap();
        let tools = payload.as_array().unwrap();
        assert_eq!(tools.len(), sootie_core::tools::TOOL_NAMES.len());

        let missing_annotations = tools
            .iter()
            .filter_map(|tool| {
                let name = tool["name"].as_str().unwrap_or("<unknown>");
                let annotations = &tool["annotations"];
                let complete = annotations["readOnlyHint"].is_boolean()
                    && annotations["destructiveHint"].is_boolean()
                    && annotations["idempotentHint"].is_boolean()
                    && annotations["openWorldHint"].is_boolean();
                (!complete).then_some(name)
            })
            .collect::<Vec<_>>();
        assert!(missing_annotations.is_empty(), "{missing_annotations:?}");

        let status = tools
            .iter()
            .find(|tool| tool["name"] == "learn_status")
            .unwrap();
        assert_eq!(status["annotations"]["readOnlyHint"].as_bool(), Some(true));

        let click = tools.iter().find(|tool| tool["name"] == "click").unwrap();
        assert_eq!(
            click["annotations"]["destructiveHint"].as_bool(),
            Some(true)
        );
    }

    #[test]
    fn sidecar_health_json_gets_readable_summary() {
        assert_eq!(
            readable_sidecar_health(
                r#"{"model_ready": true, "model_loaded": true, "model_path": "/tmp/model", "error": null}"#
            )
            .as_deref(),
            Some("model loaded successfully")
        );
    }

    #[test]
    fn linux_doctor_notes_describe_current_backend_requirements() {
        let notes = platform_notes("linux");
        assert!(notes.iter().any(|note| note.contains("xprop")));
        assert!(notes.iter().any(|note| note.contains("AT-SPI")));
        assert!(notes.iter().any(|note| note.contains("scrot")));
    }

    #[test]
    fn windows_doctor_notes_describe_current_backend_requirements() {
        let notes = platform_notes("windows");
        assert!(notes.iter().any(|note| note.contains("UI Automation")));
        assert!(notes.iter().any(|note| note.contains("Windows Forms")));
        assert!(notes
            .iter()
            .any(|note| note.contains("visible top-level window")));
        assert!(!notes.iter().any(|note| note.contains("next backend layer")));
    }

    #[test]
    fn macos_doctor_notes_describe_permission_requirements() {
        let notes = platform_notes("macos");
        assert!(notes.iter().any(|note| note.contains("AppKit")));
        assert!(notes.iter().any(|note| note.contains("CoreGraphics")));
        assert!(notes.iter().any(|note| note.contains("Accessibility")));
        assert!(notes.iter().any(|note| note.contains("Screen Recording")));
    }

    #[test]
    fn launch_context_reports_process_identity() {
        let payload = launch_context_payload();
        assert_eq!(payload["pid"].as_u64(), Some(std::process::id() as u64));
        assert!(payload["executable"].is_string() || payload["executable"].is_null());
        assert!(payload["current_dir"].is_string() || payload["current_dir"].is_null());
        assert!(payload["env"].is_object());
    }

    #[test]
    fn runtime_blockers_report_empty_context_and_screenshot_failures() {
        let blockers = runtime_blockers(Some("unknown"), None, 0, false, &[]);
        assert_eq!(
            blockers,
            vec![
                "frontmost app is unavailable".to_string(),
                "no accessible window or elements".to_string(),
                "screenshot is unavailable".to_string()
            ]
        );
    }

    #[test]
    fn runtime_blockers_pass_ready_context() {
        let blockers = runtime_blockers(Some("TextEdit"), Some("Document"), 1, true, &[]);
        assert!(blockers.is_empty());
    }

    #[test]
    fn runtime_blockers_include_failed_diagnostics() {
        let diagnostics = vec![RuntimeDiagnostic {
            name: "macos_accessibility".to_string(),
            success: false,
            message: "macOS Accessibility denied for the Sootie launch path".to_string(),
            details: None,
        }];
        let blockers = runtime_blockers(Some("TextEdit"), Some("Document"), 1, true, &diagnostics);
        assert_eq!(
            blockers,
            vec!["macOS Accessibility denied for the Sootie launch path".to_string()]
        );
    }
}
