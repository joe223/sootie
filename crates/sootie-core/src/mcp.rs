use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::backend::DesktopBackend;
use crate::browser::BrowserService;
use crate::config::{ResolutionStrategy, SootieConfig};
use crate::recipe::{
    parse_recipe, recipe_step_tool_call, recipe_wait_tool_call, substitute_params, Recipe,
    RecipeStore,
};
use crate::recipe_learning::learned_recipe_from_actions;
use crate::recipe_runtime::{
    recipe_coordinate_fallback_args, recipe_coordinate_fallback_reason,
    recipe_error_coordinate_fallback_reason, recipe_primary_dispatch_args,
    resolve_recipe_coordinate_spaces,
};
use crate::tools::tool_definitions;
use crate::types::{
    ActionResult, AppInfo, Bounds, ElementInfo, FindQuery, RuntimeDiagnostic, Screenshot,
    SootieError, SootieResult, ToolResult, WindowCommand, WindowInfo,
};
use crate::vision::{self, GroundRequest, GroundResult, VisionConfig};

#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

pub struct McpServer {
    backend: Box<dyn DesktopBackend>,
    recipes: RecipeStore,
    learning_session: Option<LearningSession>,
    resolution_strategy: ResolutionStrategy,
    vision: VisionConfig,
    browser: BrowserService,
}

struct LearningSession {
    started: Instant,
    task: String,
    events: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StdioEncoding {
    LineJson,
    ContentLength,
}

struct StdioMessage {
    encoding: StdioEncoding,
    body: String,
}

static IMAGE_ARTIFACT_COUNTER: AtomicU64 = AtomicU64::new(0);
const VISION_HISTORY_ROOT_DIR: &str = "/tmp/sootie/vision_history";
const VISION_GROUNDING_HISTORY_DIR: &str = "/tmp/sootie/vision_history/grounding";
const VISION_GROUNDING_SCREENSHOT_MIME_TYPE: &str = "image/jpeg";
const VISION_GROUNDING_JPEG_QUALITY: u8 = 90;
const MAX_LEARNED_CLIPBOARD_TEXT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
struct VisionFrame {
    offset_x: f64,
    offset_y: f64,
    width: f64,
    height: f64,
}

impl VisionFrame {
    fn from_screenshot(screenshot: &Screenshot) -> Self {
        if let Some(frame) = &screenshot.window_frame {
            if frame.width > 0.0 && frame.height > 0.0 {
                return Self {
                    offset_x: frame.x,
                    offset_y: frame.y,
                    width: frame.width,
                    height: frame.height,
                };
            }
        }
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            width: screenshot.width.unwrap_or(1).max(1) as f64,
            height: screenshot.height.unwrap_or(1).max(1) as f64,
        }
    }

    fn local_crop_box(&self, crop_box: (f64, f64, f64, f64)) -> Option<[f64; 4]> {
        let (mut x1, mut y1, mut x2, mut y2) = crop_box;
        let overlaps_screen_frame = x2 > self.offset_x
            && y2 > self.offset_y
            && x1 < self.offset_x + self.width
            && y1 < self.offset_y + self.height
            && (self.offset_x != 0.0 || self.offset_y != 0.0);
        if overlaps_screen_frame {
            x1 -= self.offset_x;
            x2 -= self.offset_x;
            y1 -= self.offset_y;
            y2 -= self.offset_y;
        }
        x1 = x1.clamp(0.0, self.width);
        x2 = x2.clamp(0.0, self.width);
        y1 = y1.clamp(0.0, self.height);
        y2 = y2.clamp(0.0, self.height);
        (x2 > x1 && y2 > y1).then_some([x1, y1, x2, y2])
    }

    fn payload(&self) -> Value {
        json!({
            "x": self.offset_x,
            "y": self.offset_y,
            "width": self.width,
            "height": self.height,
        })
    }
}

#[derive(Debug, Clone)]
struct VisionMappedGrounding {
    x: f64,
    y: f64,
    result: GroundResult,
    frame: VisionFrame,
    screenshot: Screenshot,
    history: Option<VisionHistoryArtifact>,
}

impl VisionMappedGrounding {
    fn new(
        result: GroundResult,
        frame: VisionFrame,
        screenshot: Screenshot,
        history: Option<VisionHistoryArtifact>,
    ) -> Self {
        Self {
            x: result.x + frame.offset_x,
            y: result.y + frame.offset_y,
            result,
            frame,
            screenshot,
            history,
        }
    }

    fn synthetic_bounds(&self) -> Bounds {
        self.result
            .bounds
            .as_ref()
            .map(|bounds| Bounds {
                x: bounds.x + self.frame.offset_x,
                y: bounds.y + self.frame.offset_y,
                width: bounds.width,
                height: bounds.height,
            })
            .unwrap_or(Bounds {
                x: self.x - 20.0,
                y: self.y - 20.0,
                width: 40.0,
                height: 40.0,
            })
    }
}

#[derive(Debug, Clone)]
struct VisionHistoryArtifact {
    image_path: String,
    image_uri: String,
    metadata_path: String,
    metadata_uri: String,
    image_mime_type: &'static str,
}

#[derive(Debug, Clone)]
struct GroundingAnnotation {
    index: usize,
    bounds: Bounds,
    value: String,
    confidence: Option<f64>,
    label: Option<String>,
}

struct RecipeDispatchOutcome {
    result: ToolResult,
    fallback_used: bool,
    fallback_reason: Option<String>,
}

fn suppress_recipe_action_context(tool: &str, args: &mut Value) {
    if !matches!(
        tool,
        "sootie_click"
            | "sootie_hover"
            | "sootie_long_press"
            | "sootie_drag"
            | "sootie_type"
            | "sootie_press"
            | "sootie_hotkey"
            | "sootie_scroll"
            | "sootie_focus"
            | "sootie_window"
    ) {
        return;
    }
    let Value::Object(map) = args else {
        return;
    };
    map.entry("__include_context".to_string())
        .or_insert_with(|| json!(false));
}

fn stdio_encoding_name(encoding: StdioEncoding) -> &'static str {
    match encoding {
        StdioEncoding::LineJson => "line-json",
        StdioEncoding::ContentLength => "content-length",
    }
}

impl McpServer {
    pub fn new(backend: Box<dyn DesktopBackend>) -> Self {
        let config = SootieConfig::load();
        Self {
            backend,
            recipes: RecipeStore::default(),
            learning_session: None,
            resolution_strategy: config.resolution.strategy,
            vision: VisionConfig::from_env_and_settings(&config.vision),
            browser: BrowserService::default(),
        }
    }

    pub fn with_recipe_store(backend: Box<dyn DesktopBackend>, recipes: RecipeStore) -> Self {
        let config = SootieConfig::load();
        Self {
            backend,
            recipes,
            learning_session: None,
            resolution_strategy: config.resolution.strategy,
            vision: VisionConfig::from_env_and_settings(&config.vision),
            browser: BrowserService::default(),
        }
    }

    #[cfg(test)]
    fn with_runtime_config(
        backend: Box<dyn DesktopBackend>,
        resolution_strategy: ResolutionStrategy,
        vision: VisionConfig,
    ) -> Self {
        Self {
            backend,
            recipes: RecipeStore::default(),
            learning_session: None,
            resolution_strategy,
            vision,
            browser: BrowserService::default(),
        }
    }

    #[cfg(test)]
    fn with_vision_config(backend: Box<dyn DesktopBackend>, vision: VisionConfig) -> Self {
        Self::with_runtime_config(backend, ResolutionStrategy::PlatformFirst, vision)
    }

    pub fn serve_stdio(&mut self) -> anyhow::Result<()> {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout().lock();
        self.serve_reader_writer(stdin.lock(), stdout)
    }

    fn serve_reader_writer<R: BufRead, W: Write>(
        &mut self,
        mut reader: R,
        mut writer: W,
    ) -> anyhow::Result<()> {
        while let Some(message) = read_stdio_message(&mut reader)? {
            let response = match serde_json::from_str::<JsonRpcRequest>(&message.body) {
                Ok(request) => {
                    tracing::info!(
                        method = request.method.as_str(),
                        has_id = request.id.is_some(),
                        encoding = stdio_encoding_name(message.encoding),
                        "MCP request received"
                    );
                    self.handle_request(request)
                }
                Err(error) => {
                    tracing::warn!(%error, "invalid MCP JSON-RPC request");
                    JsonRpcResponse {
                        jsonrpc: "2.0".to_string(),
                        id: None,
                        result: None,
                        error: Some(RpcError {
                            code: -32700,
                            message: format!("Parse error: {error}"),
                        }),
                    }
                }
            };
            if response.id.is_some() || response.error.is_some() {
                write_stdio_response(&mut writer, message.encoding, &response)?;
                writer.flush()?;
            }
        }
        tracing::info!("MCP stdin closed");
        Ok(())
    }

    pub fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "sootie",
                    "version": env!("CARGO_PKG_VERSION"),
                    "platform": self.backend.platform()
                },
                "capabilities": {
                    "tools": { "listChanged": false }
                }
            })),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => self.handle_tool_call(&request.params),
            "notifications/initialized" => Ok(Value::Null),
            _ => Err(SootieError::Unsupported(format!(
                "unknown JSON-RPC method '{}'",
                request.method
            ))),
        };

        match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id,
                result: None,
                error: Some(RpcError {
                    code: -32000,
                    message: error.to_string(),
                }),
            },
        }
    }

    fn handle_tool_call(&mut self, params: &Value) -> SootieResult<Value> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| SootieError::InvalidArguments("tools/call requires name".to_string()))?;
        let args = tool_call_args(name, params);
        let started = Instant::now();
        let result =
            validate_public_tool_args(name, &args).and_then(|_| self.dispatch(name, &args));
        let elapsed_ms = started.elapsed().as_millis();
        match &result {
            Ok(result) => tracing::info!(
                tool = name,
                success = result.success,
                elapsed_ms,
                "Sootie tool call completed"
            ),
            Err(error) => tracing::warn!(
                tool = name,
                elapsed_ms,
                %error,
                "Sootie tool call failed"
            ),
        }
        self.record_learning_event(name, &args, &result);
        Ok(format_tool_result(name, args, result, elapsed_ms))
    }

    fn record_learning_event(
        &mut self,
        name: &str,
        args: &Value,
        result: &SootieResult<ToolResult>,
    ) {
        if name.starts_with("sootie_learn_") {
            return;
        }
        if self.learning_session.is_none() {
            return;
        }
        if !matches!(result, Ok(result) if result.success) {
            return;
        }
        if let Some(mut event) = learned_action_event(name, args) {
            if let Ok(result) = result {
                enrich_learned_event_from_context(&mut event, result.context.as_ref());
            }
            self.enrich_learned_event_coordinates(&mut event);
            self.enrich_learned_event_clipboard(&mut event);
            if let Some(session) = &mut self.learning_session {
                session.events.push(event);
            }
        }
    }

    fn enrich_learned_event_coordinates(&self, event: &mut Value) {
        let Some(app) = event
            .get("app")
            .and_then(Value::as_str)
            .filter(|app| !app.is_empty())
            .map(str::to_string)
        else {
            return;
        };
        let window = event
            .get("window")
            .and_then(Value::as_str)
            .filter(|window| !window.is_empty());
        let Ok(Some(bounds)) = self.current_window_bounds(&app, window) else {
            return;
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return;
        }
        event["window_frame"] = json!({
            "x": bounds.x,
            "y": bounds.y,
            "width": bounds.width,
            "height": bounds.height,
        });
        enrich_point_coordinate_fields(event, &bounds, "x", "y", "");
        enrich_point_coordinate_fields(event, &bounds, "from_x", "from_y", "from_");
        enrich_point_coordinate_fields(event, &bounds, "to_x", "to_y", "to_");
    }

    fn enrich_learned_event_clipboard(&self, event: &mut Value) {
        if !learned_event_is_paste_hotkey(event) {
            return;
        }
        let Ok(text) = self.backend.clipboard_text() else {
            return;
        };
        if text.is_empty() || text.len() > MAX_LEARNED_CLIPBOARD_TEXT_BYTES {
            return;
        }
        event["clipboard_text"] = json!(text);
    }

    fn dispatch(&mut self, name: &str, args: &Value) -> SootieResult<ToolResult> {
        match name {
            "sootie_context" => {
                let app = optional_app_arg(args);
                let context = match self.backend.context(app.as_deref()) {
                    Ok(context) => context,
                    Err(error) => {
                        let diagnostics = self.backend.diagnostics();
                        return Ok(tool_error_with_runtime_diagnostic(
                            error.to_string(),
                            &diagnostics,
                        ));
                    }
                };
                let diagnostics =
                    if context.window.is_none() && context.interactive_elements.is_empty() {
                        self.backend.diagnostics()
                    } else {
                        Vec::new()
                    };
                Ok(context_tool_result(
                    context,
                    self.backend.platform(),
                    app.as_deref(),
                    &diagnostics,
                ))
            }
            "sootie_state" => {
                let app = optional_app_arg(args);
                let apps = self.backend.state(app.as_deref())?;
                let app_payloads = apps.iter().map(app_payload).collect::<Vec<_>>();
                let data = json!({ "apps": app_payloads, "app_count": apps.len() });
                Ok(ToolResult::ok(data))
            }
            "sootie_find" => {
                let query = find_query(args);
                let elements = self.find_elements(&query)?;
                let total_matches = elements.len();
                let elements = if let Some(max_results) = query.max_results {
                    elements
                        .into_iter()
                        .take(max_results as usize)
                        .collect::<Vec<_>>()
                } else {
                    elements
                };
                let summaries = elements.iter().map(element_summary).collect::<Vec<_>>();
                let mut result = ToolResult::ok(json!({
                    "elements": summaries,
                    "count": elements.len(),
                    "total_matches": total_matches,
                }));
                if elements.is_empty() && !self.resolution_strategy.is_vision_only() {
                    if let Ok(context) = self.backend.context(query.app.as_deref()) {
                        if context.window.is_none() && context.interactive_elements.is_empty() {
                            result = result.with_suggestion(empty_context_suggestion(
                                self.backend.platform(),
                                context.app.as_deref().or(query.app.as_deref()),
                                &[],
                            ));
                        }
                    }
                }
                Ok(result)
            }
            "sootie_read" => {
                let query = find_query(args);
                let content = self.backend.read(
                    query.app.as_deref(),
                    query.query.as_deref(),
                    u32_arg(args, "depth"),
                )?;
                let item_count = content.lines().filter(|line| !line.is_empty()).count();
                Ok(ToolResult::ok(json!({
                    "content": content,
                    "item_count": item_count,
                })))
            }
            "sootie_inspect" => {
                let query = find_query(args);
                match self.inspect_element(&query)? {
                    Some(element) => Ok(ToolResult::ok(element_full(&element))),
                    None => Ok(ToolResult::error(format!(
                        "Element '{}' not found",
                        target_label(args).unwrap_or_else(|| "target".to_string())
                    ))),
                }
            }
            "sootie_element_at" => match self
                .backend
                .element_at(required_f64(args, "x")?, required_f64(args, "y")?)?
            {
                Some(element) => Ok(ToolResult::ok(element_full(&element))),
                None => Ok(ToolResult::error(format!(
                    "No element found at ({}, {})",
                    required_f64(args, "x")?,
                    required_f64(args, "y")?
                ))
                .with_suggestion(element_at_miss_suggestion(self.backend.platform()))),
            },
            "sootie_screenshot" => {
                let app = optional_app_arg(args);
                let window = screenshot_window_arg(args, app.as_deref())?;
                Ok(screenshot_tool_result(self.backend.screenshot(
                    app.as_deref(),
                    window.as_deref(),
                    bool_arg(args, "full_resolution").unwrap_or(false),
                )))
            }
            "sootie_annotate" => self.annotate(args),
            "sootie_ground" => self.ground(args),
            "sootie_parse_screen" => {
                let app = optional_app_arg(args);
                let window = screenshot_window_arg(args, app.as_deref())?;
                let context = self.backend.context(app.as_deref())?;
                let screenshot = match self.backend.screenshot(
                    app.as_deref(),
                    window.as_deref(),
                    bool_arg(args, "full_resolution").unwrap_or(false),
                ) {
                    Ok(screenshot) => screenshot,
                    Err(error) => return Ok(screenshot_error_result(error)),
                };
                let mut payload = screenshot_payload(screenshot);
                payload["elements"] = json!(context.interactive_elements);
                payload["element_count"] = payload["elements"]
                    .as_array()
                    .map(|elements| json!(elements.len()))
                    .unwrap_or_else(|| json!(0));
                payload["source"] = json!("platform-context");
                Ok(ToolResult::ok(payload))
            }
            "sootie_browser_connect" => Ok(ToolResult::ok(self.browser.connect(args)?)),
            "sootie_browser_pages" => Ok(ToolResult::ok(self.browser.pages(args)?)),
            "sootie_browser_select_page" => Ok(ToolResult::ok(self.browser.select_page(args)?)),
            "sootie_browser_open" => Ok(ToolResult::ok(self.browser.open(args)?)),
            "sootie_browser_observe" => {
                let mut payload = self.browser.observe(args)?;
                let mode = str_arg(args, "mode").unwrap_or_default();
                let include_screenshot =
                    nested_bool_arg(args, "include", "screenshot").unwrap_or(false);
                if matches!(mode.as_str(), "screenshot" | "hybrid") || include_screenshot {
                    let screenshot = self.browser.screenshot(args)?;
                    payload["screenshot"] = screenshot_payload(screenshot);
                }
                Ok(ToolResult::ok(payload))
            }
            "sootie_browser_find" => Ok(ToolResult::ok(self.browser.find(args)?)),
            "sootie_browser_click" => Ok(ToolResult::ok(self.browser.click(args)?)),
            "sootie_browser_type" => Ok(ToolResult::ok(self.browser.type_text(args)?)),
            "sootie_browser_press" => Ok(ToolResult::ok(self.browser.press(args)?)),
            "sootie_browser_scroll" => Ok(ToolResult::ok(self.browser.scroll(args)?)),
            "sootie_browser_wait" => Ok(ToolResult::ok(self.browser.wait(args)?)),
            "sootie_browser_extract" => Ok(ToolResult::ok(self.browser.extract(args)?)),
            "sootie_browser_screenshot" => {
                Ok(screenshot_tool_result(self.browser.screenshot(args)))
            }
            "sootie_browser_back" => Ok(ToolResult::ok(self.browser.history(args, "back")?)),
            "sootie_browser_forward" => Ok(ToolResult::ok(self.browser.history(args, "forward")?)),
            "sootie_browser_reload" => Ok(ToolResult::ok(self.browser.history(args, "reload")?)),
            "sootie_browser_close_page" => Ok(ToolResult::ok(self.browser.close_page(args)?)),
            "sootie_browser_network" => Ok(ToolResult::ok(self.browser.network(args)?)),
            "sootie_browser_console" => Ok(ToolResult::ok(self.browser.console(args)?)),
            "sootie_browser_storage" => Ok(ToolResult::ok(self.browser.storage(args)?)),
            "sootie_browser_cookies" => Ok(ToolResult::ok(self.browser.cookies(args)?)),
            "sootie_browser_downloads" => Ok(ToolResult::ok(self.browser.downloads(args)?)),
            "sootie_browser_upload" => Ok(ToolResult::ok(self.browser.upload(args)?)),
            "sootie_browser_pdf" => Ok(ToolResult::ok(self.browser.pdf(args)?)),
            "sootie_cdp_send" => Ok(ToolResult::ok(self.browser.cdp_send(args)?)),
            "sootie_cdp_subscribe" => Ok(ToolResult::ok(self.browser.cdp_subscribe(args)?)),
            "sootie_click" => {
                let query = find_query(args);
                let (x, y) = xy_args(args, "x", "y", "target")?;
                let button = mouse_button_arg(args)?;
                let count = positive_u32_arg(args, "count", 1)?;
                if let Some(result) = self.vision_first_click(x, y, &query, &button, count)? {
                    return Ok(self.action_result_for_args(result, query.app.as_deref(), args));
                }
                self.focus_app_for_explicit_pointer_coordinates(&query, x.zip(y))?;
                let result = match self.backend.click(x, y, &query, &button, count) {
                    Ok(result) => result,
                    Err(error) => {
                        match self.vision_fallback_click(&error, x, y, &query, &button, count)? {
                            Some(result) => result,
                            None => return Err(error),
                        }
                    }
                };
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_hover" => {
                let query = find_query(args);
                let (x, y) = xy_args(args, "x", "y", "target")?;
                if let Some(result) = self.vision_first_hover(x, y, &query)? {
                    return Ok(self.action_result_for_args(result, query.app.as_deref(), args));
                }
                self.focus_app_for_explicit_pointer_coordinates(&query, x.zip(y))?;
                let result = match self.backend.hover(x, y, &query) {
                    Ok(result) => result,
                    Err(error) => match self.vision_fallback_hover(&error, x, y, &query)? {
                        Some(result) => result,
                        None => return Err(error),
                    },
                };
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_long_press" => {
                let query = find_query(args);
                let (x, y) = xy_args(args, "x", "y", "target")?;
                let duration_secs = non_negative_seconds_arg(args, "duration", "duration_ms", 1.0)?;
                let button = mouse_button_arg(args)?;
                if let Some(result) =
                    self.vision_first_long_press(x, y, &query, duration_secs, &button)?
                {
                    return Ok(self.action_result_for_args(result, query.app.as_deref(), args));
                }
                self.focus_app_for_explicit_pointer_coordinates(&query, x.zip(y))?;
                let result = match self
                    .backend
                    .long_press(x, y, &query, duration_secs, &button)
                {
                    Ok(result) => result,
                    Err(error) => {
                        match self.vision_fallback_long_press(
                            &error,
                            x,
                            y,
                            &query,
                            duration_secs,
                            &button,
                        )? {
                            Some(result) => result,
                            None => return Err(error),
                        }
                    }
                };
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_drag" => {
                let query = find_query_with_target(args, "from_target");
                let from = match self.optional_point_arg(args, "from_x", "from_y", "from_target") {
                    Ok(point) => point,
                    Err(SootieError::NotFound(_)) if query_has_target(&query) => None,
                    Err(error) => return Err(error),
                };
                let to = self.required_point_arg(args, "to_x", "to_y", "to_target")?;
                self.focus_app_for_explicit_pointer_coordinates(&query, from)?;
                let result = self.backend.drag(
                    from,
                    to,
                    &query,
                    non_negative_seconds_arg(args, "duration", "duration_ms", 0.5)?,
                    non_negative_seconds_arg(args, "hold_duration", "hold_duration_ms", 0.1)?,
                )?;
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_type" => {
                let mut query = find_query(args);
                query.query = query.query.or_else(|| str_arg(args, "into"));
                let result = self.backend.type_text(
                    &required_str(args, "text")?,
                    &query,
                    bool_arg(args, "clear")
                        .or_else(|| bool_arg(args, "clear_first"))
                        .unwrap_or(false),
                )?;
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_press" => {
                let app = optional_app_arg(args);
                let result = self.backend.press(
                    &required_str(args, "key")?,
                    &string_array_arg(args, "modifiers"),
                    app.as_deref(),
                )?;
                Ok(self.action_result_for_args(result, app.as_deref(), args))
            }
            "sootie_hotkey" => {
                let app = optional_app_arg(args);
                let result = self
                    .backend
                    .hotkey(&string_array_required(args, "keys")?, app.as_deref())?;
                Ok(self.action_result_for_args(result, app.as_deref(), args))
            }
            "sootie_scroll" => {
                let query = find_query(args);
                let at = self.optional_point_arg(args, "x", "y", "target")?;
                let direction = scroll_direction_arg(args)?;
                let result = self.backend.scroll(
                    &direction,
                    positive_i32_arg(args, "amount", 3)?,
                    query.app.as_deref(),
                    at,
                )?;
                Ok(self.action_result_for_args(result, query.app.as_deref(), args))
            }
            "sootie_focus" => {
                let app = required_app_arg(args)?;
                let platform_app_id = bundle_arg(args);
                let result = self.backend.focus(
                    &app,
                    platform_app_id.as_deref(),
                    str_arg(args, "window").as_deref(),
                )?;
                Ok(self.action_result_for_args(result, Some(&app), args))
            }
            "sootie_window" => {
                let app = required_app_arg(args)?;
                let platform_app_id = bundle_arg(args);
                let command = window_command(&required_str(args, "action")?)?;
                let bounds = self.window_bounds_arg(args, &command, &app)?;
                let result = self.backend.window(
                    command,
                    &app,
                    platform_app_id.as_deref(),
                    str_arg(args, "window").as_deref(),
                    bounds,
                )?;
                Ok(self.action_result_for_args(result, Some(&app), args))
            }
            "sootie_wait" => self.wait(args),
            "sootie_recipes" => Ok(ToolResult::ok(json!({ "recipes": self.recipes.list()? }))),
            "sootie_recipe_show" => Ok(ToolResult::ok(
                self.recipes.get(&required_str(args, "name")?)?,
            )),
            "sootie_recipe_save" => self.recipe_save(args),
            "sootie_recipe_delete" => Ok(ToolResult::ok(json!({
                "deleted": self.recipes.delete(&required_str(args, "name")?)?
            }))),
            "sootie_run" => self.run_recipe(args),
            "sootie_learn_start" => {
                let task = str_arg(args, "task_description")
                    .unwrap_or_else(|| "untitled task".to_string());
                self.learning_session = Some(LearningSession {
                    started: Instant::now(),
                    task: task.clone(),
                    events: Vec::new(),
                });
                Ok(ToolResult::ok(json!({
                    "status": "recording",
                    "message": "Recording successful Sootie actions in this session. Call sootie_learn_stop when done."
                })))
            }
            "sootie_learn_stop" => {
                let session = self.learning_session.take();
                let actions = session
                    .as_ref()
                    .map(|session| session.events.clone())
                    .unwrap_or_default();
                let action_count = actions.len();
                let task_description = session
                    .as_ref()
                    .map(|session| session.task.clone())
                    .unwrap_or_default();
                let duration_ms = session
                    .as_ref()
                    .map(|session| session.started.elapsed().as_millis())
                    .unwrap_or(0);
                let recipe = learned_recipe_from_actions(&task_description, &actions);
                let recipe_json = recipe
                    .as_ref()
                    .and_then(|recipe| serde_json::to_string_pretty(recipe).ok());
                Ok(ToolResult::ok(json!({
                    "actions": actions,
                    "task_description": task_description,
                    "action_count": action_count,
                    "duration_seconds": duration_ms / 1000,
                    "apps": learned_apps(&actions),
                    "urls": learned_urls(&actions),
                    "recipe": recipe,
                    "recipe_json": recipe_json
                })))
            }
            "sootie_learn_status" => Ok(ToolResult::ok(json!({
                "recording": self.learning_session.is_some(),
                "duration_seconds": self.learning_session.as_ref().map(|session| session.started.elapsed().as_secs()).unwrap_or(0),
                "action_count": self.learning_session.as_ref().map(|session| session.events.len()).unwrap_or(0)
            }))),
            _ => Err(SootieError::Unsupported(format!("unknown tool '{name}'"))),
        }
    }

    fn annotate(&self, args: &Value) -> SootieResult<ToolResult> {
        let app = optional_app_arg(args);
        let roles = string_array_arg(args, "roles");
        let max_labels = positive_u32_arg(args, "max_labels", 50)?.min(100) as usize;
        let screenshot = match self.backend.screenshot(app.as_deref(), None, false) {
            Ok(screenshot) => screenshot,
            Err(error) => return Ok(screenshot_error_result(error)),
        };
        let screenshot_mime_type = screenshot.mime_type.clone();
        let elements = self
            .backend
            .context(app.as_deref())?
            .interactive_elements
            .into_iter()
            .filter(|element| roles.is_empty() || role_matches(&element.role, &roles))
            .take(max_labels)
            .collect::<Vec<_>>();
        let labels = elements
            .iter()
            .enumerate()
            .map(|(idx, element)| {
                let center = element.bounds.as_ref().map(|bounds| bounds.center());
                json!({
                    "label": idx + 1,
                    "role": &element.role,
                    "name": element.name.as_ref().or(element.text.as_ref()),
                    "position": center.map(|point| json!({ "x": point.x, "y": point.y })),
                })
            })
            .collect::<Vec<_>>();
        let index = elements
            .iter()
            .enumerate()
            .map(|(idx, element)| {
                let center = element.bounds.as_ref().map(|bounds| bounds.center());
                format!(
                    "[{}] {} {} click={}",
                    idx + 1,
                    element.role,
                    element
                        .name
                        .as_ref()
                        .or(element.text.as_ref())
                        .map(String::as_str)
                        .unwrap_or(""),
                    center
                        .map(|point| format!("({}, {})", point.x.round(), point.y.round()))
                        .unwrap_or_else(|| "(unknown)".to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let mut payload = screenshot_payload(screenshot);
        payload["annotated_image"] = json!(annotated_svg_image(
            payload.get("image").and_then(Value::as_str).unwrap_or(""),
            &screenshot_mime_type,
            payload.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
            payload.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
            &elements,
        ));
        payload["mime_type"] = json!("image/svg+xml");
        Ok(ToolResult::ok(json!({
            "element_count": elements.len(),
            "elements": elements,
            "labels": labels,
            "index": index,
            "annotated_image": payload.get("annotated_image").cloned(),
            "mime_type": payload.get("mime_type").cloned(),
            "width": payload.get("width").cloned(),
            "height": payload.get("height").cloned(),
            "window_title": payload.get("window_title").cloned(),
            "window_frame": payload.get("window_frame").cloned(),
        })))
    }

    fn ground(&self, args: &Value) -> SootieResult<ToolResult> {
        let started = Instant::now();
        let description = required_str(args, "description")?;
        let crop_box = crop_box_arg(args)?;
        let roles = string_array_arg(args, "roles");
        let max_candidates = args
            .get("max_candidates")
            .map(|_| positive_u32_arg(args, "max_candidates", 1).map(|value| value as usize))
            .transpose()?;
        let query = FindQuery {
            query: Some(description.clone()),
            app: optional_app_arg(args),
            ..Default::default()
        };
        if self.resolution_strategy.is_vision_only() {
            if let Some(grounding) =
                self.try_vision_ground(query.app.as_deref(), &description, crop_box)?
            {
                return Ok(ToolResult::ok(vision_ground_payload(
                    &description,
                    crop_box,
                    grounding,
                    started.elapsed(),
                )));
            }
            return Ok(ToolResult::error(format!(
                "Vision grounding did not resolve '{description}'"
            )));
        }
        let mut candidates = self.backend.find(&query)?;
        if let Ok(context) = self.backend.context(query.app.as_deref()) {
            merge_candidate_elements(&mut candidates, context.interactive_elements);
        }
        let candidates = ranked_ground_candidates(&description, candidates)
            .into_iter()
            .filter(|candidate| candidate_in_crop(candidate, crop_box))
            .filter(|candidate| roles.is_empty() || role_matches(&candidate.role, &roles))
            .take(max_candidates.unwrap_or(usize::MAX))
            .collect::<Vec<_>>();
        let best_candidate = candidates.first();
        let best_point = best_candidate
            .and_then(|candidate| candidate.bounds.as_ref())
            .map(|bounds| bounds.center());
        let confidence = best_candidate
            .map(|candidate| ground_candidate_score(&description, candidate))
            .unwrap_or(0.0);
        if best_point.is_none() {
            if let Some(grounding) =
                self.try_vision_ground(query.app.as_deref(), &description, crop_box)?
            {
                return Ok(ToolResult::ok(vision_ground_payload(
                    &description,
                    crop_box,
                    grounding,
                    started.elapsed(),
                )));
            }
        }
        let mut payload = json!({
            "description": description,
            "candidates": candidates,
            "source": "platform-find",
            "durationMs": started.elapsed().as_millis(),
            "confidence": confidence,
        });
        if let Some((x1, y1, x2, y2)) = crop_box {
            payload["crop_box"] = json!([x1, y1, x2, y2]);
        }
        if let Some(point) = best_point {
            payload["x"] = json!(point.x);
            payload["y"] = json!(point.y);
        }
        Ok(ToolResult::ok(payload))
    }

    fn wait(&self, args: &Value) -> SootieResult<ToolResult> {
        let condition = required_str(args, "condition")?;
        let app = optional_app_arg(args);
        let value = match wait_value_arg(args, &condition) {
            Some(value) => value,
            None if condition == "titleChanged" => self
                .backend
                .context(app.as_deref())?
                .window
                .unwrap_or_default(),
            None if condition == "urlChanged" => self
                .backend
                .context(app.as_deref())?
                .url
                .unwrap_or_default(),
            None => String::new(),
        };
        let timeout = Duration::from_secs_f64(
            non_negative_seconds_arg(args, "timeout", "timeout_ms", 10.0)?.max(0.1),
        );
        let interval = Duration::from_secs_f64(
            non_negative_seconds_arg(args, "interval", "interval_ms", 0.5)?.max(0.05),
        );
        let start = Instant::now();
        loop {
            let wait_query = wait_find_query(args, app.clone(), &value);
            let matched = match condition.as_str() {
                "elementExists" => self.element_exists_for_wait(&wait_query)?,
                "elementGone" => !self.element_exists_for_wait(&wait_query)?,
                "titleContains" => self
                    .backend
                    .context(app.as_deref())?
                    .window
                    .map(|title| title.contains(&value))
                    .unwrap_or(false),
                "urlContains" => self
                    .backend
                    .context(app.as_deref())?
                    .url
                    .map(|url| url.contains(&value))
                    .unwrap_or(false),
                "urlChanged" => self
                    .backend
                    .context(app.as_deref())?
                    .url
                    .map(|url| url != value)
                    .unwrap_or(false),
                "titleChanged" => self
                    .backend
                    .context(app.as_deref())?
                    .window
                    .map(|title| title != value)
                    .unwrap_or(false),
                _ => {
                    return Err(SootieError::InvalidArguments(format!(
                        "unknown wait condition '{condition}'"
                    )))
                }
            };
            if matched {
                return Ok(ToolResult::ok(
                    json!({ "matched": true, "elapsed_ms": start.elapsed().as_millis() }),
                ));
            }
            if start.elapsed() >= timeout {
                return Ok(ToolResult::ok(
                    json!({ "matched": false, "timed_out": true, "elapsed_ms": start.elapsed().as_millis() }),
                ));
            }
            std::thread::sleep(interval);
        }
    }

    fn recipe_save(&self, args: &Value) -> SootieResult<ToolResult> {
        let recipe_value = args
            .get("recipe_json")
            .ok_or_else(|| SootieError::InvalidArguments("recipe_json is required".to_string()))?;
        let recipe = parse_recipe(recipe_value)?;
        let path = self.recipes.save(&recipe)?;
        Ok(ToolResult::ok(
            json!({ "saved": true, "path": path, "recipe": recipe }),
        ))
    }

    fn run_recipe(&mut self, args: &Value) -> SootieResult<ToolResult> {
        let recipe = self.recipes.get(&required_str(args, "recipe")?)?;
        let params = args
            .get("params")
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let missing_params = recipe
            .params
            .iter()
            .filter(|(name, param)| param.required && !params.contains_key(*name))
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        if !missing_params.is_empty() {
            return Ok(ToolResult::error(format!(
                "missing required recipe params: {}",
                missing_params.join(", ")
            )));
        }
        let locked_requirements = recipe_unlocked_screen_requirements(&recipe);
        if self.backend.screen_locked()? == Some(true) && !locked_requirements.is_empty() {
            return Ok(locked_recipe_result(&recipe));
        }
        if let Some(precondition_error) = self.check_recipe_preconditions(&recipe)? {
            return Ok(precondition_error);
        }

        let mut results = Vec::new();
        for step in &recipe.steps {
            let step_started_at = Instant::now();
            let (tool, raw_args) = recipe_step_tool_call(step, recipe.app.as_deref())?;
            let rendered_args = substitute_params(&raw_args, &params);
            let rendered_args = self.resolve_recipe_coordinates(rendered_args)?;
            let mut fallback_used = false;
            let mut fallback_reason = None;
            let result = if tool == "__delay" {
                let seconds = rendered_args
                    .get("seconds")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.5);
                std::thread::sleep(Duration::from_secs_f64(seconds.max(0.0)));
                ToolResult::ok(json!({ "matched": true, "delay_seconds": seconds }))
            } else if tool == "__set_clipboard" {
                self.recipe_set_clipboard(&rendered_args)?
            } else {
                let outcome = self.dispatch_recipe_tool(&tool, &rendered_args)?;
                fallback_used = outcome.fallback_used;
                fallback_reason = outcome.fallback_reason;
                outcome.result
            };
            let success = result.success;
            let mut step_result = json!({
                "id": step.id,
                "action": step.action,
                "tool": tool,
                "success": success,
                "data": result.data,
                "error": result.error,
                "suggestion": result.suggestion,
                "context": result.context,
            });
            if fallback_used {
                step_result["fallback_used"] = json!(true);
                if let Some(reason) = fallback_reason {
                    step_result["fallback_reason"] = json!(reason);
                }
            }
            step_result["duration_ms"] = json!(step_started_at.elapsed().as_millis() as u64);
            results.push(step_result.clone());
            if !success {
                if should_skip_step(step.on_failure.as_deref(), recipe.on_failure.as_deref()) {
                    continue;
                }
                return Ok(recipe_failed_result(&recipe, results, step_result));
            }

            if let Some(wait_after) = &step.wait_after {
                let (wait_tool, wait_args) =
                    recipe_wait_tool_call(wait_after, recipe.app.as_deref())?;
                let wait_started_at = Instant::now();
                let wait_result = if wait_tool == "__delay" {
                    let seconds = wait_args
                        .get("seconds")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.5);
                    std::thread::sleep(Duration::from_secs_f64(seconds));
                    ToolResult::ok(json!({ "matched": true, "delay_seconds": seconds }))
                } else {
                    let wait_args =
                        self.resolve_recipe_coordinates(substitute_params(&wait_args, &params))?;
                    self.dispatch(&wait_tool, &wait_args)?
                };
                let wait_failed = tool_wait_failed(&wait_result);
                step_result["wait_after"] = json!({
                    "success": wait_result.success,
                    "data": wait_result.data,
                    "error": wait_result.error,
                    "suggestion": wait_result.suggestion,
                    "context": wait_result.context,
                    "duration_ms": wait_started_at.elapsed().as_millis() as u64,
                });
                step_result["duration_ms"] = json!(step_started_at.elapsed().as_millis() as u64);
                if wait_failed {
                    step_result["success"] = json!(false);
                    results.pop();
                    results.push(step_result.clone());
                    if should_skip_step(step.on_failure.as_deref(), recipe.on_failure.as_deref()) {
                        continue;
                    }
                    return Ok(recipe_failed_result(&recipe, results, step_result));
                }
                results.pop();
                results.push(step_result);
            }
        }
        Ok(ToolResult::ok(recipe_success_payload(&recipe, results)))
    }

    fn recipe_set_clipboard(&self, args: &Value) -> SootieResult<ToolResult> {
        let text = required_str(args, "text")?;
        let result = self.backend.set_clipboard_text(&text)?;
        Ok(ToolResult::ok(json!({
            "method": result.method,
            "details": result.details,
            "bytes": text.len(),
            "chars": text.chars().count(),
        })))
    }

    fn dispatch_recipe_tool(
        &mut self,
        tool: &str,
        args: &Value,
    ) -> SootieResult<RecipeDispatchOutcome> {
        let mut primary_args = recipe_primary_dispatch_args(args);
        let mut fallback_args = recipe_coordinate_fallback_args(args);
        suppress_recipe_action_context(tool, &mut primary_args);
        if let Some(fallback_args) = fallback_args.as_mut() {
            suppress_recipe_action_context(tool, fallback_args);
        }
        match self.dispatch(tool, &primary_args) {
            Ok(result) if result.success => Ok(RecipeDispatchOutcome {
                result,
                fallback_used: false,
                fallback_reason: None,
            }),
            Ok(result) => {
                let fallback_reason = result
                    .error
                    .as_deref()
                    .and_then(recipe_coordinate_fallback_reason);
                if let (Some(fallback_args), Some(reason)) = (fallback_args, fallback_reason) {
                    let fallback = self.dispatch(tool, &fallback_args)?;
                    return Ok(RecipeDispatchOutcome {
                        result: fallback,
                        fallback_used: true,
                        fallback_reason: Some(reason.to_string()),
                    });
                }
                Ok(RecipeDispatchOutcome {
                    result,
                    fallback_used: false,
                    fallback_reason: None,
                })
            }
            Err(error) => {
                if let (Some(fallback_args), Some(reason)) = (
                    fallback_args,
                    recipe_error_coordinate_fallback_reason(&error),
                ) {
                    let fallback = self.dispatch(tool, &fallback_args)?;
                    return Ok(RecipeDispatchOutcome {
                        result: fallback,
                        fallback_used: true,
                        fallback_reason: Some(reason),
                    });
                }
                Err(error)
            }
        }
    }

    fn check_recipe_preconditions(&self, recipe: &Recipe) -> SootieResult<Option<ToolResult>> {
        let Some(preconditions) = &recipe.preconditions else {
            return Ok(None);
        };
        let mut url_context = None;
        let mut url_matches = false;
        if let Some(url) = &preconditions.url_contains {
            let context = self.recipe_url_precondition_context(
                recipe_url_precondition_app(recipe, preconditions),
                url,
            )?;
            url_matches = recipe_context_matches_url_precondition(&context, url);
            url_context = Some((url.as_str(), context));
        }
        if let Some(app) = &preconditions.app_running {
            let url_precondition_proves_app = url_matches
                && recipe_url_precondition_app(recipe, preconditions)
                    .is_some_and(|url_app| url_app.eq_ignore_ascii_case(app));
            if !url_precondition_proves_app && self.backend.state(Some(app))?.is_empty() {
                let context = self.backend.context(Some(app))?;
                if context.app.is_some()
                    || context.window.is_some()
                    || !context.interactive_elements.is_empty()
                {
                    return Ok(None);
                }
                let diagnostics = self.backend.diagnostics();
                return Ok(Some(
                    ToolResult::error(format!(
                        "recipe precondition failed: app '{app}' is not accessible"
                    ))
                    .with_suggestion(empty_context_suggestion(
                        self.backend.platform(),
                        Some(app),
                        &diagnostics,
                    )),
                ));
            }
        }
        if let Some((url, context)) = url_context {
            if !recipe_context_matches_url_precondition(&context, url) {
                return Ok(Some(
                    ToolResult::error(format!(
                        "recipe precondition failed: current URL does not contain '{url}'"
                    ))
                    .with_suggestion("Navigate to the required page first, then retry the recipe."),
                ));
            }
        }
        Ok(None)
    }

    fn recipe_url_precondition_context(
        &self,
        app: Option<&str>,
        required_url: &str,
    ) -> SootieResult<crate::types::ContextSnapshot> {
        let mut last_fast_url = None;
        for attempt in 0..3 {
            let url = self.backend.browser_url(app)?.unwrap_or_default();
            if !url.is_empty() {
                return Ok(recipe_url_precondition_url_context(app, url));
            }
            last_fast_url = Some(url);
            if attempt < 2 {
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        let mut last_context = None;
        for attempt in 0..3 {
            let context = self.backend.context(app)?;
            if recipe_context_matches_url_precondition(&context, required_url) {
                return Ok(context);
            }
            let url_is_empty = context.url.as_deref().unwrap_or_default().is_empty();
            last_context = Some(context);
            if !url_is_empty || attempt == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if let Some(context) = last_context {
            Ok(context)
        } else if let Some(url) = last_fast_url.filter(|url| !url.is_empty()) {
            Ok(recipe_url_precondition_url_context(app, url))
        } else {
            self.backend.context(app)
        }
    }

    fn action_result_for_args(
        &self,
        result: ActionResult,
        app: Option<&str>,
        args: &Value,
    ) -> ToolResult {
        self.action_result_with_context(
            result,
            app,
            bool_arg(args, "__include_context").unwrap_or(true),
        )
    }

    fn action_result_with_context(
        &self,
        result: ActionResult,
        app: Option<&str>,
        include_context: bool,
    ) -> ToolResult {
        let context = include_context
            .then(|| {
                self.backend
                    .context(app)
                    .ok()
                    .and_then(|context| serde_json::to_value(context).ok())
            })
            .flatten();
        ToolResult {
            success: true,
            data: Some(action_payload(result)),
            error: None,
            suggestion: None,
            context,
        }
    }

    fn find_elements(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
        if self.resolution_strategy.is_vision_only() {
            return Ok(self.vision_fallback_element(query)?.into_iter().collect());
        }
        let mut elements = self.backend.find(query)?;
        if elements.is_empty() {
            elements = self.context_find_elements(query)?;
        }
        if elements.is_empty() {
            if let Some(element) = self.vision_fallback_element(query)? {
                elements.push(element);
            }
        }
        Ok(elements)
    }

    fn inspect_element(&self, query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
        if self.resolution_strategy.is_vision_only() {
            return self.vision_fallback_element(query);
        }
        match self.backend.inspect(query)? {
            Some(element) => Ok(Some(element)),
            None => {
                if let Some(element) = self.context_find_elements(query)?.into_iter().next() {
                    return Ok(Some(element));
                }
                self.vision_fallback_element(query)
            }
        }
    }

    fn context_find_elements(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
        if !query_has_target(query) {
            return Ok(Vec::new());
        }
        Ok(self
            .backend
            .context(query.app.as_deref())?
            .interactive_elements
            .into_iter()
            .filter(|element| element_matches_find_query(element, query))
            .collect())
    }

    fn element_exists_for_wait(&self, query: &FindQuery) -> SootieResult<bool> {
        if self.resolution_strategy.is_vision_only() && query_has_target(query) {
            return Ok(self.vision_fallback_element(query)?.is_some());
        }
        Ok(!self.backend.find(query)?.is_empty())
    }

    fn vision_first_click(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        button: &str,
        count: u32,
    ) -> SootieResult<Option<ActionResult>> {
        if !self.should_resolve_target_with_vision_first(x, y, query) {
            return Ok(None);
        }
        self.vision_target_click(query, button, count).map(Some)
    }

    fn vision_first_hover(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> SootieResult<Option<ActionResult>> {
        if !self.should_resolve_target_with_vision_first(x, y, query) {
            return Ok(None);
        }
        self.vision_target_hover(query).map(Some)
    }

    fn vision_first_long_press(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        duration_secs: f64,
        button: &str,
    ) -> SootieResult<Option<ActionResult>> {
        if !self.should_resolve_target_with_vision_first(x, y, query) {
            return Ok(None);
        }
        self.vision_target_long_press(query, duration_secs, button)
            .map(Some)
    }

    fn should_resolve_target_with_vision_first(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> bool {
        self.resolution_strategy.is_vision_only()
            && x.is_none()
            && y.is_none()
            && query_has_target(query)
    }

    fn vision_fallback_element(&self, query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
        let Some((description, grounding)) = self.vision_grounding_for_query(query, None)? else {
            return Ok(None);
        };
        if grounding.result.confidence < self.vision.confidence_threshold() {
            return Ok(None);
        }
        Ok(Some(ElementInfo {
            id: Some(format!("vision:{}", stable_identifier_text(&description))),
            role: "VisionTarget".to_string(),
            title: Some(description.clone()),
            name: Some(description),
            text: None,
            bounds: Some(grounding.synthetic_bounds()),
            actions: vec!["click".to_string(), "hover".to_string()],
            editable: Some(false),
            enabled: Some(true),
        }))
    }

    fn vision_fallback_click(
        &self,
        error: &SootieError,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        button: &str,
        count: u32,
    ) -> SootieResult<Option<ActionResult>> {
        if !should_attempt_vision_action_fallback(error, x, y, query) {
            return Ok(None);
        }
        match self.vision_target_click(query, button, count) {
            Ok(result) => Ok(Some(result)),
            Err(SootieError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn vision_fallback_hover(
        &self,
        error: &SootieError,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> SootieResult<Option<ActionResult>> {
        if !should_attempt_vision_action_fallback(error, x, y, query) {
            return Ok(None);
        }
        match self.vision_target_hover(query) {
            Ok(result) => Ok(Some(result)),
            Err(SootieError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn vision_fallback_long_press(
        &self,
        error: &SootieError,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        duration_secs: f64,
        button: &str,
    ) -> SootieResult<Option<ActionResult>> {
        if !should_attempt_vision_action_fallback(error, x, y, query) {
            return Ok(None);
        }
        match self.vision_target_long_press(query, duration_secs, button) {
            Ok(result) => Ok(Some(result)),
            Err(SootieError::NotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn vision_target_click(
        &self,
        query: &FindQuery,
        button: &str,
        count: u32,
    ) -> SootieResult<ActionResult> {
        let (description, grounding) = self.required_vision_grounding_for_query(query)?;
        self.focus_app_for_pointer_dispatch(query)?;
        let direct = self.backend.click(
            Some(grounding.x),
            Some(grounding.y),
            &FindQuery::default(),
            button,
            count,
        )?;
        Ok(vision_action_result(
            "vision-grounded-click",
            direct,
            &description,
            &grounding,
        ))
    }

    fn vision_target_hover(&self, query: &FindQuery) -> SootieResult<ActionResult> {
        let (description, grounding) = self.required_vision_grounding_for_query(query)?;
        self.focus_app_for_pointer_dispatch(query)?;
        let direct =
            self.backend
                .hover(Some(grounding.x), Some(grounding.y), &FindQuery::default())?;
        Ok(vision_action_result(
            "vision-grounded-hover",
            direct,
            &description,
            &grounding,
        ))
    }

    fn vision_target_long_press(
        &self,
        query: &FindQuery,
        duration_secs: f64,
        button: &str,
    ) -> SootieResult<ActionResult> {
        let (description, grounding) = self.required_vision_grounding_for_query(query)?;
        self.focus_app_for_pointer_dispatch(query)?;
        let direct = self.backend.long_press(
            Some(grounding.x),
            Some(grounding.y),
            &FindQuery::default(),
            duration_secs,
            button,
        )?;
        Ok(vision_action_result(
            "vision-grounded-long-press",
            direct,
            &description,
            &grounding,
        ))
    }

    fn focus_app_for_pointer_dispatch(&self, query: &FindQuery) -> SootieResult<()> {
        if let Some(app) = query.app.as_deref() {
            self.backend.focus(app, None, None)?;
        }
        Ok(())
    }

    fn focus_app_for_explicit_pointer_coordinates(
        &self,
        query: &FindQuery,
        point: Option<(f64, f64)>,
    ) -> SootieResult<()> {
        if point.is_some() {
            self.focus_app_for_pointer_dispatch(query)?;
        }
        Ok(())
    }

    fn required_vision_grounding_for_query(
        &self,
        query: &FindQuery,
    ) -> SootieResult<(String, VisionMappedGrounding)> {
        match self.vision_grounding_for_query(query, None)? {
            Some((description, grounding))
                if grounding.result.confidence >= self.vision.confidence_threshold() =>
            {
                Ok((description, grounding))
            }
            Some((description, grounding)) => Err(SootieError::NotFound(format!(
                "vision confidence {:.2} below threshold {:.2} for '{}'",
                grounding.result.confidence,
                self.vision.confidence_threshold(),
                description
            ))),
            None => Err(SootieError::NotFound("vision target not found".into())),
        }
    }

    fn vision_grounding_for_query(
        &self,
        query: &FindQuery,
        crop_box: Option<(f64, f64, f64, f64)>,
    ) -> SootieResult<Option<(String, VisionMappedGrounding)>> {
        let Some(description) = vision_description(query) else {
            return Ok(None);
        };
        Ok(self
            .try_vision_ground(query.app.as_deref(), &description, crop_box)?
            .map(|grounding| (description, grounding)))
    }

    fn try_vision_ground(
        &self,
        app: Option<&str>,
        description: &str,
        crop_box: Option<(f64, f64, f64, f64)>,
    ) -> SootieResult<Option<VisionMappedGrounding>> {
        if !self.vision.is_enabled() {
            return Ok(None);
        }
        let screenshot = match self.backend.screenshot(app, None, false) {
            Ok(screenshot) => screenshot,
            Err(error) => {
                tracing::debug!(%error, "vision fallback skipped because screenshot failed");
                return Ok(None);
            }
        };
        let frame = VisionFrame::from_screenshot(&screenshot);
        let local_crop_box = crop_box.and_then(|crop_box| frame.local_crop_box(crop_box));
        let request = GroundRequest {
            image_base64: &screenshot.data_base64,
            description,
            screen_width: frame.width,
            screen_height: frame.height,
            crop_box: local_crop_box,
        };
        let result = match vision::ground(&self.vision, &request) {
            Ok(result) => result,
            Err(error) => {
                tracing::debug!(%error, "vision fallback grounding failed");
                return Ok(None);
            }
        };
        let history = result.as_ref().and_then(|result| {
            persist_grounding_history_screenshot(
                &screenshot,
                description,
                &frame,
                local_crop_box,
                result,
            )
        });
        Ok(result.map(|result| VisionMappedGrounding::new(result, frame, screenshot, history)))
    }

    fn optional_point_arg(
        &self,
        args: &Value,
        x_key: &str,
        y_key: &str,
        target_key: &str,
    ) -> SootieResult<Option<(f64, f64)>> {
        if let Some(point) = point_arg(args, x_key, y_key, target_key)? {
            return Ok(Some(point));
        }
        let query = find_query_with_target(args, target_key);
        if !query_has_target(&query) {
            return Ok(None);
        }
        if self.resolution_strategy.is_vision_only() {
            return self
                .required_vision_grounding_for_query(&query)
                .map(|(_, grounding)| Some((grounding.x, grounding.y)));
        }
        if let Some(point) = self
            .backend
            .find(&query)?
            .into_iter()
            .find_map(|element| element.bounds.map(|bounds| bounds.center()))
        {
            return Ok(Some((point.x, point.y)));
        }
        self.required_vision_grounding_for_query(&query)
            .map(|(_, grounding)| Some((grounding.x, grounding.y)))
            .map_err(|_| SootieError::NotFound(format!("{target_key} did not resolve")))
    }

    fn required_point_arg(
        &self,
        args: &Value,
        x_key: &str,
        y_key: &str,
        target_key: &str,
    ) -> SootieResult<(f64, f64)> {
        self.optional_point_arg(args, x_key, y_key, target_key)?
            .ok_or_else(|| {
                SootieError::InvalidArguments(format!(
                    "{x_key}/{y_key} or {target_key}.coordinate is required"
                ))
            })
    }

    fn window_bounds_arg(
        &self,
        args: &Value,
        command: &WindowCommand,
        app: &str,
    ) -> SootieResult<Option<Bounds>> {
        let explicit = partial_bounds_arg(args)?;
        let requires_bounds = matches!(command, WindowCommand::Move | WindowCommand::Resize);
        let has_partial = explicit.iter().any(Option::is_some);
        if !requires_bounds && !has_partial {
            return Ok(None);
        }
        if let [Some(x), Some(y), Some(width), Some(height)] = explicit {
            return validate_window_bounds(Bounds {
                x,
                y,
                width,
                height,
            })
            .map(Some);
        }
        if !requires_bounds {
            return Ok(None);
        }
        let current = self
            .current_window_bounds(app, str_arg(args, "window").as_deref())?
            .ok_or_else(|| {
                SootieError::InvalidArguments(
                    "move/resize requires x/y/width/height when current window bounds are unavailable"
                        .to_string(),
                )
            })?;
        validate_window_bounds(Bounds {
            x: explicit[0].unwrap_or(current.x),
            y: explicit[1].unwrap_or(current.y),
            width: explicit[2].unwrap_or(current.width),
            height: explicit[3].unwrap_or(current.height),
        })
        .map(Some)
    }

    fn current_window_bounds(
        &self,
        app: &str,
        window: Option<&str>,
    ) -> SootieResult<Option<Bounds>> {
        let windows = self
            .backend
            .state(Some(app))?
            .into_iter()
            .flat_map(|app| app.windows.into_iter())
            .collect::<Vec<_>>();
        let selected = if let Some(needle) = window {
            windows
                .iter()
                .find(|candidate| candidate.title.contains(needle))
        } else {
            windows
                .iter()
                .find(|candidate| candidate.focused)
                .or_else(|| windows.first())
        };
        Ok(selected.and_then(|window| window.bounds.clone()))
    }

    fn resolve_recipe_coordinates(&self, args: Value) -> SootieResult<Value> {
        let app = optional_app_arg(&args);
        let window = args
            .get("window")
            .and_then(Value::as_str)
            .map(str::to_string);
        resolve_recipe_coordinate_spaces(args, app.as_deref(), window.as_deref(), |app, window| {
            self.current_window_bounds(app, window)
        })
    }
}

fn read_stdio_message<R: BufRead>(reader: &mut R) -> anyhow::Result<Option<StdioMessage>> {
    let mut first_line = String::new();
    loop {
        first_line.clear();
        if reader.read_line(&mut first_line)? == 0 {
            return Ok(None);
        }
        if !first_line.trim().is_empty() {
            break;
        }
    }

    if !looks_like_stdio_header(&first_line) {
        return Ok(Some(StdioMessage {
            encoding: StdioEncoding::LineJson,
            body: first_line.trim_end_matches(['\r', '\n']).to_string(),
        }));
    }

    let mut content_length = parse_content_length_header(&first_line)?;

    let mut header_line = String::new();
    loop {
        header_line.clear();
        if reader.read_line(&mut header_line)? == 0 {
            return Err(anyhow::anyhow!("unexpected EOF while reading MCP headers"));
        }
        if header_line.trim().is_empty() {
            break;
        }
        if let Some(length) = parse_content_length_header(&header_line)? {
            content_length = Some(length);
        }
    }
    let content_length =
        content_length.ok_or_else(|| anyhow::anyhow!("MCP stdio frame missing Content-Length"))?;

    let mut body = vec![0_u8; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(StdioMessage {
        encoding: StdioEncoding::ContentLength,
        body: String::from_utf8(body)?,
    }))
}

fn looks_like_stdio_header(line: &str) -> bool {
    let Some((name, _)) = line.split_once(':') else {
        return false;
    };
    !line.trim_start().starts_with(['{', '['])
        && !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn parse_content_length_header(line: &str) -> anyhow::Result<Option<usize>> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(None);
    };
    if !name.eq_ignore_ascii_case("content-length") {
        return Ok(None);
    }
    Ok(Some(value.trim().parse()?))
}

fn write_stdio_response<W: Write>(
    writer: &mut W,
    encoding: StdioEncoding,
    response: &JsonRpcResponse,
) -> anyhow::Result<()> {
    let body = serde_json::to_string(response)?;
    match encoding {
        StdioEncoding::LineJson => {
            writeln!(writer, "{body}")?;
        }
        StdioEncoding::ContentLength => {
            write!(writer, "Content-Length: {}\r\n\r\n{body}", body.len())?;
        }
    }
    Ok(())
}

fn tool_call_args(name: &str, params: &Value) -> Value {
    let raw = params
        .get("arguments")
        .or_else(|| params.get("data"))
        .or_else(|| params.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    normalize_tool_args(name, raw)
}

fn normalize_tool_args(name: &str, raw: Value) -> Value {
    let Value::Object(mut map) = raw else {
        return raw;
    };

    for key in ["data", "input"] {
        if let Some(inner) = map.remove(key) {
            return merge_argument_envelope(inner, map);
        }
    }

    if name != "sootie_run" {
        if let Some(inner) = map.remove("params") {
            return merge_argument_envelope(inner, map);
        }
    }

    Value::Object(map)
}

fn merge_argument_envelope(inner: Value, rest: serde_json::Map<String, Value>) -> Value {
    let Value::Object(mut inner_map) = inner else {
        return Value::Object(rest);
    };
    for (key, value) in rest {
        inner_map.entry(key).or_insert(value);
    }
    Value::Object(inner_map)
}

fn validate_public_tool_args(name: &str, args: &Value) -> SootieResult<()> {
    let Value::Object(map) = args else {
        return Err(SootieError::InvalidArguments(format!(
            "{name} arguments must be an object"
        )));
    };
    let definitions = tool_definitions();
    let tool = definitions
        .iter()
        .find(|tool| tool.name == name)
        .ok_or_else(|| SootieError::Unsupported(format!("unknown tool '{name}'")))?;
    let properties = tool.input_schema["properties"]
        .as_object()
        .ok_or_else(|| SootieError::Platform(format!("{name} schema has no properties")))?;
    let unknown = map
        .keys()
        .filter(|key| !properties.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        let allowed = properties.keys().cloned().collect::<Vec<_>>().join(", ");
        return Err(SootieError::InvalidArguments(format!(
            "{name} does not accept argument(s): {}. Allowed arguments: {allowed}",
            unknown.join(", ")
        )));
    }
    if let Some(missing) = missing_public_required_args(&tool.input_schema, map) {
        return Err(SootieError::InvalidArguments(format!(
            "{name} requires argument(s): {}",
            missing.join(", ")
        )));
    }
    for (key, value) in map {
        validate_public_arg_type(name, key, value, &properties[key])?;
    }
    Ok(())
}

fn missing_public_required_args(
    schema: &Value,
    args: &serde_json::Map<String, Value>,
) -> Option<Vec<String>> {
    let missing = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|key| !args.contains_key(*key))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

fn validate_public_arg_type(
    tool_name: &str,
    key: &str,
    value: &Value,
    schema: &Value,
) -> SootieResult<()> {
    if let Some(variants) = schema.get("anyOf").and_then(Value::as_array) {
        if variants
            .iter()
            .any(|variant| validate_public_arg_type(tool_name, key, value, variant).is_ok())
        {
            return Ok(());
        }
        return Err(SootieError::InvalidArguments(format!(
            "{tool_name}.{key} must match one of the advertised schema variants"
        )));
    }
    let Some(ty) = schema.get("type").and_then(Value::as_str) else {
        return Ok(());
    };
    let valid = match ty {
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "number" => value.is_number(),
        "integer" => json_number_is_integer(value),
        "object" => value.is_object(),
        "array" => validate_public_array_arg(key, value, schema)?,
        _ => true,
    };
    if valid {
        return Ok(());
    }
    Err(SootieError::InvalidArguments(format!(
        "{tool_name}.{key} must be {}",
        public_schema_type_label(schema)
    )))
}

fn validate_public_array_arg(key: &str, value: &Value, schema: &Value) -> SootieResult<bool> {
    let Some(items) = value.as_array() else {
        return Ok(false);
    };
    let Some(item_ty) = schema
        .get("items")
        .and_then(|items| items.get("type"))
        .and_then(Value::as_str)
    else {
        return Ok(true);
    };
    for (index, item) in items.iter().enumerate() {
        let valid = match item_ty {
            "string" => item.is_string(),
            "number" => item.is_number(),
            "integer" => json_number_is_integer(item),
            "boolean" => item.is_boolean(),
            "object" => item.is_object(),
            _ => true,
        };
        if !valid {
            return Err(SootieError::InvalidArguments(format!(
                "{key}[{index}] must be a {item_ty}"
            )));
        }
    }
    Ok(true)
}

fn json_number_is_integer(value: &Value) -> bool {
    value.as_i64().is_some()
        || value.as_u64().is_some()
        || value
            .as_f64()
            .map(|number| number.is_finite() && number.fract() == 0.0)
            .unwrap_or(false)
}

fn public_schema_type_label(schema: &Value) -> String {
    match schema.get("type").and_then(Value::as_str) {
        Some("array") => schema
            .get("items")
            .and_then(|items| items.get("type"))
            .and_then(Value::as_str)
            .map(|item_ty| format!("an array of {item_ty}"))
            .unwrap_or_else(|| "an array".to_string()),
        Some("object") => "an object".to_string(),
        Some("integer") => "an integer".to_string(),
        Some("number") => "a number".to_string(),
        Some("boolean") => "a boolean".to_string(),
        Some("string") => "a string".to_string(),
        Some(ty) => format!("a {ty}"),
        None => "the advertised schema type".to_string(),
    }
}

fn recipe_url_precondition_app<'a>(
    recipe: &'a Recipe,
    preconditions: &'a crate::recipe::RecipePreconditions,
) -> Option<&'a str> {
    recipe
        .app
        .as_deref()
        .or(preconditions.app_running.as_deref())
}

fn recipe_url_precondition_url_context(
    app: Option<&str>,
    url: String,
) -> crate::types::ContextSnapshot {
    crate::types::ContextSnapshot {
        app: app.map(str::to_string),
        app_id: app.map(str::to_string),
        platform_app_id: None,
        bundle_id: None,
        pid: None,
        window: None,
        url: Some(url),
        focused_element: None,
        interactive_elements: vec![],
    }
}

fn recipe_unlocked_screen_requirements(recipe: &Recipe) -> Vec<Value> {
    recipe
        .steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            let (tool, args) = recipe_step_tool_call(step, recipe.app.as_deref()).ok()?;
            tool_requires_unlocked_screen(&tool, &args).then(|| {
                json!({
                    "step_index": index,
                    "step_id": step.id,
                    "tool": tool,
                    "action": step.action,
                })
            })
        })
        .collect()
}

fn tool_requires_unlocked_screen(tool: &str, args: &Value) -> bool {
    match tool {
        "sootie_click"
        | "sootie_hover"
        | "sootie_long_press"
        | "sootie_drag"
        | "sootie_type"
        | "sootie_press"
        | "sootie_hotkey"
        | "sootie_scroll"
        | "sootie_focus"
        | "sootie_screenshot"
        | "sootie_parse_screen"
        | "sootie_ground"
        | "sootie_annotate" => true,
        "sootie_window" => match args.get("action").and_then(Value::as_str) {
            Some(action) => !action.eq_ignore_ascii_case("list"),
            None => true,
        },
        _ => false,
    }
}

fn locked_recipe_result(recipe: &Recipe) -> ToolResult {
    let blocked_steps = recipe_unlocked_screen_requirements(recipe);
    let mut result = ToolResult::error(format!(
        "recipe '{}' requires an unlocked macOS screen",
        recipe.name
    ));
    result.data = Some(json!({
        "locked": true,
        "blocked_steps": blocked_steps,
    }));
    result.with_suggestion(
        "macOS is locked, so UI actions or screenshots would affect the lock screen instead of the target app. Unlock the Mac, verify the target window is visible, then retry.",
    )
}

fn should_skip_step(step_policy: Option<&str>, recipe_policy: Option<&str>) -> bool {
    matches!(step_policy.or(recipe_policy), Some("skip"))
}

fn tool_wait_failed(result: &ToolResult) -> bool {
    !result.success
        || result
            .data
            .as_ref()
            .and_then(|data| data.get("matched"))
            .and_then(Value::as_bool)
            .is_some_and(|matched| !matched)
}

fn recipe_context_matches_url_precondition(
    context: &crate::types::ContextSnapshot,
    required_url: &str,
) -> bool {
    if context
        .url
        .as_deref()
        .is_some_and(|url| url.contains(required_url))
    {
        return true;
    }
    if context.url.as_deref().is_some_and(|url| !url.is_empty()) {
        return false;
    }
    context
        .window
        .as_deref()
        .is_some_and(|window| window.contains(required_url))
        || context.interactive_elements.iter().any(|element| {
            [
                element.name.as_deref(),
                element.title.as_deref(),
                element.text.as_deref(),
                element.id.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|text| text.contains(required_url))
        })
}

fn recipe_success_payload(recipe: &Recipe, steps: Vec<Value>) -> Value {
    let mut payload = json!({
        "recipe": recipe.name,
        "success": true,
        "steps_completed": steps.len(),
        "total_steps": recipe.steps.len(),
        "steps": steps,
    });
    if let Some(steps) = payload.get("steps").and_then(Value::as_array) {
        if let Some(screenshot) = last_recipe_screenshot_artifact(steps) {
            payload["last_screenshot"] = screenshot;
        }
    }
    payload
}

fn last_recipe_screenshot_artifact(steps: &[Value]) -> Option<Value> {
    steps.iter().rev().find_map(|step| {
        if step.get("tool").and_then(Value::as_str) != Some("sootie_screenshot") {
            return None;
        }
        let data = step.get("data")?;
        let artifact_path = data.get("artifact_path").and_then(Value::as_str)?;
        let mut screenshot = json!({
            "step_id": step.get("id").cloned().unwrap_or(Value::Null),
            "artifact_path": artifact_path,
        });
        copy_json_field(data, &mut screenshot, "artifact_uri");
        copy_json_field(data, &mut screenshot, "width");
        copy_json_field(data, &mut screenshot, "height");
        copy_json_field(data, &mut screenshot, "window_title");
        copy_json_field(data, &mut screenshot, "mime_type");
        Some(screenshot)
    })
}

fn copy_json_field(source: &Value, target: &mut Value, field: &str) {
    if let Some(value) = source.get(field) {
        target[field] = value.clone();
    }
}

fn recipe_failed_result(recipe: &Recipe, steps: Vec<Value>, failed_step: Value) -> ToolResult {
    let suggestion = recipe_failure_suggestion(&failed_step).unwrap_or_else(|| {
        "Inspect failed_step, current context, and screenshots before retrying.".to_string()
    });
    let steps_completed = steps
        .iter()
        .filter(|step| {
            step.get("success")
                .and_then(Value::as_bool)
                .is_some_and(|success| success)
        })
        .count();
    ToolResult {
        success: false,
        data: Some(json!({
            "recipe": recipe.name,
            "success": false,
            "failed_step": failed_step,
            "steps_completed": steps_completed,
            "steps_attempted": steps.len(),
            "total_steps": recipe.steps.len(),
            "steps": steps,
        })),
        error: Some(format!("recipe '{}' failed", recipe.name)),
        suggestion: Some(suggestion),
        context: None,
    }
}

fn recipe_failure_suggestion(failed_step: &Value) -> Option<String> {
    failed_step
        .get("suggestion")
        .and_then(Value::as_str)
        .filter(|suggestion| !suggestion.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            failed_step
                .get("wait_after")
                .and_then(|wait_after| wait_after.get("suggestion"))
                .and_then(Value::as_str)
                .filter(|suggestion| !suggestion.trim().is_empty())
                .map(str::to_string)
        })
}

fn learned_action_event(name: &str, args: &Value) -> Option<Value> {
    let app = app_arg(args);
    let mut event = json!({
        "timestamp": unix_timestamp_seconds(),
        "app": app,
        "bundle_id": bundle_arg(args),
        "window": Value::Null,
        "url": Value::Null,
        "element": Value::Null,
    });
    match name {
        "sootie_click" => {
            event["action_type"] = json!("click");
            event["x"] = point_component_or_null(args, "x", "target", "x");
            event["y"] = point_component_or_null(args, "y", "target", "y");
            event["query"] = query_arg(args).map(Value::String).unwrap_or(Value::Null);
            event["target"] = value_or_null(args, "target");
            event["button"] = json!(mouse_button_arg(args).unwrap_or_else(|_| "left".to_string()));
            event["count"] = json!(positive_u32_arg(args, "count", 1).unwrap_or(1));
        }
        "sootie_hover" => {
            event["action_type"] = json!("hover");
            event["x"] = point_component_or_null(args, "x", "target", "x");
            event["y"] = point_component_or_null(args, "y", "target", "y");
            event["query"] = query_arg(args).map(Value::String).unwrap_or(Value::Null);
            event["target"] = value_or_null(args, "target");
        }
        "sootie_long_press" => {
            event["action_type"] = json!("longPress");
            event["x"] = point_component_or_null(args, "x", "target", "x");
            event["y"] = point_component_or_null(args, "y", "target", "y");
            event["query"] = query_arg(args).map(Value::String).unwrap_or(Value::Null);
            event["target"] = value_or_null(args, "target");
            event["duration"] =
                json!(
                    non_negative_seconds_arg(args, "duration", "duration_ms", 1.0).unwrap_or(1.0)
                );
            event["button"] = json!(mouse_button_arg(args).unwrap_or_else(|_| "left".to_string()));
        }
        "sootie_drag" => {
            event["action_type"] = json!("drag");
            event["from_x"] = point_component_or_null(args, "from_x", "from_target", "x");
            event["from_y"] = point_component_or_null(args, "from_y", "from_target", "y");
            event["to_x"] = point_component_or_null(args, "to_x", "to_target", "x");
            event["to_y"] = point_component_or_null(args, "to_y", "to_target", "y");
            event["query"] = query_arg(args).map(Value::String).unwrap_or(Value::Null);
            event["from_target"] = value_or_null(args, "from_target");
            event["to_target"] = value_or_null(args, "to_target");
            event["duration"] =
                json!(
                    non_negative_seconds_arg(args, "duration", "duration_ms", 0.5).unwrap_or(0.5)
                );
            event["hold_duration"] =
                json!(
                    non_negative_seconds_arg(args, "hold_duration", "hold_duration_ms", 0.1)
                        .unwrap_or(0.1)
                );
        }
        "sootie_type" => {
            event["action_type"] = json!("typeText");
            event["text"] = value_or_null(args, "text");
            event["query"] = query_arg(args).map(Value::String).unwrap_or(Value::Null);
            event["target"] = value_or_null(args, "target");
        }
        "sootie_press" => {
            event["action_type"] = json!("keyPress");
            event["key_code"] = Value::Null;
            event["key_name"] = value_or_null(args, "key");
            event["modifiers"] = json!(string_array_arg(args, "modifiers"));
        }
        "sootie_hotkey" => {
            let keys = string_array_arg(args, "keys");
            let (key_name, modifiers) = keys
                .split_last()
                .map(|(key, modifiers)| (json!(key), json!(modifiers)))
                .unwrap_or((Value::Null, json!([])));
            event["action_type"] = json!("hotkey");
            event["key_name"] = key_name;
            event["modifiers"] = modifiers;
        }
        "sootie_focus" => {
            event["action_type"] = json!("appSwitch");
            event["app"] = Value::Null;
            event["to_app"] = json!(app_arg(args));
            event["to_bundle_id"] = json!(bundle_arg(args));
        }
        "sootie_scroll" => {
            let amount = i64::from(positive_i32_arg(args, "amount", 3).unwrap_or(3));
            let direction = args
                .get("direction")
                .and_then(Value::as_str)
                .map(|direction| direction.trim().to_lowercase())
                .unwrap_or_else(|| "down".to_string());
            let (delta_x, delta_y) = match direction.as_str() {
                "left" => (-amount, 0),
                "right" => (amount, 0),
                "up" => (0, amount),
                _ => (0, -amount),
            };
            event["action_type"] = json!("scroll");
            event["delta_x"] = json!(delta_x);
            event["delta_y"] = json!(delta_y);
            event["x"] = value_or_null(args, "x");
            event["y"] = value_or_null(args, "y");
        }
        "sootie_window" => {
            event["action_type"] = json!("window");
            event["command"] = value_or_null(args, "action");
            event["window"] = value_or_null(args, "window");
            event["x"] = value_or_null(args, "x");
            event["y"] = value_or_null(args, "y");
            event["width"] = value_or_null(args, "width");
            event["height"] = value_or_null(args, "height");
        }
        _ => return None,
    }
    Some(event)
}

fn learned_event_is_paste_hotkey(event: &Value) -> bool {
    if event.get("action_type").and_then(Value::as_str) != Some("hotkey") {
        return false;
    }
    let key = event
        .get("key_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !key.eq_ignore_ascii_case("v") {
        return false;
    }
    event
        .get("modifiers")
        .and_then(Value::as_array)
        .is_some_and(|modifiers| {
            modifiers.iter().any(|modifier| {
                modifier.as_str().is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "cmd" | "command" | "meta" | "ctrl" | "control"
                    )
                })
            })
        })
}

fn enrich_learned_event_from_context(event: &mut Value, context: Option<&Value>) {
    let Some(context) = context else {
        return;
    };
    copy_context_string(event, context, "app");
    copy_context_string(event, context, "window");
    copy_context_string(event, context, "url");
    if event.get("element").is_none_or(Value::is_null) {
        if let Some(element) = context.get("focused_element") {
            event["element"] = element.clone();
        }
    }
}

fn enrich_point_coordinate_fields(
    event: &mut Value,
    bounds: &Bounds,
    x_key: &str,
    y_key: &str,
    prefix: &str,
) {
    let Some(x) = event.get(x_key).and_then(Value::as_f64) else {
        return;
    };
    let Some(y) = event.get(y_key).and_then(Value::as_f64) else {
        return;
    };
    let window_x = x - bounds.x;
    let window_y = y - bounds.y;
    event[format!("{prefix}screen_coordinate")] = json!({ "x": x, "y": y });
    event[format!("{prefix}window_coordinate")] = json!({ "x": window_x, "y": window_y });
    event[format!("{prefix}window_normalized_coordinate")] = json!({
        "x": window_x / bounds.width,
        "y": window_y / bounds.height,
    });
    if prefix.is_empty() {
        event["coordinate_space"] = json!("screen");
    }
}

fn copy_context_string(event: &mut Value, context: &Value, key: &str) {
    let should_fill = event
        .get(key)
        .is_none_or(|value| value.is_null() || value.as_str().is_some_and(str::is_empty));
    if should_fill {
        if let Some(value) = context.get(key).and_then(Value::as_str) {
            if !value.is_empty() {
                event[key] = json!(value);
            }
        }
    }
}

fn value_or_null(args: &Value, key: &str) -> Value {
    args.get(key).cloned().unwrap_or(Value::Null)
}

fn point_component_or_null(args: &Value, key: &str, target_key: &str, axis: &str) -> Value {
    finite_f64_arg(args, key)
        .ok()
        .flatten()
        .or_else(|| {
            target_coordinate(args.get(target_key).or_else(|| args.get("target")), axis)
                .ok()
                .flatten()
        })
        .map(|value| json!(value))
        .unwrap_or(Value::Null)
}

fn learned_apps(actions: &[Value]) -> Vec<String> {
    let mut apps = Vec::<String>::new();
    for action in actions {
        for key in ["app", "to_app"] {
            if let Some(app) = action.get(key).and_then(Value::as_str) {
                if !app.is_empty() && !apps.iter().any(|existing| existing == app) {
                    apps.push(app.to_string());
                }
            }
        }
    }
    apps
}

fn learned_urls(actions: &[Value]) -> Vec<String> {
    let mut urls = Vec::<String>::new();
    for action in actions {
        if let Some(url) = action.get("url").and_then(Value::as_str) {
            if !url.is_empty() && !urls.iter().any(|existing| existing == url) {
                urls.push(url.to_string());
            }
        }
    }
    urls
}

fn unix_timestamp_seconds() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or_default()
}

fn format_tool_result(
    name: &str,
    arguments: Value,
    result: SootieResult<ToolResult>,
    duration_ms: u128,
) -> Value {
    let (tool_result, is_error) = match result {
        Ok(result) => {
            let is_error = !result.success;
            (result, is_error)
        }
        Err(error) => {
            let message = error.to_string();
            let mut result = ToolResult::error(message.clone());
            if macos_foreground_permission_error(&message) {
                result = result.with_suggestion(
                    "macOS refused to move the target app to the foreground. Grant Accessibility and Automation permissions to the calling terminal/app, verify the target app has a normal window, then retry.",
                );
            } else if macos_window_accessibility_error(&message) {
                result = result.with_suggestion(
                    "The target app is visible to the window server, but macOS Accessibility did not expose a matching window for raising or focusing. Verify the window title with sootie_state, grant Accessibility/Automation to the launcher, or use a coordinate action against a visible window.",
                );
            } else if screen_capture_locked_recovery_needed(&message) {
                result = result.with_suggestion(
                    "macOS is locked, so UI actions would affect the lock screen instead of the target app. Unlock the Mac, verify the target window is visible, then retry.",
                );
            }
            (result, true)
        }
    };
    let response = json!({
        "success": tool_result.success,
        "data": tool_result.data,
        "error": tool_result.error,
        "suggestion": tool_result.suggestion,
        "context": tool_result.context,
        "report": {
            "tool": name,
            "arguments": arguments,
            "duration_ms": duration_ms,
            "success": tool_result.success,
            "error": tool_result.error,
        }
    });
    let content = content_items(name, &tool_result, &response);
    json!({
        "content": content,
        "structuredContent": response,
        "isError": is_error
    })
}

fn context_tool_result(
    context: crate::types::ContextSnapshot,
    platform: &str,
    requested_app: Option<&str>,
    diagnostics: &[RuntimeDiagnostic],
) -> ToolResult {
    let needs_suggestion = context.window.is_none() && context.interactive_elements.is_empty();
    let mut result = ToolResult::ok_with_context(context.clone(), context.clone());
    if needs_suggestion {
        result = result.with_suggestion(empty_context_suggestion(
            platform,
            context.app.as_deref().or(requested_app),
            diagnostics,
        ));
    }
    result
}

fn tool_error_with_runtime_diagnostic(
    message: String,
    diagnostics: &[RuntimeDiagnostic],
) -> ToolResult {
    let mut result = ToolResult::error(message);
    let diagnostic_note = empty_context_diagnostic_note(diagnostics);
    if !diagnostic_note.is_empty() {
        result = result.with_suggestion(format!(
            "{} Run `sootie doctor --check` for full runtime diagnostics.",
            diagnostic_note.trim()
        ));
    }
    result
}

fn empty_context_suggestion(
    platform: &str,
    app: Option<&str>,
    diagnostics: &[RuntimeDiagnostic],
) -> String {
    let app_label = app
        .filter(|app| !app.trim().is_empty())
        .map(|app| format!(" for '{app}'"))
        .unwrap_or_default();
    let diagnostic_note = empty_context_diagnostic_note(diagnostics);
    if platform == "macos" {
        format!(
            "No accessible window or elements were exposed{app_label}; screen capture is separate from Accessibility. If the screen is visible, grant Accessibility, Automation, and Screen Recording permissions to the app that launched sootie, then restart it. For browser DOM targets, launch the browser with remote debugging enabled.{diagnostic_note}"
        )
    } else {
        format!(
            "No accessible window or elements were exposed{app_label}. Verify the target app has a normal window and that the platform accessibility backend can inspect it.{diagnostic_note}"
        )
    }
}

fn empty_context_diagnostic_note(diagnostics: &[RuntimeDiagnostic]) -> String {
    let Some(diagnostic) = diagnostics.iter().find(|diagnostic| !diagnostic.success) else {
        return String::new();
    };
    let recovery = diagnostic
        .details
        .as_ref()
        .and_then(|details| details.get("recovery"))
        .and_then(Value::as_str)
        .filter(|recovery| !recovery.trim().is_empty())
        .map(|recovery| format!(" Recovery: {recovery}"))
        .unwrap_or_default();
    format!(" Runtime diagnostic: {}.{recovery}", diagnostic.message)
}

fn element_at_miss_suggestion(platform: &str) -> String {
    if platform == "macos" {
        "The screen is capturable, but no accessible element with bounds contains that coordinate. Run sootie_context or sootie_parse_screen to inspect available targets; if the target is browser DOM content, launch the browser with remote debugging enabled for DOM-backed targeting.".to_string()
    } else {
        "No accessible element with bounds contains that coordinate. Run sootie_context or sootie_parse_screen to inspect available targets, then retry with a coordinate inside one of those bounds.".to_string()
    }
}

fn macos_foreground_permission_error(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("frontmost after activation")
        || message.contains("-1743")
        || message.contains("-10827")
        || message.contains("not authorized to send apple events")
        || (message.contains("accessibility") && message.contains("frontmost"))
}

fn macos_window_accessibility_error(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("window not found") && message.contains("osascript failed")
}

fn content_items(name: &str, tool_result: &ToolResult, response: &Value) -> Vec<Value> {
    if tool_result.success {
        if let Some(image) = image_content_item(name, tool_result) {
            return vec![
                image,
                text_content_item(image_tool_text(name, tool_result, response)),
            ];
        }
    }
    vec![text_content_item(pretty_json(response))]
}

fn image_content_item(name: &str, tool_result: &ToolResult) -> Option<Value> {
    let data = tool_result.data.as_ref()?;
    let image_key = match name {
        "sootie_screenshot" => "image",
        "sootie_annotate" => "annotated_image",
        _ => return None,
    };
    let image = data.get(image_key)?.as_str()?;
    let mime_type = data
        .get("mime_type")
        .and_then(Value::as_str)
        .unwrap_or("image/png");
    Some(json!({
        "type": "image",
        "data": image,
        "mimeType": mime_type
    }))
}

fn text_content_item(text: String) -> Value {
    json!({ "type": "text", "text": text })
}

fn image_tool_text(name: &str, tool_result: &ToolResult, response: &Value) -> String {
    let Some(data) = tool_result.data.as_ref() else {
        return pretty_json(response);
    };
    match name {
        "sootie_screenshot" => {
            let title = data
                .get("window_title")
                .and_then(Value::as_str)
                .unwrap_or("screen");
            let width = data.get("width").and_then(Value::as_u64).unwrap_or(0);
            let height = data.get("height").and_then(Value::as_u64).unwrap_or(0);
            let summary = format!("Screenshot: {title} ({width}x{height})");
            match data.get("artifact_path").and_then(Value::as_str) {
                Some(path) => format!("{summary}\nArtifact: {path}"),
                None => summary,
            }
        }
        "sootie_annotate" => {
            let count = data
                .get("element_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let index = data.get("index").and_then(Value::as_str).unwrap_or("");
            if index.is_empty() {
                format!("Annotated screenshot: {count} elements")
            } else {
                format!("Annotated screenshot: {count} elements\n\n{index}")
            }
        }
        _ => pretty_json(response),
    }
}

fn pretty_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn screenshot_tool_result(result: SootieResult<Screenshot>) -> ToolResult {
    match result {
        Ok(screenshot) => ToolResult::ok(screenshot_payload(screenshot)),
        Err(error) => screenshot_error_result(error),
    }
}

fn screenshot_error_result(error: SootieError) -> ToolResult {
    let message = error.to_string();
    let mut result = ToolResult::error(message.clone());
    if screen_capture_locked_recovery_needed(&message) {
        result = result.with_suggestion(
            "macOS is locked, so screenshots would capture the lock screen instead of the target app. Unlock the Mac, verify the target window is visible, then retry.",
        );
    } else if screen_capture_recovery_needed(&message) {
        result = result.with_suggestion(
            "macOS is not exposing an active display to this process. If the screen is visible, grant Screen Recording permission to the terminal or app that launched sootie, restart that process, then retry.",
        );
    }
    result
}

fn screen_capture_locked_recovery_needed(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("screen is locked") || message.contains("lock screen")
}

fn screen_capture_recovery_needed(message: &str) -> bool {
    let message = message.to_lowercase();
    screen_capture_display_unavailable(&message)
        || message.contains("blank black image")
        || message.contains("screen recording permission")
}

fn screen_capture_display_unavailable(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("could not create image from display")
        || message.contains("does not intersect any displays")
}

fn app_payload(app: &AppInfo) -> Value {
    let mut payload = json!({
        "name": &app.name,
        "app_id": &app.app_id,
        "platform_app_id": &app.platform_app_id,
        "bundle_id": &app.bundle_id,
        "pid": app.pid,
        "active": app.is_frontmost,
    });
    if !app.windows.is_empty() {
        payload["windows"] = json!(app.windows.iter().map(window_payload).collect::<Vec<_>>());
    }
    payload
}

fn window_payload(window: &WindowInfo) -> Value {
    let mut payload = json!({ "title": &window.title });
    if let Some(bounds) = &window.bounds {
        payload["position"] = json!({ "x": bounds.x, "y": bounds.y });
        payload["size"] = json!({ "width": bounds.width, "height": bounds.height });
    }
    payload
}

fn element_summary(element: &ElementInfo) -> Value {
    let mut payload = json!({
        "role": &element.role,
        "name": element.name.as_ref().or(element.text.as_ref()),
        "title": element.title.as_ref().or(element.name.as_ref()).or(element.text.as_ref()),
        "actionable": !element.actions.is_empty(),
    });
    if let Some(bounds) = &element.bounds {
        payload["position"] = json!({ "x": bounds.x, "y": bounds.y });
        payload["size"] = json!({ "width": bounds.width, "height": bounds.height });
    }
    if !element.actions.is_empty() {
        payload["actions"] = json!(&element.actions);
    }
    if let Some(identifier) = &element.id {
        payload["identifier"] = json!(identifier);
    }
    payload
}

fn element_full(element: &ElementInfo) -> Value {
    let mut payload = element_summary(element);
    if let Some(bounds) = &element.bounds {
        payload["frame"] = json!({
            "x": bounds.x,
            "y": bounds.y,
            "width": bounds.width,
            "height": bounds.height,
        });
    }
    if let Some(name) = &element.name {
        payload["computed_name"] = json!(name);
    }
    if let Some(title) = element.title.as_ref().or(element.name.as_ref()) {
        payload["title"] = json!(title);
    }
    if let Some(text) = &element.text {
        payload["value"] = json!(text);
    }
    if let Some(editable) = element.editable {
        payload["editable"] = json!(editable);
    }
    if let Some(enabled) = element.enabled {
        payload["enabled"] = json!(enabled);
    }
    if !element.actions.is_empty() {
        payload["supported_actions"] = json!(&element.actions);
    }
    payload
}

fn find_query(args: &Value) -> FindQuery {
    find_query_with_target(args, "target")
}

fn find_query_with_target(args: &Value, target_key: &str) -> FindQuery {
    let target = args.get(target_key).or_else(|| args.get("target"));
    FindQuery {
        query: query_arg(args).or_else(|| target_query(target)),
        role: str_arg(args, "role").or_else(|| target_selector_string(target, "role")),
        dom_id: str_arg(args, "dom_id")
            .or_else(|| target_selector_string(target, "dom_id"))
            .or_else(|| target_selector_string(target, "id")),
        dom_class: str_arg(args, "dom_class")
            .or_else(|| target_selector_string(target, "dom_class"))
            .or_else(|| target_selector_string(target, "class")),
        identifier: str_arg(args, "identifier")
            .or_else(|| target_selector_string(target, "identifier")),
        app: optional_app_arg(args).or_else(|| target_app(target)),
        depth: u32_arg(args, "depth"),
        max_results: u32_arg(args, "max_results").map(|value| value.clamp(1, 500)),
    }
}

fn query_has_target(query: &FindQuery) -> bool {
    query.query.is_some()
        || query.role.is_some()
        || query.dom_id.is_some()
        || query.dom_class.is_some()
        || query.identifier.is_some()
}

fn target_query(target: Option<&Value>) -> Option<String> {
    match target? {
        Value::String(text) => Some(text.clone()),
        Value::Object(_) => target_selector_string(target, "query")
            .or_else(|| target_selector_string(target, "name"))
            .or_else(|| target_selector_string(target, "text"))
            .or_else(|| target_selector_string(target, "computed_name"))
            .or_else(|| target_selector_string(target, "description")),
        _ => None,
    }
}

fn target_selector_string(target: Option<&Value>, key: &str) -> Option<String> {
    let target = target?;
    target
        .get("selector")
        .and_then(|selector| selector.get(key))
        .or_else(|| target.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn target_app(target: Option<&Value>) -> Option<String> {
    let target = target?;
    app_name_value(target.get("app"))
        .or_else(|| object_string_field(Some(target), "app_id"))
        .or_else(|| object_string_field(Some(target), "platform_app_id"))
        .or_else(|| object_string_field(Some(target), "bundle_id"))
}

fn xy_args(
    args: &Value,
    x_key: &str,
    y_key: &str,
    target_key: &str,
) -> SootieResult<(Option<f64>, Option<f64>)> {
    Ok((
        finite_f64_arg(args, x_key)?.or(target_coordinate(args.get(target_key), "x")?),
        finite_f64_arg(args, y_key)?.or(target_coordinate(args.get(target_key), "y")?),
    ))
}

fn point_arg(
    args: &Value,
    x_key: &str,
    y_key: &str,
    target_key: &str,
) -> SootieResult<Option<(f64, f64)>> {
    Ok(match xy_args(args, x_key, y_key, target_key)? {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    })
}

fn target_coordinate(target: Option<&Value>, key: &str) -> SootieResult<Option<f64>> {
    let Some(target) = target else {
        return Ok(None);
    };
    let Some(value) = target
        .get("coordinate")
        .and_then(|coordinate| coordinate.get(key))
        .or_else(|| target.get(key))
    else {
        return Ok(None);
    };
    let value = f64_value_arg(value)
        .ok_or_else(|| SootieError::InvalidArguments(format!("{key} must be a finite number")))?;
    if value.is_finite() {
        Ok(Some(value))
    } else {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a finite number"
        )))
    }
}

fn wait_find_query(args: &Value, app: Option<String>, value: &str) -> FindQuery {
    let mut query = find_query(args);
    if query.app.is_none() {
        query.app = app;
    }
    if query.query.is_none() && !value.is_empty() {
        query.query = Some(value.to_string());
    }
    query
}

fn wait_value_arg(args: &Value, condition: &str) -> Option<String> {
    str_arg(args, "value").or_else(|| match condition {
        "titleContains" | "titleChanged" => str_arg(args, "title"),
        "urlContains" | "urlChanged" => str_arg(args, "url"),
        _ => None,
    })
}

fn target_label(args: &Value) -> Option<String> {
    query_arg(args).or_else(|| target_query(args.get("target")))
}

fn query_arg(args: &Value) -> Option<String> {
    str_arg(args, "query")
        .or_else(|| str_arg(args, "description"))
        .or_else(|| str_arg(args, "el_description"))
}

fn screenshot_payload(screenshot: Screenshot) -> Value {
    let mut payload = json!({
        "image": screenshot.data_base64,
        "width": screenshot.width,
        "height": screenshot.height,
        "window_title": screenshot.window_title,
        "mime_type": screenshot.mime_type,
    });
    if let Some(artifact) = persist_image_artifact(
        payload.get("image").and_then(Value::as_str).unwrap_or(""),
        payload
            .get("mime_type")
            .and_then(Value::as_str)
            .unwrap_or("image/png"),
        "screenshot",
    ) {
        payload["artifact_path"] = json!(artifact.path);
        payload["artifact_uri"] = json!(artifact.uri);
    }
    if let Some(frame) = screenshot.window_frame {
        payload["window_frame"] = json!({
            "x": frame.x,
            "y": frame.y,
            "width": frame.width,
            "height": frame.height,
        });
    }
    payload
}

struct ImageArtifact {
    path: String,
    uri: String,
}

fn persist_image_artifact(data_base64: &str, mime_type: &str, stem: &str) -> Option<ImageArtifact> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64)
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    let artifact_dir = std::env::temp_dir().join("sootie-artifacts");
    fs::create_dir_all(&artifact_dir).ok()?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let sequence = IMAGE_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = artifact_dir.join(format!(
        "{}-{}-{}-{}.{}",
        stem,
        std::process::id(),
        nanos,
        sequence,
        image_extension(mime_type)
    ));
    fs::write(&path, bytes).ok()?;
    Some(ImageArtifact {
        path: path.to_string_lossy().to_string(),
        uri: file_uri(&path),
    })
}

fn persist_grounding_history_screenshot(
    screenshot: &Screenshot,
    description: &str,
    frame: &VisionFrame,
    crop_box: Option<[f64; 4]>,
    result: &GroundResult,
) -> Option<VisionHistoryArtifact> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&screenshot.data_base64)
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    let history_dir = Path::new(VISION_GROUNDING_HISTORY_DIR);
    fs::create_dir_all(history_dir).ok()?;
    let (image_path, metadata_path) = vision_history_paths(history_dir, description)?;
    let annotations = grounding_annotations(result, frame);
    let annotated_jpeg = grounding_annotated_jpeg(&bytes, description, frame, &annotations)?;
    fs::write(&image_path, annotated_jpeg).ok()?;
    let artifact = VisionHistoryArtifact {
        image_uri: file_uri(&image_path),
        image_path: image_path.to_string_lossy().to_string(),
        metadata_uri: file_uri(&metadata_path),
        metadata_path: metadata_path.to_string_lossy().to_string(),
        image_mime_type: VISION_GROUNDING_SCREENSHOT_MIME_TYPE,
    };
    let metadata = json!({
        "feature": "grounding",
        "description": description,
        "prompt": description,
        "screenshot_mime_type": artifact.image_mime_type,
        "original_mime_type": &screenshot.mime_type,
        "width": screenshot.width,
        "height": screenshot.height,
        "window_title": &screenshot.window_title,
        "window_frame": &screenshot.window_frame,
        "history_root_dir": VISION_HISTORY_ROOT_DIR,
        "history_dir": VISION_GROUNDING_HISTORY_DIR,
        "grounding_frame": frame.payload(),
        "crop_box": crop_box,
        "screenshot_path": &artifact.image_path,
        "screenshot_uri": &artifact.image_uri,
        "metadata_path": &artifact.metadata_path,
        "metadata_uri": &artifact.metadata_uri,
        "predictions": grounding_annotations_payload(&annotations),
        "result": {
            "x": result.x,
            "y": result.y,
            "confidence": result.confidence,
            "method": &result.method,
            "inference_ms": result.inference_ms,
            "raw": &result.raw_text,
            "bounds": &result.bounds,
            "sidecar_response": &result.response,
        },
    });
    fs::write(&metadata_path, serde_json::to_vec_pretty(&metadata).ok()?).ok()?;
    Some(artifact)
}

fn vision_history_paths(
    history_dir: &Path,
    description: &str,
) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let sequence = IMAGE_ARTIFACT_COUNTER.fetch_add(1, Ordering::Relaxed);
    let label = vision_history_label(description);
    let base = format!("grounding-{}-{}-{}", nanos, sequence, label);
    Some((
        history_dir.join(format!("{base}.jpg")),
        history_dir.join(format!("{base}.json")),
    ))
}

fn vision_history_label(description: &str) -> String {
    let label = stable_identifier_text(description);
    if label.is_empty() {
        "target".to_string()
    } else {
        label.chars().take(48).collect()
    }
}

fn grounding_annotations(result: &GroundResult, frame: &VisionFrame) -> Vec<GroundingAnnotation> {
    let annotations = result
        .response
        .get("matches")
        .and_then(Value::as_array)
        .map(|matches| {
            matches
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    grounding_annotation_from_value(index + 1, value, frame)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !annotations.is_empty() {
        return annotations;
    }

    let Some(bounds) = grounding_bounds_from_value(&result.response, frame)
        .or_else(|| result.bounds.clone())
        .or_else(|| grounding_point_bounds(result.x, result.y, frame))
        .and_then(|bounds| clamp_grounding_bounds(bounds, frame))
    else {
        return Vec::new();
    };
    let confidence = Some(result.confidence);
    let label = grounding_prediction_label(&result.response).or_else(|| result.raw_text.clone());
    vec![GroundingAnnotation {
        index: 1,
        bounds,
        value: grounding_prediction_value(label.as_deref(), confidence),
        confidence,
        label,
    }]
}

fn grounding_annotation_from_value(
    index: usize,
    value: &Value,
    frame: &VisionFrame,
) -> Option<GroundingAnnotation> {
    let bounds = grounding_bounds_from_value(value, frame).or_else(|| {
        grounding_point_from_value(value.get("point"), frame)
            .and_then(|(x, y)| grounding_point_bounds(x, y, frame))
    })?;
    let confidence = grounding_numeric_value(value, &["confidence", "score", "probability"]);
    let label = grounding_prediction_label(value);
    Some(GroundingAnnotation {
        index,
        bounds: clamp_grounding_bounds(bounds, frame)?,
        value: grounding_prediction_value(label.as_deref(), confidence),
        confidence,
        label,
    })
}

fn grounding_bounds_from_value(value: &Value, frame: &VisionFrame) -> Option<Bounds> {
    ["bbox", "bounds", "box"]
        .iter()
        .find_map(|key| value.get(*key))
        .and_then(|bounds| parse_grounding_bounds(bounds, frame.width, frame.height))
        .and_then(|bounds| clamp_grounding_bounds(bounds, frame))
}

fn parse_grounding_bounds(value: &Value, screen_width: f64, screen_height: f64) -> Option<Bounds> {
    match value {
        Value::Array(values) if values.len() >= 4 => {
            let first = values.first()?.as_f64()?;
            let second = values.get(1)?.as_f64()?;
            let third = values.get(2)?.as_f64()?;
            let fourth = values.get(3)?.as_f64()?;
            if third > first && fourth > second {
                bounds_from_xyxy(first, second, third, fourth, screen_width, screen_height)
            } else {
                bounds_from_xywh(first, second, third, fourth, screen_width, screen_height)
            }
        }
        Value::Object(object) => {
            if let (Some(x), Some(y), Some(width), Some(height)) = (
                object.get("x").and_then(Value::as_f64),
                object.get("y").and_then(Value::as_f64),
                object.get("width").and_then(Value::as_f64),
                object.get("height").and_then(Value::as_f64),
            ) {
                return bounds_from_xywh(x, y, width, height, screen_width, screen_height);
            }
            if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                object
                    .get("x1")
                    .or_else(|| object.get("left"))
                    .and_then(Value::as_f64),
                object
                    .get("y1")
                    .or_else(|| object.get("top"))
                    .and_then(Value::as_f64),
                object
                    .get("x2")
                    .or_else(|| object.get("right"))
                    .and_then(Value::as_f64),
                object
                    .get("y2")
                    .or_else(|| object.get("bottom"))
                    .and_then(Value::as_f64),
            ) {
                return bounds_from_xyxy(x1, y1, x2, y2, screen_width, screen_height);
            }
            let position = object.get("position")?;
            let size = object.get("size")?;
            bounds_from_xywh(
                position.get("x")?.as_f64()?,
                position.get("y")?.as_f64()?,
                size.get("width")?.as_f64()?,
                size.get("height")?.as_f64()?,
                screen_width,
                screen_height,
            )
        }
        _ => None,
    }
}

fn bounds_from_xywh(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    screen_width: f64,
    screen_height: f64,
) -> Option<Bounds> {
    Some(Bounds {
        x: scale_grounding_coordinate(x, screen_width),
        y: scale_grounding_coordinate(y, screen_height),
        width: scale_grounding_size(width, screen_width),
        height: scale_grounding_size(height, screen_height),
    })
}

fn bounds_from_xyxy(
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    screen_width: f64,
    screen_height: f64,
) -> Option<Bounds> {
    let x1 = scale_grounding_coordinate(x1, screen_width);
    let y1 = scale_grounding_coordinate(y1, screen_height);
    let x2 = scale_grounding_coordinate(x2, screen_width);
    let y2 = scale_grounding_coordinate(y2, screen_height);
    Some(Bounds {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    })
}

fn grounding_point_from_value(value: Option<&Value>, frame: &VisionFrame) -> Option<(f64, f64)> {
    match value? {
        Value::Array(values) => Some((
            scale_grounding_coordinate(values.first()?.as_f64()?, frame.width),
            scale_grounding_coordinate(values.get(1)?.as_f64()?, frame.height),
        )),
        Value::Object(object) => Some((
            scale_grounding_coordinate(object.get("x")?.as_f64()?, frame.width),
            scale_grounding_coordinate(object.get("y")?.as_f64()?, frame.height),
        )),
        _ => None,
    }
}

fn grounding_point_bounds(x: f64, y: f64, frame: &VisionFrame) -> Option<Bounds> {
    clamp_grounding_bounds(
        Bounds {
            x: x - 20.0,
            y: y - 20.0,
            width: 40.0,
            height: 40.0,
        },
        frame,
    )
}

fn clamp_grounding_bounds(bounds: Bounds, frame: &VisionFrame) -> Option<Bounds> {
    if !bounds.x.is_finite()
        || !bounds.y.is_finite()
        || !bounds.width.is_finite()
        || !bounds.height.is_finite()
        || bounds.width <= 0.0
        || bounds.height <= 0.0
    {
        return None;
    }
    let x1 = bounds.x.clamp(0.0, frame.width);
    let y1 = bounds.y.clamp(0.0, frame.height);
    let x2 = (bounds.x + bounds.width).clamp(0.0, frame.width);
    let y2 = (bounds.y + bounds.height).clamp(0.0, frame.height);
    (x2 > x1 && y2 > y1).then_some(Bounds {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    })
}

fn scale_grounding_coordinate(value: f64, size: f64) -> f64 {
    if value.abs() <= 1.0 && size > 0.0 {
        value * size
    } else {
        value
    }
}

fn scale_grounding_size(value: f64, size: f64) -> f64 {
    if (0.0..=1.0).contains(&value) && size > 0.0 {
        value * size
    } else {
        value
    }
}

fn grounding_numeric_value(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(Value::as_f64)
}

fn grounding_prediction_label(value: &Value) -> Option<String> {
    [
        "label",
        "text",
        "name",
        "class",
        "prediction",
        "value",
        "raw",
        "caption",
    ]
    .iter()
    .find_map(|key| value.get(*key))
    .and_then(|value| match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

fn grounding_prediction_value(label: Option<&str>, confidence: Option<f64>) -> String {
    match (label, confidence) {
        (Some(label), Some(confidence)) => format!("{label} ({confidence:.2})"),
        (Some(label), None) => label.to_string(),
        (None, Some(confidence)) => format!("{confidence:.2}"),
        (None, None) => "prediction".to_string(),
    }
}

fn grounding_annotations_payload(annotations: &[GroundingAnnotation]) -> Vec<Value> {
    annotations
        .iter()
        .map(|annotation| {
            json!({
                "index": annotation.index,
                "bbox": {
                    "x": annotation.bounds.x,
                    "y": annotation.bounds.y,
                    "width": annotation.bounds.width,
                    "height": annotation.bounds.height,
                },
                "value": &annotation.value,
                "confidence": annotation.confidence,
                "label": &annotation.label,
            })
        })
        .collect()
}

fn grounding_annotated_jpeg(
    screenshot_bytes: &[u8],
    description: &str,
    frame: &VisionFrame,
    annotations: &[GroundingAnnotation],
) -> Option<Vec<u8>> {
    let mut image = image::load_from_memory(screenshot_bytes).ok()?.to_rgba8();
    let image_width = image.width().max(1);
    let image_height = image.height().max(1);
    let prompt_text = format!("Prompt: {}", truncate_text(description, 160));
    let prompt_width = raster_text_box_width(&prompt_text, image_width.saturating_sub(16));
    fill_rect(&mut image, 8, 8, prompt_width, 28, [255, 255, 255, 235]);
    draw_text(&mut image, 18, 17, &prompt_text, 2, [17, 24, 39, 255]);

    for annotation in annotations {
        let color = grounding_annotation_color(annotation.index);
        let color = hex_color(color);
        let (x, y, width, height) =
            raster_bounds(&annotation.bounds, frame, image_width, image_height)?;
        let label = format!(
            "#{} {}",
            annotation.index,
            truncate_text(&annotation.value, 80)
        );
        let label_width = raster_text_box_width(&label, image_width.saturating_sub(8));
        let label_x = raster_label_x(x, width, label_width, image_width);
        let label_y = raster_label_y(y, height, image_height);
        draw_rect(&mut image, x, y, width, height, 3, color);
        fill_rect(&mut image, label_x, label_y, label_width, 24, color);
        draw_text(
            &mut image,
            label_x + 7,
            label_y + 7,
            &label,
            2,
            [255, 255, 255, 255],
        );
    }

    let rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let mut output = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
        &mut output,
        VISION_GROUNDING_JPEG_QUALITY,
    );
    encoder
        .encode(
            &rgb,
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .ok()?;
    Some(output)
}

fn raster_bounds(
    bounds: &Bounds,
    frame: &VisionFrame,
    image_width: u32,
    image_height: u32,
) -> Option<(i32, i32, i32, i32)> {
    let scale_x = image_width as f64 / frame.width.max(1.0);
    let scale_y = image_height as f64 / frame.height.max(1.0);
    let x1 = (bounds.x * scale_x).round().clamp(0.0, image_width as f64) as i32;
    let y1 = (bounds.y * scale_y).round().clamp(0.0, image_height as f64) as i32;
    let x2 = ((bounds.x + bounds.width) * scale_x)
        .round()
        .clamp(0.0, image_width as f64) as i32;
    let y2 = ((bounds.y + bounds.height) * scale_y)
        .round()
        .clamp(0.0, image_height as f64) as i32;
    (x2 > x1 && y2 > y1).then_some((x1, y1, x2 - x1, y2 - y1))
}

fn raster_text_box_width(text: &str, max_width: u32) -> i32 {
    let available = max_width.max(32) as i32;
    let desired = text.chars().count() as i32 * 12 + 22;
    desired.max(32).min(available)
}

fn raster_label_x(x: i32, width: i32, label_width: i32, image_width: u32) -> i32 {
    let image_width = image_width as i32;
    let right = x + width + 6;
    if right + label_width + 4 <= image_width {
        return right.max(4);
    }
    (x - label_width - 6).max(4)
}

fn raster_label_y(y: i32, height: i32, image_height: u32) -> i32 {
    let image_height = image_height as i32;
    if y >= 30 {
        return (y - 28).max(4);
    }
    (y + height + 4).min((image_height - 28).max(4))
}

fn draw_rect(
    image: &mut image::RgbaImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    stroke: i32,
    color: [u8; 4],
) {
    fill_rect(image, x, y, width, stroke, color);
    fill_rect(image, x, y + height - stroke, width, stroke, color);
    fill_rect(image, x, y, stroke, height, color);
    fill_rect(image, x + width - stroke, y, stroke, height, color);
}

fn fill_rect(
    image: &mut image::RgbaImage,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [u8; 4],
) {
    let x1 = x.max(0) as u32;
    let y1 = y.max(0) as u32;
    let x2 = (x + width).max(0).min(image.width() as i32) as u32;
    let y2 = (y + height).max(0).min(image.height() as i32) as u32;
    for pixel_y in y1..y2 {
        for pixel_x in x1..x2 {
            blend_pixel(image.get_pixel_mut(pixel_x, pixel_y), color);
        }
    }
}

fn draw_text(image: &mut image::RgbaImage, x: i32, y: i32, text: &str, scale: i32, color: [u8; 4]) {
    let mut cursor_x = x;
    for character in text.chars() {
        if character == ' ' {
            cursor_x += 4 * scale;
            continue;
        }
        draw_glyph(image, cursor_x, y, character, scale, color);
        cursor_x += 6 * scale;
        if cursor_x >= image.width() as i32 - 4 {
            break;
        }
    }
}

fn draw_glyph(
    image: &mut image::RgbaImage,
    x: i32,
    y: i32,
    character: char,
    scale: i32,
    color: [u8; 4],
) {
    let glyph = glyph_rows(character);
    for (row_index, row) in glyph.iter().enumerate() {
        for column in 0..5 {
            if row & (1 << (4 - column)) != 0 {
                fill_rect(
                    image,
                    x + column * scale,
                    y + row_index as i32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
}

fn blend_pixel(pixel: &mut image::Rgba<u8>, color: [u8; 4]) {
    let alpha = color[3] as u16;
    let inverse = 255 - alpha;
    for channel in 0..3 {
        pixel[channel] =
            ((color[channel] as u16 * alpha + pixel[channel] as u16 * inverse) / 255) as u8;
    }
    pixel[3] = 255;
}

fn hex_color(value: &str) -> [u8; 4] {
    let value = value.trim_start_matches('#');
    if value.len() != 6 {
        return [220, 38, 38, 255];
    }
    let red = u8::from_str_radix(&value[0..2], 16).unwrap_or(220);
    let green = u8::from_str_radix(&value[2..4], 16).unwrap_or(38);
    let blue = u8::from_str_radix(&value[4..6], 16).unwrap_or(38);
    [red, green, blue, 255]
}

fn glyph_rows(character: char) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '_' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b11111,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        _ => [
            0b11111, 0b10001, 0b00101, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
    }
}

fn grounding_annotation_color(index: usize) -> &'static str {
    const COLORS: [&str; 6] = [
        "#dc2626", "#2563eb", "#16a34a", "#d97706", "#7c3aed", "#0891b2",
    ];
    COLORS[(index.saturating_sub(1)) % COLORS.len()]
}

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut output = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    output.push_str("...");
    output
}

fn image_extension(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn file_uri(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        format!("file:///{}", path.trim_start_matches('/'))
    } else {
        format!("file://{path}")
    }
}

fn annotated_svg_image(
    screenshot_base64: &str,
    screenshot_mime_type: &str,
    width: u32,
    height: u32,
    elements: &[ElementInfo],
) -> String {
    let width = width.max(1);
    let height = height.max(1);
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    if !screenshot_base64.is_empty() {
        svg.push_str(&format!(
            r#"<image width="100%" height="100%" preserveAspectRatio="none" href="data:{};base64,{}"/>"#,
            xml_escape(screenshot_mime_type),
            screenshot_base64
        ));
    }
    for (index, element) in elements.iter().enumerate() {
        let Some(bounds) = &element.bounds else {
            continue;
        };
        let label = index + 1;
        let label_x = bounds.x.max(0.0);
        let label_y = (bounds.y - 22.0).max(0.0);
        svg.push_str(&format!(
            r##"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="none" stroke="#2563eb" stroke-width="2"/><g><rect x="{:.1}" y="{:.1}" width="24" height="20" rx="4" fill="#2563eb"/><text x="{:.1}" y="{:.1}" fill="#ffffff" font-family="Arial, sans-serif" font-size="13" font-weight="700" text-anchor="middle">{}</text></g>"##,
            bounds.x.max(0.0),
            bounds.y.max(0.0),
            bounds.width.max(1.0),
            bounds.height.max(1.0),
            label_x,
            label_y,
            label_x + 12.0,
            label_y + 14.0,
            label
        ));
    }
    svg.push_str("</svg>");
    base64::engine::general_purpose::STANDARD.encode(svg.as_bytes())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn role_matches(role: &str, filters: &[String]) -> bool {
    filters.iter().any(|filter| {
        role.eq_ignore_ascii_case(filter)
            || role
                .strip_prefix("AX")
                .is_some_and(|stripped| stripped.eq_ignore_ascii_case(filter))
            || filter
                .strip_prefix("AX")
                .is_some_and(|stripped| role.eq_ignore_ascii_case(stripped))
    })
}

fn element_matches_find_query(element: &ElementInfo, query: &FindQuery) -> bool {
    if let Some(role) = &query.role {
        if !role_matches(&element.role, std::slice::from_ref(role)) {
            return false;
        }
    }
    if let Some(identifier) = &query.identifier {
        if !element_field_matches(element.id.as_deref(), identifier)
            && !element_text_matches_find_query(element, identifier)
        {
            return false;
        }
    }
    if let Some(dom_id) = &query.dom_id {
        if !element_field_matches(element.id.as_deref(), dom_id) {
            return false;
        }
    }
    if let Some(dom_class) = &query.dom_class {
        if !element_text_matches_find_query(element, dom_class) {
            return false;
        }
    }
    if let Some(text) = &query.query {
        if !element_text_matches_find_query(element, text) {
            return false;
        }
    }
    true
}

fn element_text_matches_find_query(element: &ElementInfo, needle: &str) -> bool {
    element_field_matches(element.name.as_deref(), needle)
        || element_field_matches(element.title.as_deref(), needle)
        || element_field_matches(element.text.as_deref(), needle)
        || element_field_matches(element.id.as_deref(), needle)
        || element_field_matches(Some(&element.role), needle)
}

fn element_field_matches(value: Option<&str>, needle: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    value
        .map(|value| value.to_lowercase().contains(&needle))
        .unwrap_or(false)
}

fn vision_description(query: &FindQuery) -> Option<String> {
    query
        .query
        .as_ref()
        .or(query.identifier.as_ref())
        .or(query.dom_id.as_ref())
        .or(query.dom_class.as_ref())
        .or(query.role.as_ref())
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn should_attempt_vision_action_fallback(
    error: &SootieError,
    x: Option<f64>,
    y: Option<f64>,
    query: &FindQuery,
) -> bool {
    x.is_none()
        && y.is_none()
        && query_has_target(query)
        && matches!(error, SootieError::NotFound(_))
}

fn vision_ground_payload(
    description: &str,
    crop_box: Option<(f64, f64, f64, f64)>,
    grounding: VisionMappedGrounding,
    duration: Duration,
) -> Value {
    let candidate = vision_candidate_payload(description, &grounding);
    let mut payload = json!({
        "description": description,
        "candidates": [candidate],
        "source": "vision-ground",
        "durationMs": duration.as_millis(),
        "confidence": grounding.result.confidence,
        "x": grounding.x,
        "y": grounding.y,
        "method": grounding.result.method,
        "grounding_frame": grounding.frame.payload(),
        "screen_size": {
            "width": grounding.screenshot.width,
            "height": grounding.screenshot.height,
        },
        "raw": grounding.result.raw_text,
        "sidecar_response": grounding.result.response,
    });
    if let Some(inference_ms) = grounding.result.inference_ms {
        payload["inference_ms"] = json!(inference_ms);
    }
    if let Some(window_frame) = &grounding.screenshot.window_frame {
        payload["window_frame"] = json!(window_frame);
    }
    if let Some(history) = &grounding.history {
        payload["vision_history_root_dir"] = json!(VISION_HISTORY_ROOT_DIR);
        payload["vision_history_dir"] = json!(VISION_GROUNDING_HISTORY_DIR);
        payload["vision_screenshot_path"] = json!(&history.image_path);
        payload["vision_screenshot_uri"] = json!(&history.image_uri);
        payload["vision_screenshot_mime_type"] = json!(history.image_mime_type);
        payload["vision_metadata_path"] = json!(&history.metadata_path);
        payload["vision_metadata_uri"] = json!(&history.metadata_uri);
    }
    if let Some((x1, y1, x2, y2)) = crop_box {
        payload["crop_box"] = json!([x1, y1, x2, y2]);
    }
    payload
}

fn vision_candidate_payload(description: &str, grounding: &VisionMappedGrounding) -> Value {
    let bounds = grounding.synthetic_bounds();
    json!({
        "role": "VisionTarget",
        "name": description,
        "title": description,
        "actionable": true,
        "position": { "x": bounds.x, "y": bounds.y },
        "size": { "width": bounds.width, "height": bounds.height },
        "actions": ["click", "hover"],
        "confidence": grounding.result.confidence,
    })
}

fn vision_action_result(
    method: &str,
    direct: ActionResult,
    description: &str,
    grounding: &VisionMappedGrounding,
) -> ActionResult {
    ActionResult {
        method: method.to_string(),
        details: json!({
            "method": method,
            "description": description,
            "x": grounding.x,
            "y": grounding.y,
            "confidence": grounding.result.confidence,
            "grounding_method": grounding.result.method,
            "grounding_frame": grounding.frame.payload(),
            "vision_history": grounding.history.as_ref().map(|history| json!({
                "root_dir": VISION_HISTORY_ROOT_DIR,
                "dir": VISION_GROUNDING_HISTORY_DIR,
                "screenshot_path": &history.image_path,
                "screenshot_uri": &history.image_uri,
                "screenshot_mime_type": history.image_mime_type,
                "metadata_path": &history.metadata_path,
                "metadata_uri": &history.metadata_uri,
            })),
            "dispatch": action_payload(direct),
        }),
    }
}

fn stable_identifier_text(value: &str) -> String {
    let mut identifier = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while identifier.contains("--") {
        identifier = identifier.replace("--", "-");
    }
    identifier.trim_matches('-').to_string()
}

fn action_payload(result: ActionResult) -> Value {
    let mut payload = match result.details {
        Value::Object(map) => Value::Object(map),
        other if other.is_null() => json!({}),
        other => json!({ "details": other }),
    };
    if let Value::Object(map) = &mut payload {
        map.entry("method".to_string())
            .or_insert_with(|| json!(result.method));
    }
    payload
}

fn partial_bounds_arg(args: &Value) -> SootieResult<[Option<f64>; 4]> {
    Ok([
        optional_finite_bound_arg(args, "x", 0)?,
        optional_finite_bound_arg(args, "y", 1)?,
        optional_finite_bound_arg(args, "width", 2)?,
        optional_finite_bound_arg(args, "height", 3)?,
    ])
}

fn optional_finite_bound_arg(args: &Value, key: &str, index: usize) -> SootieResult<Option<f64>> {
    let value = args
        .get(key)
        .or_else(|| bounds_object_arg(args, key))
        .or_else(|| bounds_array_arg(args, index));
    let Some(value) = value else {
        return Ok(None);
    };
    let value = f64_value_arg(value)
        .ok_or_else(|| SootieError::InvalidArguments(format!("{key} must be a finite number")))?;
    if value.is_finite() {
        Ok(Some(value))
    } else {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a finite number"
        )))
    }
}

fn bounds_object_arg<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    let bounds = args.get("bounds")?;
    bounds.get(key).or_else(|| match key {
        "x" | "y" => bounds
            .get("position")
            .and_then(|position| position.get(key)),
        "width" | "height" => bounds.get("size").and_then(|size| size.get(key)),
        _ => None,
    })
}

fn bounds_array_arg(args: &Value, index: usize) -> Option<&Value> {
    args.get("bounds")?.as_array()?.get(index)
}

fn validate_window_bounds(bounds: Bounds) -> SootieResult<Bounds> {
    if !bounds.x.is_finite() || !bounds.y.is_finite() {
        return Err(SootieError::InvalidArguments(
            "x/y must be finite numbers".to_string(),
        ));
    }
    if !bounds.width.is_finite() || !bounds.height.is_finite() {
        return Err(SootieError::InvalidArguments(
            "width/height must be finite numbers".to_string(),
        ));
    }
    if bounds.width <= 0.0 || bounds.height <= 0.0 {
        return Err(SootieError::InvalidArguments(
            "width/height must be positive numbers".to_string(),
        ));
    }
    Ok(bounds)
}

fn crop_box_arg(args: &Value) -> SootieResult<Option<(f64, f64, f64, f64)>> {
    let Some(values) = args.get("crop_box") else {
        return Ok(None);
    };
    let Some(items) = values.as_array() else {
        return Err(SootieError::InvalidArguments(
            "crop_box must be [x1, y1, x2, y2]".to_string(),
        ));
    };
    if items.len() != 4 {
        return Err(SootieError::InvalidArguments(
            "crop_box must contain exactly four numbers".to_string(),
        ));
    }
    let mut numbers = [0.0; 4];
    for (index, item) in items.iter().enumerate() {
        numbers[index] = f64_value_arg(item).ok_or_else(|| {
            SootieError::InvalidArguments("crop_box values must be numbers".to_string())
        })?;
        if !numbers[index].is_finite() {
            return Err(SootieError::InvalidArguments(
                "crop_box values must be finite numbers".to_string(),
            ));
        }
    }
    Ok(Some((numbers[0], numbers[1], numbers[2], numbers[3])))
}

fn candidate_in_crop(candidate: &ElementInfo, crop_box: Option<(f64, f64, f64, f64)>) -> bool {
    let Some((x1, y1, x2, y2)) = crop_box else {
        return true;
    };
    let Some(bounds) = &candidate.bounds else {
        return false;
    };
    let center = bounds.center();
    let min_x = x1.min(x2);
    let max_x = x1.max(x2);
    let min_y = y1.min(y2);
    let max_y = y1.max(y2);
    center.x >= min_x && center.x <= max_x && center.y >= min_y && center.y <= max_y
}

fn merge_candidate_elements(candidates: &mut Vec<ElementInfo>, elements: Vec<ElementInfo>) {
    for element in elements {
        if !candidates
            .iter()
            .any(|candidate| same_candidate(candidate, &element))
        {
            candidates.push(element);
        }
    }
}

fn same_candidate(left: &ElementInfo, right: &ElementInfo) -> bool {
    if left.id.is_some() && left.id == right.id {
        return true;
    }
    left.role == right.role
        && left.name == right.name
        && left.text == right.text
        && left.bounds == right.bounds
}

fn ranked_ground_candidates(description: &str, candidates: Vec<ElementInfo>) -> Vec<ElementInfo> {
    let mut scored = candidates
        .into_iter()
        .map(|candidate| {
            let score = ground_candidate_score(description, &candidate);
            (candidate, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect::<Vec<_>>();
    scored.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .partial_cmp(left_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| candidate_order(left).cmp(&candidate_order(right)))
    });
    scored.into_iter().map(|(candidate, _)| candidate).collect()
}

fn ground_candidate_score(description: &str, candidate: &ElementInfo) -> f64 {
    let tokens = search_tokens(description);
    if tokens.is_empty() {
        return candidate.bounds.as_ref().map(|_| 0.25).unwrap_or(0.0);
    }
    let description = description.to_lowercase();
    let name = candidate.name.as_deref().unwrap_or("").to_lowercase();
    let text = candidate.text.as_deref().unwrap_or("").to_lowercase();
    let role = candidate.role.to_lowercase();
    let id = candidate.id.as_deref().unwrap_or("").to_lowercase();
    if exact_or_phrase_match(&description, &name) || exact_or_phrase_match(&description, &text) {
        return bounded_ground_score(1.0, candidate);
    }
    let haystack = format!("{name} {text} {role} {id}");
    let matched = tokens
        .iter()
        .filter(|token| haystack.contains(token.as_str()))
        .count();
    let coverage = matched as f64 / tokens.len() as f64;
    let mut score = coverage * 0.7;
    if phrase_contains(&description, &name) {
        score += 0.2;
    }
    if phrase_contains(&description, &text) {
        score += 0.15;
    }
    if !role.is_empty() && tokens.iter().any(|token| role.contains(token)) {
        score += 0.1;
    }
    if role_action_hint_matches(&tokens, candidate) {
        score += 0.1;
    }
    if !candidate.actions.is_empty() {
        score += 0.05;
    }
    if candidate.editable == Some(true) && input_hint_matches(&tokens) {
        score += 0.15;
    }
    bounded_ground_score(score, candidate)
}

fn exact_or_phrase_match(description: &str, value: &str) -> bool {
    !value.is_empty() && (description == value || description.contains(value))
}

fn phrase_contains(description: &str, value: &str) -> bool {
    !value.is_empty() && (description.contains(value) || value.contains(description))
}

fn role_action_hint_matches(tokens: &[String], candidate: &ElementInfo) -> bool {
    tokens.iter().any(|token| {
        let role = candidate.role.to_lowercase();
        let actions = candidate.actions.join(" ").to_lowercase();
        match token.as_str() {
            "button" | "click" | "press" => role.contains("button") || actions.contains("click"),
            "field" | "input" | "text" | "type" | "search" => {
                role.contains("field") || role.contains("edit") || actions.contains("setvalue")
            }
            "link" => role.contains("link"),
            "menu" => role.contains("menu"),
            "checkbox" | "check" => role.contains("check"),
            _ => false,
        }
    })
}

fn input_hint_matches(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "field" | "input" | "text" | "type" | "search" | "textbox"
        )
    })
}

fn bounded_ground_score(mut score: f64, candidate: &ElementInfo) -> f64 {
    if candidate.enabled == Some(false) {
        score *= 0.5;
    }
    if candidate.bounds.is_none() {
        score *= 0.4;
    }
    score.min(1.0)
}

fn candidate_order(candidate: &ElementInfo) -> String {
    format!(
        "{}:{}:{}:{}",
        candidate.role,
        candidate.name.as_deref().unwrap_or(""),
        candidate.text.as_deref().unwrap_or(""),
        candidate.id.as_deref().unwrap_or("")
    )
}

fn search_tokens(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 2)
        .map(|token| token.to_lowercase())
        .collect()
}

fn window_command(value: &str) -> SootieResult<WindowCommand> {
    match value {
        "list" => Ok(WindowCommand::List),
        "focus" => Ok(WindowCommand::Focus),
        "minimize" => Ok(WindowCommand::Minimize),
        "maximize" => Ok(WindowCommand::Maximize),
        "restore" => Ok(WindowCommand::Restore),
        "close" => Ok(WindowCommand::Close),
        "move" => Ok(WindowCommand::Move),
        "resize" => Ok(WindowCommand::Resize),
        other => Err(SootieError::InvalidArguments(format!(
            "unknown window action '{other}'"
        ))),
    }
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn app_arg(args: &Value) -> Option<String> {
    app_name_value(args.get("app")).or_else(|| app_name_value(args.get("to_app")))
}

fn optional_app_arg(args: &Value) -> Option<String> {
    app_arg(args).or_else(|| bundle_arg(args))
}

fn screenshot_window_arg(args: &Value, app: Option<&str>) -> SootieResult<Option<String>> {
    let window = str_arg(args, "window");
    if window.is_some() && app.is_none() {
        return Err(SootieError::InvalidArguments(
            "window requires app for app-scoped screenshot capture".to_string(),
        ));
    }
    Ok(window)
}

fn bundle_arg(args: &Value) -> Option<String> {
    str_arg(args, "platform_app_id")
        .or_else(|| str_arg(args, "to_platform_app_id"))
        .or_else(|| str_arg(args, "bundle_id"))
        .or_else(|| str_arg(args, "to_bundle_id"))
        .or_else(|| object_string_field(args.get("app"), "platform_app_id"))
        .or_else(|| object_string_field(args.get("to_app"), "platform_app_id"))
        .or_else(|| object_string_field(args.get("app"), "bundle_id"))
        .or_else(|| object_string_field(args.get("to_app"), "bundle_id"))
}

fn required_app_arg(args: &Value) -> SootieResult<String> {
    app_arg(args)
        .or_else(|| bundle_arg(args))
        .ok_or_else(|| SootieError::InvalidArguments("app is required".to_string()))
}

fn app_name_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(name) => Some(name.clone()),
        Value::Object(map) => ["name", "app", "app_id", "platform_app_id", "bundle_id"]
            .iter()
            .find_map(|key| map.get(*key).and_then(Value::as_str))
            .map(str::to_string),
        _ => None,
    }
}

fn object_string_field(value: Option<&Value>, key: &str) -> Option<String> {
    value?
        .as_object()?
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn required_str(args: &Value, key: &str) -> SootieResult<String> {
    str_arg(args, key).ok_or_else(|| SootieError::InvalidArguments(format!("{key} is required")))
}

fn mouse_button_arg(args: &Value) -> SootieResult<String> {
    let button = str_arg(args, "button")
        .map(|button| button.trim().to_lowercase())
        .filter(|button| !button.is_empty())
        .unwrap_or_else(|| "left".to_string());
    match button.as_str() {
        "left" | "right" | "middle" => Ok(button),
        other => Err(SootieError::InvalidArguments(format!(
            "unsupported mouse button '{other}'"
        ))),
    }
}

fn scroll_direction_arg(args: &Value) -> SootieResult<String> {
    let direction = required_str(args, "direction")?.trim().to_lowercase();
    match direction.as_str() {
        "up" | "down" | "left" | "right" => Ok(direction),
        other => Err(SootieError::InvalidArguments(format!(
            "unsupported scroll direction '{other}'"
        ))),
    }
}

fn f64_arg(args: &Value, key: &str) -> Option<f64> {
    args.get(key).and_then(f64_value_arg)
}

fn seconds_arg_result(
    args: &Value,
    seconds_key: &str,
    millis_key: &str,
) -> SootieResult<Option<f64>> {
    if args.get(seconds_key).is_some() {
        return f64_arg(args, seconds_key)
            .map(Some)
            .ok_or_else(|| invalid_duration(seconds_key, millis_key));
    }
    if args.get(millis_key).is_some() {
        return f64_arg(args, millis_key)
            .map(|value| Some(value / 1000.0))
            .ok_or_else(|| invalid_duration(seconds_key, millis_key));
    }
    Ok(None)
}

fn non_negative_seconds_arg(
    args: &Value,
    seconds_key: &str,
    millis_key: &str,
    default: f64,
) -> SootieResult<f64> {
    let value = seconds_arg_result(args, seconds_key, millis_key)?.unwrap_or(default);
    if value.is_finite() && value >= 0.0 {
        Ok(value)
    } else {
        Err(invalid_duration(seconds_key, millis_key))
    }
}

fn invalid_duration(seconds_key: &str, millis_key: &str) -> SootieError {
    SootieError::InvalidArguments(format!(
        "{seconds_key}/{millis_key} must be a non-negative finite duration"
    ))
}

fn required_f64(args: &Value, key: &str) -> SootieResult<f64> {
    finite_f64_arg(args, key)?
        .ok_or_else(|| SootieError::InvalidArguments(format!("{key} is required")))
}

fn finite_f64_arg(args: &Value, key: &str) -> SootieResult<Option<f64>> {
    let Some(_) = args.get(key) else {
        return Ok(None);
    };
    let value = f64_arg(args, key)
        .ok_or_else(|| SootieError::InvalidArguments(format!("{key} must be a finite number")))?;
    if value.is_finite() {
        Ok(Some(value))
    } else {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a finite number"
        )))
    }
}

fn u32_arg(args: &Value, key: &str) -> Option<u32> {
    let value = args.get(key)?;
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).ok();
    }
    if let Some(value) = value.as_i64() {
        return u32::try_from(value).ok();
    }
    let value = f64_value_arg(value)?;
    (value.is_finite() && value >= 0.0 && value <= u32::MAX as f64 && value.fract() == 0.0)
        .then_some(value as u32)
}

fn positive_u32_arg(args: &Value, key: &str, default: u32) -> SootieResult<u32> {
    if args.get(key).is_none() {
        return Ok(default);
    }
    let value = u32_arg(args, key).ok_or_else(|| {
        SootieError::InvalidArguments(format!("{key} must be a positive integer"))
    })?;
    if value > 0 {
        Ok(value)
    } else {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a positive integer"
        )))
    }
}

fn i32_arg(args: &Value, key: &str) -> Option<i32> {
    let value = args.get(key)?;
    if let Some(value) = value.as_i64() {
        return i32::try_from(value).ok();
    }
    let value = f64_value_arg(value)?;
    (value.is_finite()
        && value >= i32::MIN as f64
        && value <= i32::MAX as f64
        && value.fract() == 0.0)
        .then_some(value as i32)
}

fn positive_i32_arg(args: &Value, key: &str, default: i32) -> SootieResult<i32> {
    if args.get(key).is_none() {
        return Ok(default);
    }
    let value = i32_arg(args, key).ok_or_else(|| {
        SootieError::InvalidArguments(format!("{key} must be a positive integer"))
    })?;
    if value > 0 {
        Ok(value)
    } else {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a positive integer"
        )))
    }
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    match args.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn nested_bool_arg(args: &Value, object_key: &str, key: &str) -> Option<bool> {
    match args.get(object_key)?.as_object()?.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn f64_value_arg(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.trim().parse::<f64>().ok())
}

fn string_array_arg(args: &Value, key: &str) -> Vec<String> {
    let Some(value) = args.get(key) else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Value::String(text) => split_string_list(text),
        _ => Vec::new(),
    }
}

fn string_array_required(args: &Value, key: &str) -> SootieResult<Vec<String>> {
    let items = string_array_arg(args, key);
    if items.is_empty() {
        Err(SootieError::InvalidArguments(format!(
            "{key} must be a non-empty string array or comma-separated string"
        )))
    } else {
        Ok(items)
    }
}

fn split_string_list(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::DesktopBackend;
    use crate::tools::TOOL_NAMES;
    use crate::types::{
        ActionResult, AppInfo, ContextSnapshot, ElementInfo, Screenshot, WindowInfo,
    };
    use base64::Engine;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;

    struct FakeBackend;

    impl DesktopBackend for FakeBackend {
        fn platform(&self) -> &'static str {
            "fake"
        }
        fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot> {
            if matches!(app, Some("MissingApp" | "UrlOnlyBrowser")) {
                return Ok(ContextSnapshot {
                    app: None,
                    app_id: None,
                    platform_app_id: None,
                    bundle_id: None,
                    pid: None,
                    window: None,
                    url: None,
                    focused_element: None,
                    interactive_elements: vec![],
                });
            }
            let mut interactive_elements = vec![
                ElementInfo {
                    id: Some("fake-button".into()),
                    role: "AXButton".into(),
                    title: Some("Submit".into()),
                    name: Some("Submit".into()),
                    text: None,
                    bounds: Some(Bounds {
                        x: 10.0,
                        y: 20.0,
                        width: 100.0,
                        height: 40.0,
                    }),
                    actions: vec!["click".into()],
                    editable: Some(false),
                    enabled: Some(true),
                },
                ElementInfo {
                    id: Some("fake-field".into()),
                    role: "AXTextField".into(),
                    title: Some("Name".into()),
                    name: Some("Name".into()),
                    text: None,
                    bounds: Some(Bounds {
                        x: 20.0,
                        y: 80.0,
                        width: 200.0,
                        height: 30.0,
                    }),
                    actions: vec!["setValue".into()],
                    editable: Some(true),
                    enabled: Some(true),
                },
            ];
            if app == Some("Safari") {
                interactive_elements.push(ElementInfo {
                    id: Some("ellipse-tool".into()),
                    role: "AXGroup".into(),
                    title: Some("Ellipse — O or 4".into()),
                    name: Some("Ellipse — O or 4".into()),
                    text: None,
                    bounds: Some(Bounds {
                        x: 1400.0,
                        y: 100.0,
                        width: 36.0,
                        height: 36.0,
                    }),
                    actions: vec!["click".into()],
                    editable: Some(false),
                    enabled: Some(true),
                });
            }
            Ok(ContextSnapshot {
                app: Some("Fake".into()),
                app_id: Some("Fake".into()),
                platform_app_id: Some("fake".into()),
                bundle_id: Some("com.example.fake".into()),
                pid: Some(42),
                window: Some("Main".into()),
                url: Some("https://example.com/current".into()),
                focused_element: None,
                interactive_elements,
            })
        }
        fn browser_url(&self, app: Option<&str>) -> SootieResult<Option<String>> {
            if matches!(app, Some("Safari" | "UrlOnlyBrowser")) {
                return Ok(Some("https://excalidraw.com/".into()));
            }
            Ok(Some("https://example.com/current".into()))
        }
        fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
            if matches!(app, Some("MissingApp" | "UrlOnlyBrowser")) {
                return Ok(vec![]);
            }
            Ok(vec![AppInfo {
                name: "Fake".into(),
                app_id: Some("Fake".into()),
                platform_app_id: Some("fake".into()),
                pid: Some(42),
                bundle_id: None,
                is_frontmost: true,
                windows: vec![WindowInfo {
                    id: Some("win-1".into()),
                    title: "Main".into(),
                    bounds: Some(Bounds {
                        x: 1.0,
                        y: 2.0,
                        width: 800.0,
                        height: 600.0,
                    }),
                    focused: true,
                }],
            }])
        }
        fn find(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
            if query.query.as_deref() == Some("Ellipse") {
                return Ok(vec![]);
            }
            if query.query.as_deref() == Some("Many") {
                return Ok(vec![
                    ElementInfo {
                        id: Some("fake-button".into()),
                        role: "button".into(),
                        title: Some("Submit".into()),
                        name: Some("Submit".into()),
                        text: None,
                        bounds: Some(Bounds {
                            x: 10.0,
                            y: 20.0,
                            width: 100.0,
                            height: 40.0,
                        }),
                        actions: vec!["click".into()],
                        editable: Some(false),
                        enabled: Some(true),
                    },
                    ElementInfo {
                        id: Some("fake-link".into()),
                        role: "link".into(),
                        title: Some("Learn more".into()),
                        name: Some("Learn more".into()),
                        text: None,
                        bounds: Some(Bounds {
                            x: 120.0,
                            y: 20.0,
                            width: 100.0,
                            height: 40.0,
                        }),
                        actions: vec!["click".into()],
                        editable: Some(false),
                        enabled: Some(true),
                    },
                ]);
            }
            Ok(vec![ElementInfo {
                id: Some("fake-button".into()),
                role: "button".into(),
                title: Some("Submit".into()),
                name: Some("Submit".into()),
                text: None,
                bounds: Some(Bounds {
                    x: 10.0,
                    y: 20.0,
                    width: 100.0,
                    height: 40.0,
                }),
                actions: vec!["click".into()],
                editable: Some(false),
                enabled: Some(true),
            }])
        }
        fn read(
            &self,
            _app: Option<&str>,
            _query: Option<&str>,
            _depth: Option<u32>,
        ) -> SootieResult<String> {
            Ok("text".into())
        }
        fn inspect(&self, _query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }
        fn element_at(&self, _x: f64, _y: f64) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }
        fn screenshot(
            &self,
            _app: Option<&str>,
            _window: Option<&str>,
            full_resolution: bool,
        ) -> SootieResult<Screenshot> {
            Ok(Screenshot {
                mime_type: "image/png".into(),
                data_base64: "abc123".into(),
                width: Some(if full_resolution { 1600 } else { 800 }),
                height: Some(if full_resolution { 1200 } else { 600 }),
                window_title: Some("Main".into()),
                window_frame: Some(Bounds {
                    x: 1.0,
                    y: 2.0,
                    width: 800.0,
                    height: 600.0,
                }),
            })
        }
        fn click(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            _query: &FindQuery,
            button: &str,
            count: u32,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-click".into(),
                details: json!({ "x": x, "y": y, "button": button, "count": count }),
            })
        }
        fn hover(
            &self,
            _x: Option<f64>,
            _y: Option<f64>,
            _query: &FindQuery,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake".into(),
                details: json!({}),
            })
        }
        fn long_press(
            &self,
            _x: Option<f64>,
            _y: Option<f64>,
            _query: &FindQuery,
            duration_secs: f64,
            button: &str,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-long-press".into(),
                details: json!({ "duration": duration_secs, "button": button }),
            })
        }
        fn drag(
            &self,
            from: Option<(f64, f64)>,
            to: (f64, f64),
            _query: &FindQuery,
            duration_secs: f64,
            hold_duration_secs: f64,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-drag".into(),
                details: json!({
                    "from": from,
                    "to": to,
                    "duration": duration_secs,
                    "hold_duration": hold_duration_secs
                }),
            })
        }
        fn type_text(
            &self,
            text: &str,
            _target: &FindQuery,
            clear: bool,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-type".into(),
                details: json!({ "text": text, "clear": clear }),
            })
        }
        fn press(
            &self,
            _key: &str,
            _modifiers: &[String],
            _app: Option<&str>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake".into(),
                details: json!({}),
            })
        }
        fn hotkey(&self, _keys: &[String], _app: Option<&str>) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake".into(),
                details: json!({}),
            })
        }
        fn clipboard_text(&self) -> SootieResult<String> {
            Ok("<svg/>".to_string())
        }
        fn scroll(
            &self,
            direction: &str,
            amount: i32,
            app: Option<&str>,
            at: Option<(f64, f64)>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-scroll".into(),
                details: json!({ "direction": direction, "amount": amount, "app": app, "at": at }),
            })
        }
        fn focus(
            &self,
            app: &str,
            platform_app_id: Option<&str>,
            _window: Option<&str>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake".into(),
                details: json!({ "app": app, "platform_app_id": platform_app_id }),
            })
        }
        fn window(
            &self,
            command: WindowCommand,
            app: &str,
            platform_app_id: Option<&str>,
            window: Option<&str>,
            bounds: Option<Bounds>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-window".into(),
                details: json!({ "command": command, "app": app, "platform_app_id": platform_app_id, "window": window, "bounds": bounds }),
            })
        }
    }

    struct LockedFakeBackend;

    impl DesktopBackend for LockedFakeBackend {
        fn platform(&self) -> &'static str {
            FakeBackend.platform()
        }
        fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
            FakeBackend.diagnostics()
        }
        fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot> {
            FakeBackend.context(app)
        }
        fn browser_url(&self, app: Option<&str>) -> SootieResult<Option<String>> {
            FakeBackend.browser_url(app)
        }
        fn screen_locked(&self) -> SootieResult<Option<bool>> {
            Ok(Some(true))
        }
        fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
            FakeBackend.state(app)
        }
        fn find(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
            FakeBackend.find(query)
        }
        fn read(
            &self,
            app: Option<&str>,
            query: Option<&str>,
            depth: Option<u32>,
        ) -> SootieResult<String> {
            FakeBackend.read(app, query, depth)
        }
        fn inspect(&self, query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
            FakeBackend.inspect(query)
        }
        fn element_at(&self, x: f64, y: f64) -> SootieResult<Option<ElementInfo>> {
            FakeBackend.element_at(x, y)
        }
        fn screenshot(
            &self,
            app: Option<&str>,
            window: Option<&str>,
            full_resolution: bool,
        ) -> SootieResult<Screenshot> {
            FakeBackend.screenshot(app, window, full_resolution)
        }
        fn click(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            query: &FindQuery,
            button: &str,
            count: u32,
        ) -> SootieResult<ActionResult> {
            FakeBackend.click(x, y, query, button, count)
        }
        fn hover(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            query: &FindQuery,
        ) -> SootieResult<ActionResult> {
            FakeBackend.hover(x, y, query)
        }
        fn long_press(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            query: &FindQuery,
            duration_secs: f64,
            button: &str,
        ) -> SootieResult<ActionResult> {
            FakeBackend.long_press(x, y, query, duration_secs, button)
        }
        fn drag(
            &self,
            from: Option<(f64, f64)>,
            to: (f64, f64),
            query: &FindQuery,
            duration_secs: f64,
            hold_duration_secs: f64,
        ) -> SootieResult<ActionResult> {
            FakeBackend.drag(from, to, query, duration_secs, hold_duration_secs)
        }
        fn type_text(
            &self,
            text: &str,
            target: &FindQuery,
            clear: bool,
        ) -> SootieResult<ActionResult> {
            FakeBackend.type_text(text, target, clear)
        }
        fn press(
            &self,
            key: &str,
            modifiers: &[String],
            app: Option<&str>,
        ) -> SootieResult<ActionResult> {
            FakeBackend.press(key, modifiers, app)
        }
        fn hotkey(&self, keys: &[String], app: Option<&str>) -> SootieResult<ActionResult> {
            FakeBackend.hotkey(keys, app)
        }
        fn scroll(
            &self,
            direction: &str,
            amount: i32,
            app: Option<&str>,
            at: Option<(f64, f64)>,
        ) -> SootieResult<ActionResult> {
            FakeBackend.scroll(direction, amount, app, at)
        }
        fn focus(
            &self,
            app: &str,
            platform_app_id: Option<&str>,
            window: Option<&str>,
        ) -> SootieResult<ActionResult> {
            FakeBackend.focus(app, platform_app_id, window)
        }
        fn window(
            &self,
            command: WindowCommand,
            app: &str,
            platform_app_id: Option<&str>,
            window: Option<&str>,
            bounds: Option<Bounds>,
        ) -> SootieResult<ActionResult> {
            FakeBackend.window(command, app, platform_app_id, window, bounds)
        }
    }

    struct VisionOnlyBackend;

    impl DesktopBackend for VisionOnlyBackend {
        fn platform(&self) -> &'static str {
            "fake"
        }

        fn context(&self, _app: Option<&str>) -> SootieResult<ContextSnapshot> {
            Ok(ContextSnapshot {
                app: Some("Fake".into()),
                app_id: Some("Fake".into()),
                platform_app_id: Some("fake".into()),
                bundle_id: None,
                pid: Some(42),
                window: Some("Main".into()),
                url: None,
                focused_element: None,
                interactive_elements: vec![],
            })
        }

        fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
            Ok(vec![AppInfo {
                name: app.unwrap_or("Fake").into(),
                app_id: Some(app.unwrap_or("Fake").into()),
                platform_app_id: Some("fake".into()),
                pid: Some(42),
                bundle_id: None,
                is_frontmost: true,
                windows: vec![WindowInfo {
                    id: Some("win-1".into()),
                    title: "Main".into(),
                    bounds: Some(Bounds {
                        x: 10.0,
                        y: 20.0,
                        width: 400.0,
                        height: 300.0,
                    }),
                    focused: true,
                }],
            }])
        }

        fn find(&self, _query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
            Ok(vec![])
        }

        fn read(
            &self,
            _app: Option<&str>,
            _query: Option<&str>,
            _depth: Option<u32>,
        ) -> SootieResult<String> {
            Ok(String::new())
        }

        fn inspect(&self, _query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }

        fn element_at(&self, _x: f64, _y: f64) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }

        fn screenshot(
            &self,
            _app: Option<&str>,
            _window: Option<&str>,
            _full_resolution: bool,
        ) -> SootieResult<Screenshot> {
            Ok(Screenshot {
                mime_type: "image/png".into(),
                data_base64: "abc123".into(),
                width: Some(400),
                height: Some(300),
                window_title: Some("Main".into()),
                window_frame: Some(Bounds {
                    x: 10.0,
                    y: 20.0,
                    width: 400.0,
                    height: 300.0,
                }),
            })
        }

        fn click(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            _query: &FindQuery,
            button: &str,
            count: u32,
        ) -> SootieResult<ActionResult> {
            match (x, y) {
                (Some(x), Some(y)) => Ok(ActionResult {
                    method: "fake-click".into(),
                    details: json!({ "x": x, "y": y, "button": button, "count": count }),
                }),
                _ => Err(SootieError::NotFound("target not found".into())),
            }
        }

        fn hover(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            _query: &FindQuery,
        ) -> SootieResult<ActionResult> {
            match (x, y) {
                (Some(x), Some(y)) => Ok(ActionResult {
                    method: "fake-hover".into(),
                    details: json!({ "x": x, "y": y }),
                }),
                _ => Err(SootieError::NotFound("target not found".into())),
            }
        }

        fn long_press(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            _query: &FindQuery,
            duration_secs: f64,
            button: &str,
        ) -> SootieResult<ActionResult> {
            match (x, y) {
                (Some(x), Some(y)) => Ok(ActionResult {
                    method: "fake-long-press".into(),
                    details: json!({
                        "x": x,
                        "y": y,
                        "duration": duration_secs,
                        "button": button
                    }),
                }),
                _ => Err(SootieError::NotFound("target not found".into())),
            }
        }

        fn drag(
            &self,
            _from: Option<(f64, f64)>,
            _to: (f64, f64),
            _query: &FindQuery,
            _duration_secs: f64,
            _hold_duration_secs: f64,
        ) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("drag".into()))
        }

        fn type_text(
            &self,
            _text: &str,
            _target: &FindQuery,
            _clear: bool,
        ) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("type".into()))
        }

        fn press(
            &self,
            _key: &str,
            _modifiers: &[String],
            _app: Option<&str>,
        ) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("press".into()))
        }

        fn hotkey(&self, _keys: &[String], _app: Option<&str>) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("hotkey".into()))
        }

        fn scroll(
            &self,
            _direction: &str,
            _amount: i32,
            _app: Option<&str>,
            _at: Option<(f64, f64)>,
        ) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("scroll".into()))
        }

        fn focus(
            &self,
            app: &str,
            platform_app_id: Option<&str>,
            window: Option<&str>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "fake-focus".into(),
                details: json!({
                    "app": app,
                    "platform_app_id": platform_app_id,
                    "window": window
                }),
            })
        }

        fn window(
            &self,
            _command: WindowCommand,
            _app: &str,
            _platform_app_id: Option<&str>,
            _window: Option<&str>,
            _bounds: Option<Bounds>,
        ) -> SootieResult<ActionResult> {
            Err(SootieError::Unsupported("window".into()))
        }
    }

    struct RecordingBackend {
        events: Arc<Mutex<Vec<String>>>,
        fail_focus: bool,
        record_context: bool,
        record_screenshot: bool,
    }

    impl RecordingBackend {
        fn record(&self, event: impl Into<String>) {
            self.events.lock().unwrap().push(event.into());
        }
    }

    impl DesktopBackend for RecordingBackend {
        fn platform(&self) -> &'static str {
            "fake"
        }

        fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot> {
            if self.record_context {
                self.record(format!("context:{}", app.unwrap_or("<none>")));
            }
            Ok(ContextSnapshot {
                app: Some(app.unwrap_or("Fake").into()),
                app_id: Some(app.unwrap_or("Fake").into()),
                platform_app_id: Some("fake".into()),
                bundle_id: None,
                pid: Some(42),
                window: Some("Main".into()),
                url: None,
                focused_element: None,
                interactive_elements: vec![],
            })
        }

        fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
            Ok(vec![AppInfo {
                name: app.unwrap_or("Fake").into(),
                app_id: Some(app.unwrap_or("Fake").into()),
                platform_app_id: Some("fake".into()),
                pid: Some(42),
                bundle_id: None,
                is_frontmost: true,
                windows: vec![],
            }])
        }

        fn find(&self, _query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
            Ok(vec![])
        }

        fn read(
            &self,
            _app: Option<&str>,
            _query: Option<&str>,
            _depth: Option<u32>,
        ) -> SootieResult<String> {
            Ok(String::new())
        }

        fn inspect(&self, _query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }

        fn element_at(&self, _x: f64, _y: f64) -> SootieResult<Option<ElementInfo>> {
            Ok(None)
        }

        fn screenshot(
            &self,
            app: Option<&str>,
            window: Option<&str>,
            full_resolution: bool,
        ) -> SootieResult<Screenshot> {
            if self.record_screenshot {
                self.record(format!(
                    "screenshot:{}:{}:{}",
                    app.unwrap_or("<none>"),
                    window.unwrap_or("<none>"),
                    full_resolution
                ));
            }
            Ok(Screenshot {
                mime_type: "image/png".into(),
                data_base64: "abc123".into(),
                width: Some(100),
                height: Some(100),
                window_title: Some("Main".into()),
                window_frame: None,
            })
        }

        fn click(
            &self,
            x: Option<f64>,
            y: Option<f64>,
            _query: &FindQuery,
            button: &str,
            count: u32,
        ) -> SootieResult<ActionResult> {
            self.record("click");
            Ok(ActionResult {
                method: "recording-click".into(),
                details: json!({ "x": x, "y": y, "button": button, "count": count }),
            })
        }

        fn hover(
            &self,
            _x: Option<f64>,
            _y: Option<f64>,
            _query: &FindQuery,
        ) -> SootieResult<ActionResult> {
            self.record("hover");
            Ok(ActionResult {
                method: "recording-hover".into(),
                details: json!({}),
            })
        }

        fn long_press(
            &self,
            _x: Option<f64>,
            _y: Option<f64>,
            _query: &FindQuery,
            duration_secs: f64,
            button: &str,
        ) -> SootieResult<ActionResult> {
            self.record("long_press");
            Ok(ActionResult {
                method: "recording-long-press".into(),
                details: json!({ "duration": duration_secs, "button": button }),
            })
        }

        fn drag(
            &self,
            _from: Option<(f64, f64)>,
            _to: (f64, f64),
            _query: &FindQuery,
            duration_secs: f64,
            hold_duration_secs: f64,
        ) -> SootieResult<ActionResult> {
            self.record("drag");
            Ok(ActionResult {
                method: "recording-drag".into(),
                details: json!({ "duration": duration_secs, "hold_duration": hold_duration_secs }),
            })
        }

        fn type_text(
            &self,
            _text: &str,
            _target: &FindQuery,
            _clear: bool,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "recording-type".into(),
                details: json!({}),
            })
        }

        fn set_clipboard_text(&self, text: &str) -> SootieResult<ActionResult> {
            self.record(format!("clipboard:{text}"));
            Ok(ActionResult {
                method: "recording-clipboard".into(),
                details: json!({ "bytes": text.len() }),
            })
        }

        fn clipboard_text(&self) -> SootieResult<String> {
            Ok("<svg/>".to_string())
        }

        fn press(
            &self,
            _key: &str,
            _modifiers: &[String],
            _app: Option<&str>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "recording-press".into(),
                details: json!({}),
            })
        }

        fn hotkey(&self, _keys: &[String], _app: Option<&str>) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "recording-hotkey".into(),
                details: json!({}),
            })
        }

        fn scroll(
            &self,
            _direction: &str,
            _amount: i32,
            _app: Option<&str>,
            _at: Option<(f64, f64)>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "recording-scroll".into(),
                details: json!({}),
            })
        }

        fn focus(
            &self,
            app: &str,
            _platform_app_id: Option<&str>,
            _window: Option<&str>,
        ) -> SootieResult<ActionResult> {
            self.record(format!("focus:{app}"));
            if self.fail_focus {
                return Err(SootieError::Platform("focus failed".into()));
            }
            Ok(ActionResult {
                method: "recording-focus".into(),
                details: json!({ "app": app }),
            })
        }

        fn window(
            &self,
            _command: WindowCommand,
            _app: &str,
            _platform_app_id: Option<&str>,
            _window: Option<&str>,
            _bounds: Option<Bounds>,
        ) -> SootieResult<ActionResult> {
            Ok(ActionResult {
                method: "recording-window".into(),
                details: json!({}),
            })
        }
    }

    fn spawn_vision_ground_server(response: Value) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_test_http_request(&mut stream);
            let body = serde_json::to_string(&response).unwrap();
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(http_response.as_bytes()).unwrap();
            request
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    fn read_test_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if test_http_body_complete(&bytes) {
                break;
            }
        }
        String::from_utf8_lossy(&bytes).to_string()
    }

    fn test_http_body_complete(bytes: &[u8]) -> bool {
        let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&bytes[..header_end]);
        let content_length = headers.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        });
        match content_length {
            Some(length) => bytes.len() >= header_end + 4 + length,
            None => true,
        }
    }

    fn call_tool(name: &str, arguments: Value) -> Value {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({ "name": name, "arguments": arguments }),
        });
        response.result.unwrap()
    }

    fn assert_tool_arg_rejected(name: &str, arguments: Value, rejected_args: &[&str]) {
        let result = call_tool(name, arguments);
        assert_eq!(result["isError"], true);
        let error = result["structuredContent"]["error"].as_str().unwrap();
        assert!(
            error.contains(&format!("{name} does not accept argument(s):")),
            "{error}"
        );
        for rejected_arg in rejected_args {
            assert!(error.contains(rejected_arg), "{error}");
        }
    }

    #[test]
    fn initializes_and_lists_tools() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        });
        assert!(response.error.is_none());
        assert_eq!(
            response.result.unwrap()["tools"].as_array().unwrap().len(),
            TOOL_NAMES.len()
        );
    }

    #[test]
    fn tools_list_serializes_mcp_annotations() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/list".into(),
            params: json!({}),
        });
        assert!(response.error.is_none());

        let result = response.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            let annotations = &tool["annotations"];
            assert!(
                annotations["readOnlyHint"].is_boolean(),
                "{name} missing readOnlyHint"
            );
            assert!(
                annotations["destructiveHint"].is_boolean(),
                "{name} missing destructiveHint"
            );
            assert!(
                annotations["idempotentHint"].is_boolean(),
                "{name} missing idempotentHint"
            );
            assert!(
                annotations["openWorldHint"].is_boolean(),
                "{name} missing openWorldHint"
            );
        }

        let status = tools
            .iter()
            .find(|tool| tool["name"] == "sootie_learn_status")
            .unwrap();
        assert_eq!(status["annotations"]["readOnlyHint"], true);
        assert_eq!(status["annotations"]["destructiveHint"], false);
        assert_eq!(status["annotations"]["idempotentHint"], true);
        assert_eq!(status["annotations"]["openWorldHint"], false);

        let click = tools
            .iter()
            .find(|tool| tool["name"] == "sootie_click")
            .unwrap();
        assert_eq!(click["annotations"]["readOnlyHint"], false);
        assert_eq!(click["annotations"]["destructiveHint"], true);
        assert_eq!(click["annotations"]["idempotentHint"], false);
        assert_eq!(click["annotations"]["openWorldHint"], true);
    }

    #[test]
    fn stdio_accepts_content_length_framed_requests() {
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let input = format!("Content-Length: {}\r\n\r\n{}", request.len(), request);
        let mut output = Vec::new();
        let mut server = McpServer::new(Box::new(FakeBackend));

        server
            .serve_reader_writer(std::io::Cursor::new(input), &mut output)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("Content-Length: "));
        let (_, body) = output.split_once("\r\n\r\n").unwrap();
        let response: Value = serde_json::from_str(body).unwrap();
        assert_eq!(response["id"], json!(1));
        assert_eq!(
            response["result"]["tools"].as_array().unwrap().len(),
            TOOL_NAMES.len()
        );
    }

    #[test]
    fn stdio_accepts_content_length_after_other_headers() {
        let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let input = format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            request.len(),
            request
        );
        let mut output = Vec::new();
        let mut server = McpServer::new(Box::new(FakeBackend));

        server
            .serve_reader_writer(std::io::Cursor::new(input), &mut output)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("Content-Length: "));
        let (_, body) = output.split_once("\r\n\r\n").unwrap();
        let response: Value = serde_json::from_str(body).unwrap();
        assert_eq!(response["id"], json!(1));
        assert_eq!(
            response["result"]["tools"].as_array().unwrap().len(),
            TOOL_NAMES.len()
        );
    }

    #[test]
    fn stdio_keeps_line_json_compatibility() {
        let input = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
        let mut output = Vec::new();
        let mut server = McpServer::new(Box::new(FakeBackend));

        server
            .serve_reader_writer(std::io::Cursor::new(format!("{input}\n")), &mut output)
            .unwrap();

        let output = String::from_utf8(output).unwrap();
        assert!(!output.starts_with("Content-Length: "));
        let response: Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(response["id"], json!(1));
        assert_eq!(
            response["result"]["tools"].as_array().unwrap().len(),
            TOOL_NAMES.len()
        );
    }

    #[test]
    fn advertised_tools_all_dispatch_to_tool_reports() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        store
            .save(
                &parse_recipe(&json!({
                    "schema_version": 1,
                    "name": "empty",
                    "steps": []
                }))
                .unwrap(),
            )
            .unwrap();
        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);

        for name in TOOL_NAMES {
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(name)),
                method: "tools/call".into(),
                params: json!({
                    "name": name,
                    "arguments": smoke_arguments_for_tool(name)
                }),
            });
            assert!(
                response.error.is_none(),
                "{name} returned JSON-RPC error: {:?}",
                response.error
            );
            let result = response.result.unwrap();
            assert_eq!(
                result["structuredContent"]["report"]["tool"], *name,
                "{name} did not return a standard tool report"
            );
            let error = result["structuredContent"]["error"].as_str().unwrap_or("");
            assert!(
                !error.contains("unknown tool"),
                "{name} did not dispatch: {error}"
            );
        }
    }

    fn smoke_arguments_for_tool(name: &str) -> Value {
        match name {
            "sootie_context"
            | "sootie_state"
            | "sootie_find"
            | "sootie_read"
            | "sootie_screenshot"
            | "sootie_recipes"
            | "sootie_parse_screen"
            | "sootie_learn_stop"
            | "sootie_learn_status" => json!({}),
            "sootie_inspect" => json!({ "query": "Submit" }),
            "sootie_element_at" => json!({ "x": 1.0, "y": 2.0 }),
            "sootie_focus" => json!({ "app": "Fake" }),
            "sootie_click" | "sootie_hover" | "sootie_long_press" => {
                json!({ "x": 1.0, "y": 2.0 })
            }
            "sootie_drag" => json!({
                "from_x": 1.0,
                "from_y": 2.0,
                "to_x": 3.0,
                "to_y": 4.0
            }),
            "sootie_type" => json!({ "text": "hello" }),
            "sootie_press" => json!({ "key": "Enter" }),
            "sootie_hotkey" => json!({ "keys": ["cmd", "l"] }),
            "sootie_scroll" => json!({ "direction": "down" }),
            "sootie_window" => json!({ "action": "list", "app": "Fake" }),
            "sootie_wait" => json!({
                "condition": "titleContains",
                "value": "Main",
                "timeout": 0.1,
                "interval": 0.05
            }),
            "sootie_run" => json!({ "recipe": "empty" }),
            "sootie_recipe_show" => json!({ "name": "empty" }),
            "sootie_recipe_save" => json!({
                "recipe_json": "{\"schema_version\":1,\"name\":\"saved\",\"steps\":[]}"
            }),
            "sootie_recipe_delete" => json!({ "name": "saved" }),
            "sootie_ground" => json!({ "description": "Submit" }),
            "sootie_annotate" => json!({ "max_labels": 3 }),
            "sootie_browser_connect"
            | "sootie_browser_pages"
            | "sootie_browser_observe"
            | "sootie_browser_find"
            | "sootie_browser_extract"
            | "sootie_browser_screenshot"
            | "sootie_browser_back"
            | "sootie_browser_forward"
            | "sootie_browser_reload"
            | "sootie_browser_close_page" => json!({ "port": 9 }),
            "sootie_browser_select_page" => json!({ "port": 9, "page_id": "missing" }),
            "sootie_browser_open" => json!({ "port": 9, "url": "https://example.com" }),
            "sootie_browser_click" => json!({ "port": 9, "query": "Submit" }),
            "sootie_browser_type" => json!({ "port": 9, "text": "hello", "into": "Search" }),
            "sootie_browser_press" => json!({ "port": 9, "key": "Enter" }),
            "sootie_browser_scroll" => json!({ "port": 9, "direction": "down" }),
            "sootie_browser_wait" => {
                json!({ "port": 9, "condition": "urlContains", "value": "example" })
            }
            "sootie_browser_network" => json!({ "port": 9 }),
            "sootie_browser_console" => json!({ "port": 9 }),
            "sootie_browser_storage" => {
                json!({ "port": 9, "area": "localStorage", "action": "list" })
            }
            "sootie_browser_cookies" => json!({ "port": 9, "action": "list" }),
            "sootie_browser_downloads" => json!({ "port": 9, "action": "deny", "unsafe": true }),
            "sootie_browser_upload" => {
                json!({ "port": 9, "selector": "input[type=file]", "file_paths": ["/tmp/missing"], "unsafe": true })
            }
            "sootie_browser_pdf" => json!({ "port": 9 }),
            "sootie_cdp_send" => {
                json!({ "port": 9, "method": "Browser.getVersion", "unsafe": true })
            }
            "sootie_cdp_subscribe" => json!({ "port": 9, "domain": "Log", "unsafe": true }),
            "sootie_learn_start" => json!({ "task_description": "smoke" }),
            other => panic!("missing smoke arguments for {other}"),
        }
    }

    #[test]
    fn formats_tool_errors_as_mcp_result() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_press","arguments":{}}),
        });
        assert!(response.error.is_none());
        assert_eq!(response.result.unwrap()["isError"], true);
    }

    #[test]
    fn browser_connect_reports_missing_cdp_as_tool_error() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_browser_connect",
                "arguments": { "port": 9 }
            }),
        });
        assert!(response.error.is_none());
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["structuredContent"]["report"]["tool"],
            "sootie_browser_connect"
        );
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("BROWSER_NOT_CONNECTED"));
    }

    #[test]
    fn format_tool_result_suggests_recovery_for_foreground_failure() {
        let result = format_tool_result(
            "sootie_focus",
            json!({"app":"TextEdit"}),
            Err(SootieError::Platform(
                "macOS did not make 'TextEdit' frontmost after activation".to_string(),
            )),
            10,
        );
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["suggestion"]
            .as_str()
            .unwrap()
            .contains("Accessibility"));
    }

    #[test]
    fn macos_apple_event_errors_suggest_permission_recovery() {
        for message in [
            "Apple event error -1743: Unknown error",
            "execution error: An error of type -10827 has occurred. (-10827)",
        ] {
            let result = format_tool_result(
                "sootie_focus",
                json!({"app":"TextEdit"}),
                Err(SootieError::Platform(message.to_string())),
                10,
            );
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["suggestion"]
                .as_str()
                .unwrap()
                .contains("Automation"));
        }
    }

    #[test]
    fn macos_window_lookup_errors_suggest_accessibility_recovery() {
        let result = format_tool_result(
            "sootie_focus",
            json!({"app":"Calculator","window":"Calculator"}),
            Err(SootieError::Platform(
                "osascript failed: execution error: window not found: Calculator (-2700)"
                    .to_string(),
            )),
            10,
        );
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["suggestion"]
            .as_str()
            .unwrap()
            .contains("sootie_state"));
    }

    #[test]
    fn locked_ui_action_errors_suggest_unlocking() {
        let result = format_tool_result(
            "sootie_drag",
            json!({"app":"Safari"}),
            Err(SootieError::Platform(
                "macOS screen is locked; drag would affect the lock screen instead of the target app"
                    .to_string(),
            )),
            10,
        );
        assert_eq!(result["isError"], true);
        let suggestion = result["structuredContent"]["suggestion"].as_str().unwrap();
        assert!(suggestion.contains("Unlock the Mac"));
        assert!(suggestion.contains("target app"));
    }

    #[test]
    fn element_at_miss_returns_recovery_suggestion() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_element_at","arguments":{"x":100,"y":100}}),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["suggestion"]
            .as_str()
            .unwrap()
            .contains("sootie_parse_screen"));
    }

    #[test]
    fn context_tool_suggests_recovery_for_empty_accessibility_tree() {
        let result = context_tool_result(
            ContextSnapshot {
                app: Some("Codex".into()),
                app_id: Some("Codex".into()),
                platform_app_id: Some("com.openai.codex".into()),
                bundle_id: Some("com.openai.codex".into()),
                pid: Some(42),
                window: None,
                url: None,
                focused_element: None,
                interactive_elements: Vec::new(),
            },
            "macos",
            None,
            &[],
        );

        assert!(result.success);
        assert!(result
            .suggestion
            .unwrap()
            .contains("screen capture is separate from Accessibility"));
    }

    #[test]
    fn context_tool_uses_requested_app_in_empty_recovery_suggestion() {
        let result = context_tool_result(
            ContextSnapshot {
                app: None,
                app_id: None,
                platform_app_id: None,
                bundle_id: None,
                pid: None,
                window: None,
                url: None,
                focused_element: None,
                interactive_elements: Vec::new(),
            },
            "macos",
            Some("Calculator"),
            &[],
        );

        assert!(result.success);
        assert!(result.suggestion.unwrap().contains("for 'Calculator'"));
    }

    #[test]
    fn context_tool_includes_runtime_diagnostic_for_empty_context() {
        let result = context_tool_result(
            ContextSnapshot {
                app: Some("unknown".into()),
                app_id: Some("unknown".into()),
                platform_app_id: None,
                bundle_id: None,
                pid: None,
                window: None,
                url: None,
                focused_element: None,
                interactive_elements: Vec::new(),
            },
            "macos",
            None,
            &[RuntimeDiagnostic {
                name: "macos_automation".into(),
                success: false,
                message: "macOS Automation denied for the Sootie launch path".into(),
                details: Some(json!({
                    "recovery": "Grant permission to the launcher, then restart it."
                })),
            }],
        );

        let suggestion = result.suggestion.unwrap();
        assert!(suggestion.contains("Runtime diagnostic"));
        assert!(suggestion.contains("macOS Automation denied"));
        assert!(suggestion.contains("Grant permission to the launcher"));
    }

    #[test]
    fn context_error_can_include_runtime_diagnostic_recovery() {
        let result = tool_error_with_runtime_diagnostic(
            "platform error: xprop failed".to_string(),
            &[RuntimeDiagnostic {
                name: "linux_xprop".into(),
                success: false,
                message: "Linux xprop/X11 probe failed for the Sootie launch path".into(),
                details: Some(json!({
                    "recovery": "Install xprop and run Sootie from an interactive X11 desktop session."
                })),
            }],
        );

        assert!(!result.success);
        let suggestion = result.suggestion.unwrap();
        assert!(suggestion.contains("Runtime diagnostic"));
        assert!(suggestion.contains("Linux xprop/X11 probe failed"));
        assert!(suggestion.contains("Install xprop"));
        assert!(suggestion.contains("sootie doctor --check"));
    }

    #[test]
    fn context_tool_returns_context_field() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_context","arguments":{}}),
        });
        let result = response.result.unwrap();
        assert_eq!(result["structuredContent"]["data"]["app"], "Fake");
        assert_eq!(result["structuredContent"]["data"]["app_id"], "Fake");
        assert_eq!(
            result["structuredContent"]["data"]["platform_app_id"],
            "fake"
        );
        assert_eq!(
            result["structuredContent"]["data"]["bundle_id"],
            "com.example.fake"
        );
        assert_eq!(result["structuredContent"]["data"]["pid"], 42);
        assert_eq!(
            result["structuredContent"]["data"]["interactive_elements"][0]["title"],
            "Submit"
        );
        assert_eq!(result["structuredContent"]["context"]["app"], "Fake");
    }

    #[test]
    fn find_tool_returns_counted_element_summaries() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_find","arguments":{"query":"Submit"}}),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["count"], 1);
        assert_eq!(data["total_matches"], 1);
        assert_eq!(data["elements"][0]["name"], "Submit");
        assert_eq!(data["elements"][0]["position"]["x"], 10.0);
    }

    #[test]
    fn find_tool_limits_elements_without_losing_total_matches() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_find","arguments":{"query":"Many","max_results":1}}),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["count"], 1);
        assert_eq!(data["total_matches"], 2);
        assert_eq!(data["elements"][0]["name"], "Submit");
    }

    #[test]
    fn find_tool_falls_back_to_context_elements() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_find",
                "arguments":{"query":"Ellipse","role":"AXGroup","app":"Safari"}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["count"], 1);
        assert_eq!(data["elements"][0]["role"], "AXGroup");
        assert_eq!(data["elements"][0]["name"], "Ellipse — O or 4");
        assert_eq!(data["elements"][0]["position"]["x"], 1400.0);
    }

    #[test]
    fn inspect_tool_falls_back_to_context_elements() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_inspect",
                "arguments":{"query":"Ellipse","role":"AXGroup","app":"Safari"}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["role"], "AXGroup");
        assert_eq!(data["name"], "Ellipse — O or 4");
        assert_eq!(data["position"]["x"], 1400.0);
    }

    #[test]
    fn read_tool_returns_content_shape() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_read","arguments":{}}),
        });
        let result = response.result.unwrap();
        assert_eq!(result["structuredContent"]["data"]["content"], "text");
        assert_eq!(result["structuredContent"]["data"]["item_count"], 1);
    }

    #[test]
    fn screenshot_tool_returns_portable_payload_fields() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_screenshot","arguments":{"full_resolution":true}}),
        });
        let result = response.result.unwrap();
        let content = result["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["data"], "abc123");
        assert_eq!(content[0]["mimeType"], "image/png");
        assert_eq!(content[1]["type"], "text");
        assert!(content[1]["text"].as_str().unwrap().contains("Main"));
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["mime_type"], "image/png");
        assert_eq!(data["image"], "abc123");
        assert_eq!(data["window_title"], "Main");
        assert_eq!(data["width"], 1600);
        assert_eq!(data["height"], 1200);
        assert_eq!(data["window_frame"]["x"], 1.0);
        assert_eq!(data["window_frame"]["width"], 800.0);
    }

    #[test]
    fn screenshot_payload_persists_decodable_image_artifact() {
        let image_bytes = b"png bytes";
        let image = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        let payload = screenshot_payload(Screenshot {
            mime_type: "image/png".into(),
            data_base64: image.clone(),
            width: Some(1),
            height: Some(1),
            window_title: Some("Main".into()),
            window_frame: None,
        });

        assert_eq!(payload["image"], image);
        let artifact_path = payload["artifact_path"].as_str().unwrap();
        assert!(artifact_path.ends_with(".png"));
        assert!(payload["artifact_uri"]
            .as_str()
            .unwrap()
            .starts_with("file://"));
        assert_eq!(std::fs::read(artifact_path).unwrap(), image_bytes);
        let _ = std::fs::remove_file(artifact_path);
    }

    #[test]
    fn vision_history_persists_annotated_grounding_screenshot_under_feature_directory() {
        let mut png_cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            100,
            80,
            image::Rgba([240, 240, 240, 255]),
        ))
        .write_to(&mut png_cursor, image::ImageFormat::Png)
        .unwrap();
        let image_bytes = png_cursor.into_inner();
        let screenshot = Screenshot {
            mime_type: "image/png".into(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(&image_bytes),
            width: Some(100),
            height: Some(80),
            window_title: Some("Vision".into()),
            window_frame: Some(Bounds {
                x: 1.0,
                y: 2.0,
                width: 100.0,
                height: 80.0,
            }),
        };
        let frame = VisionFrame::from_screenshot(&screenshot);
        let result = GroundResult {
            x: 50.0,
            y: 40.0,
            confidence: 0.91,
            method: "vision-match".into(),
            inference_ms: Some(7),
            raw_text: Some("submit".into()),
            bounds: None,
            response: json!({
                "matches": [
                    {
                        "label": "Submit button",
                        "confidence": 0.91,
                        "point": {"x": 0.5, "y": 0.5},
                        "bbox": {"x": 0.1, "y": 0.2, "width": 0.3, "height": 0.4}
                    },
                    {
                        "label": "Cancel button",
                        "confidence": 0.42,
                        "point": {"x": 0.75, "y": 0.3},
                        "bbox": {"x1": 0.6, "y1": 0.1, "x2": 0.9, "y2": 0.4}
                    }
                ]
            }),
        };
        let artifact = persist_grounding_history_screenshot(
            &screenshot,
            "submit flower",
            &frame,
            Some([0.0, 0.0, 5.0, 5.0]),
            &result,
        )
        .unwrap();

        assert!(artifact
            .image_path
            .starts_with(VISION_GROUNDING_HISTORY_DIR));
        assert!(artifact.image_path.ends_with(".jpg"));
        assert!(artifact
            .metadata_path
            .starts_with(VISION_GROUNDING_HISTORY_DIR));
        assert!(artifact.metadata_path.ends_with(".json"));
        assert_eq!(
            artifact.image_mime_type,
            VISION_GROUNDING_SCREENSHOT_MIME_TYPE
        );
        let jpg = std::fs::read(&artifact.image_path).unwrap();
        assert!(jpg.starts_with(&[0xff, 0xd8, 0xff]));
        let decoded = image::load_from_memory(&jpg).unwrap();
        assert_eq!(decoded.width(), 100);
        assert_eq!(decoded.height(), 80);
        let metadata = std::fs::read_to_string(&artifact.metadata_path).unwrap();
        assert!(metadata.contains("\"feature\": \"grounding\""));
        assert!(metadata.contains("\"history_dir\": \"/tmp/sootie/vision_history/grounding\""));
        assert!(metadata.contains("\"screenshot_mime_type\": \"image/jpeg\""));
        assert!(metadata.contains("\"predictions\""));
        assert!(metadata.contains("Submit button (0.91)"));
        assert!(metadata.contains("Cancel button (0.42)"));
        assert!(metadata.contains(&artifact.image_path));
        let _ = std::fs::remove_file(&artifact.image_path);
        let _ = std::fs::remove_file(&artifact.metadata_path);
    }

    #[test]
    fn screenshot_tool_does_not_accept_unadvertised_resolution_alias() {
        assert_tool_arg_rejected(
            "sootie_screenshot",
            json!({"fullResolution":true}),
            &["fullResolution"],
        );
    }

    #[test]
    fn screenshot_display_unavailable_returns_recovery_suggestion() {
        let result = screenshot_tool_result(Err(SootieError::Platform(
            "screencapture failed: could not create image from display".to_string(),
        )));
        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("could not create image from display"));
        assert!(result
            .suggestion
            .as_deref()
            .unwrap()
            .contains("active display"));
    }

    #[test]
    fn screenshot_blank_black_returns_recovery_suggestion() {
        let result = screenshot_tool_result(Err(SootieError::Platform(
            "screencapture returned a blank black image".to_string(),
        )));
        assert!(!result.success);
        assert!(result
            .suggestion
            .as_deref()
            .unwrap()
            .contains("Screen Recording"));
    }

    #[test]
    fn screenshot_locked_screen_returns_unlock_suggestion() {
        let result = screenshot_tool_result(Err(SootieError::Platform(
            "macOS screen is locked; screenshot would capture the lock screen instead of the target app"
                .to_string(),
        )));
        assert!(!result.success);
        let suggestion = result.suggestion.as_deref().unwrap();
        assert!(suggestion.contains("Unlock the Mac"));
        assert!(suggestion.contains("target window"));
    }

    #[test]
    fn parse_screen_returns_screenshot_and_elements() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_parse_screen","arguments":{"full_resolution":true}}),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["source"], "platform-context");
        assert_eq!(data["image"], "abc123");
        assert_eq!(data["width"], 1600);
        assert_eq!(data["height"], 1200);
        assert_eq!(data["element_count"], 2);
        assert_eq!(data["elements"][0]["role"], "AXButton");
    }

    #[test]
    fn parse_screen_uses_app_scope_for_context_and_screenshot() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::new(Box::new(RecordingBackend {
            events: events.clone(),
            fail_focus: false,
            record_context: true,
            record_screenshot: true,
        }));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_parse_screen",
                "arguments": { "app": "Safari", "window": "Excalidraw", "full_resolution": true }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["source"], "platform-context");
        assert_eq!(data["window_title"], "Main");
        assert_eq!(
            events.lock().unwrap().as_slice(),
            [
                "context:Safari".to_string(),
                "screenshot:Safari:Excalidraw:true".to_string()
            ]
        );
    }

    #[test]
    fn screenshot_uses_window_scope_for_capture() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::new(Box::new(RecordingBackend {
            events: events.clone(),
            fail_focus: false,
            record_context: false,
            record_screenshot: true,
        }));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_screenshot",
                "arguments": { "app": "Safari", "window": "Excalidraw" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["screenshot:Safari:Excalidraw:false".to_string()]
        );
    }

    #[test]
    fn screenshot_window_requires_app_scope() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_screenshot",
                "arguments": { "window": "Excalidraw" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("window requires app"));
    }

    #[test]
    fn annotate_tool_filters_roles_and_returns_screenshot_fields() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_annotate",
                "arguments":{"roles":["AXButton"],"max_labels":10}
            }),
        });
        let result = response.result.unwrap();
        let content = result["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image");
        assert_eq!(content[0]["mimeType"], "image/svg+xml");
        assert_eq!(content[1]["type"], "text");
        assert!(content[1]["text"].as_str().unwrap().contains("Submit"));
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["element_count"], 1);
        assert_eq!(data["elements"][0]["role"], "AXButton");
        assert_eq!(data["labels"][0]["label"], 1);
        assert_eq!(data["labels"][0]["position"]["x"], 60.0);
        assert_ne!(data["annotated_image"], "abc123");
        assert_eq!(data["annotated_image"], content[0]["data"]);
        assert_eq!(data["mime_type"], "image/svg+xml");
        assert_eq!(data["width"], 800);
        assert!(data["index"].as_str().unwrap().contains("Submit"));
        let svg_bytes = base64::engine::general_purpose::STANDARD
            .decode(data["annotated_image"].as_str().unwrap())
            .unwrap();
        let svg = String::from_utf8(svg_bytes).unwrap();
        assert!(svg.contains("data:image/png;base64,abc123"));
        assert!(svg.contains("<text"));
        assert!(svg.contains(">1</text>"));
    }

    #[test]
    fn annotate_tool_rejects_non_positive_max_labels() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_annotate",
                "arguments":{"max_labels":0}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("max_labels must be a positive integer"));
    }

    #[test]
    fn ground_tool_returns_best_point_fields() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_ground","arguments":{"description":"Submit"}}),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["x"], 60.0);
        assert_eq!(data["y"], 40.0);
        assert_eq!(data["confidence"], 1.0);
    }

    #[test]
    fn ground_tool_ranks_context_elements_by_description() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_ground","arguments":{"description":"Name"}}),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["x"], 120.0);
        assert_eq!(data["y"], 95.0);
        assert_eq!(data["confidence"], 1.0);
        assert_eq!(data["candidates"][0]["name"], "Name");
        assert_eq!(data["source"], "platform-find");
    }

    #[test]
    fn ground_tool_filters_candidates_by_crop_box() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_ground",
                "arguments":{"description":"Submit","crop_box":[0,0,50,50]}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["candidates"].as_array().unwrap().len(), 0);
        assert_eq!(data["confidence"], 0.0);
        assert_eq!(data["crop_box"], json!([0.0, 0.0, 50.0, 50.0]));
        assert!(data.get("x").is_none());
    }

    #[test]
    fn ground_tool_falls_back_to_vision_coordinates() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 120.0,
            "y": 80.0,
            "confidence": 0.84,
            "method": "full-screen",
            "raw": "[0.3, 0.26]",
            "inference_ms": 12
        }));
        let mut server = McpServer::with_vision_config(
            Box::new(VisionOnlyBackend),
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_ground",
                "arguments":{"description":"canvas flower","crop_box":[20,30,260,220]}
            }),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["source"], "vision-ground");
        assert_eq!(data["x"], 130.0);
        assert_eq!(data["y"], 100.0);
        assert_eq!(data["confidence"], 0.84);
        assert_eq!(data["method"], "full-screen");
        assert_eq!(
            data["grounding_frame"],
            json!({"x":10.0,"y":20.0,"width":400.0,"height":300.0})
        );
        let request = handle.join().unwrap();
        assert!(request.contains("\"description\":\"canvas flower\""));
        assert!(request.contains("\"screen_w\":400.0"));
        assert!(request.contains("\"crop_box\":[10.0,10.0,250.0,200.0]"));
    }

    #[test]
    fn vision_only_ground_uses_vision_even_when_platform_has_candidate() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 200.0,
            "y": 100.0,
            "confidence": 0.88,
            "method": "full-screen"
        }));
        let mut server = McpServer::with_runtime_config(
            Box::new(FakeBackend),
            ResolutionStrategy::VisionOnly,
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_ground","arguments":{"description":"Submit"}}),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["source"], "vision-ground");
        assert_eq!(data["x"], 201.0);
        assert_eq!(data["y"], 102.0);
        assert_eq!(data["confidence"], 0.88);
        let request = handle.join().unwrap();
        assert!(request.contains("\"description\":\"Submit\""));
    }

    #[test]
    fn find_tool_falls_back_to_vision_synthetic_element() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 42.0,
            "y": 24.0,
            "confidence": 0.72,
            "method": "full-screen"
        }));
        let mut server = McpServer::with_vision_config(
            Box::new(VisionOnlyBackend),
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_find","arguments":{"query":"visual target"}}),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["count"], 1);
        assert_eq!(data["elements"][0]["role"], "VisionTarget");
        assert_eq!(data["elements"][0]["position"], json!({"x":32.0,"y":24.0}));
        assert_eq!(
            data["elements"][0]["size"],
            json!({"width":40.0,"height":40.0})
        );
        handle.join().unwrap();
    }

    #[test]
    fn vision_only_find_skips_platform_candidate() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 42.0,
            "y": 24.0,
            "confidence": 0.72,
            "method": "full-screen"
        }));
        let mut server = McpServer::with_runtime_config(
            Box::new(FakeBackend),
            ResolutionStrategy::VisionOnly,
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_find","arguments":{"query":"Submit"}}),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["count"], 1);
        assert_eq!(data["elements"][0]["role"], "VisionTarget");
        assert_eq!(data["elements"][0]["position"], json!({"x":23.0,"y":6.0}));
        let request = handle.join().unwrap();
        assert!(request.contains("\"description\":\"Submit\""));
    }

    #[test]
    fn ground_tool_rejects_unadvertised_candidate_filters() {
        assert_tool_arg_rejected(
            "sootie_ground",
            json!({"description":"Name","roles":"textfield","max_candidates":"1"}),
            &["roles", "max_candidates"],
        );
    }

    #[test]
    fn ground_tool_rejects_non_positive_max_candidates() {
        assert_tool_arg_rejected(
            "sootie_ground",
            json!({"description":"Submit","max_candidates":0}),
            &["max_candidates"],
        );
    }

    #[test]
    fn crop_box_accepts_numeric_string_values() {
        assert_eq!(
            crop_box_arg(&json!({"crop_box":["0","1.5","50","100"]})).unwrap(),
            Some((0.0, 1.5, 50.0, 100.0))
        );
    }

    #[test]
    fn crop_box_rejects_non_finite_values() {
        let error = crop_box_arg(&json!({"crop_box":["0","NaN","50","100"]})).unwrap_err();
        assert!(error
            .to_string()
            .contains("crop_box values must be finite numbers"));
    }

    #[test]
    fn find_query_accepts_nested_target_selector() {
        let query = find_query(&json!({
            "target": {
                "app": {"app_id": "Fake"},
                "selector": {
                    "name": "Submit",
                    "role": "button",
                    "id": "submit-button",
                    "dom_class": "primary"
                }
            },
            "domId": "legacy-id",
            "domClass": "legacy-class"
        }));
        assert_eq!(query.app.as_deref(), Some("Fake"));
        assert_eq!(query.query.as_deref(), Some("Submit"));
        assert_eq!(query.role.as_deref(), Some("button"));
        assert_eq!(query.dom_id.as_deref(), Some("submit-button"));
        assert_eq!(query.dom_class.as_deref(), Some("primary"));
    }

    #[test]
    fn find_query_accepts_target_app_identity_fields() {
        let query = find_query(&json!({
            "target": {
                "app_id": "Fake",
                "selector": {"name": "Submit"}
            }
        }));
        assert_eq!(query.app.as_deref(), Some("Fake"));
        assert_eq!(query.query.as_deref(), Some("Submit"));

        let query = find_query(&json!({
            "target": {
                "platform_app_id": "com.example.fake",
                "selector": {"name": "Submit"}
            }
        }));
        assert_eq!(query.app.as_deref(), Some("com.example.fake"));
    }

    #[test]
    fn find_query_accepts_description_aliases() {
        assert_eq!(
            find_query(&json!({"description":"Submit"}))
                .query
                .as_deref(),
            Some("Submit")
        );
        assert_eq!(
            find_query(&json!({"el_description":"URL input field"}))
                .query
                .as_deref(),
            Some("URL input field")
        );
    }

    #[test]
    fn app_arg_accepts_compatible_shapes() {
        assert_eq!(app_arg(&json!({"app":"Fake"})).as_deref(), Some("Fake"));
        assert_eq!(
            app_arg(&json!({"app":{"name":"Fake"}})).as_deref(),
            Some("Fake")
        );
        assert_eq!(
            app_arg(&json!({"app":{"app_id":"Fake"}})).as_deref(),
            Some("Fake")
        );
        assert_eq!(
            app_arg(&json!({"to_app":{"name":"Finder","bundle_id":"com.apple.finder"}})).as_deref(),
            Some("Finder")
        );
        assert_eq!(
            bundle_arg(&json!({"to_app":{"name":"Finder","bundle_id":"com.apple.finder"}}))
                .as_deref(),
            Some("com.apple.finder")
        );
        assert_eq!(
            bundle_arg(&json!({"to_app":{"name":"Finder","platform_app_id":"com.apple.finder"}}))
                .as_deref(),
            Some("com.apple.finder")
        );
        assert_eq!(
            required_app_arg(&json!({"to_bundle_id":"com.apple.finder"})).unwrap(),
            "com.apple.finder"
        );
        assert_eq!(
            optional_app_arg(&json!({"bundle_id":"com.apple.finder"})).as_deref(),
            Some("com.apple.finder")
        );
        assert_eq!(
            optional_app_arg(&json!({"to_platform_app_id":"com.apple.finder"})).as_deref(),
            Some("com.apple.finder")
        );
        assert_eq!(
            find_query(&json!({"platform_app_id":"com.example.fake","query":"Submit"}))
                .app
                .as_deref(),
            Some("com.example.fake")
        );
    }

    #[test]
    fn scalar_args_accept_compatible_string_and_float_shapes() {
        assert_eq!(u32_arg(&json!({"count":"2"}), "count"), Some(2));
        assert_eq!(u32_arg(&json!({"count":2.0}), "count"), Some(2));
        assert_eq!(i32_arg(&json!({"amount":"-3"}), "amount"), Some(-3));
        assert_eq!(i32_arg(&json!({"amount":3.0}), "amount"), Some(3));
        assert_eq!(f64_arg(&json!({"timeout":"0.25"}), "timeout"), Some(0.25));
        assert_eq!(
            bool_arg(&json!({"clear_first":"true"}), "clear_first"),
            Some(true)
        );
        assert_eq!(bool_arg(&json!({"clear":"false"}), "clear"), Some(false));
        assert_eq!(
            nested_bool_arg(
                &json!({"include":{"screenshot":"true"}}),
                "include",
                "screenshot"
            ),
            Some(true)
        );
        assert_eq!(u32_arg(&json!({"count":2.5}), "count"), None);
    }

    #[test]
    fn string_array_args_accept_comma_separated_strings() {
        assert_eq!(
            string_array_arg(&json!({"keys":"cmd, shift, l"}), "keys"),
            vec!["cmd", "shift", "l"]
        );
        assert_eq!(
            string_array_arg(&json!({"roles":"button,, textfield "}), "roles"),
            vec!["button", "textfield"]
        );
        assert_eq!(
            string_array_required(&json!({"modifiers":"cmd"}), "modifiers").unwrap(),
            vec!["cmd"]
        );
    }

    #[test]
    fn public_tool_calls_reject_values_outside_advertised_schema_types() {
        for (tool, arguments, expected) in [
            (
                "sootie_hotkey",
                json!({"keys":"cmd,l"}),
                "sootie_hotkey.keys must be an array of string",
            ),
            (
                "sootie_press",
                json!({"key":"tab","modifiers":["shift", 1]}),
                "modifiers[1] must be a string",
            ),
            (
                "sootie_type",
                json!({"text":"hello","clear":"true"}),
                "sootie_type.clear must be a boolean",
            ),
            (
                "sootie_browser_scroll",
                json!({"amount": []}),
                "sootie_browser_scroll.amount must match one of the advertised schema variants",
            ),
        ] {
            let result = call_tool(tool, arguments);
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn browser_mutating_tools_apply_policy_before_cdp_connection() {
        for (tool, arguments, expected) in [
            (
                "sootie_browser_storage",
                json!({
                    "port": 9,
                    "area": "localStorage",
                    "action": "get",
                    "key": "token"
                }),
                "POLICY_BLOCKED: browser storage access requires unsafe=true",
            ),
            (
                "sootie_browser_cookies",
                json!({
                    "port": 9,
                    "action": "list"
                }),
                "POLICY_BLOCKED: browser cookie access requires unsafe=true",
            ),
            (
                "sootie_cdp_send",
                json!({
                    "port": 9,
                    "method": "Browser.getVersion"
                }),
                "POLICY_BLOCKED: raw CDP requires unsafe=true",
            ),
            (
                "sootie_cdp_subscribe",
                json!({
                    "port": 9,
                    "domain": "Log"
                }),
                "POLICY_BLOCKED: raw CDP requires unsafe=true",
            ),
        ] {
            let result = call_tool(tool, arguments);
            assert_eq!(result["isError"], true, "{tool}");
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn public_tool_calls_reject_missing_advertised_required_args_before_dispatch() {
        for (tool, arguments, expected) in [
            (
                "sootie_recipe_save",
                json!({}),
                "sootie_recipe_save requires argument(s): recipe_json",
            ),
            (
                "sootie_drag",
                json!({"to_x": 10.0}),
                "sootie_drag requires argument(s): to_y",
            ),
            (
                "sootie_window",
                json!({"app": "Fake"}),
                "sootie_window requires argument(s): action",
            ),
        ] {
            let result = call_tool(tool, arguments);
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn action_tool_rejects_unadvertised_nested_target() {
        assert_tool_arg_rejected(
            "sootie_click",
            json!({
                "target":{
                    "app":"Fake",
                    "coordinate":{"x":42.0,"y":24.0}
                },
                "button":"right"
            }),
            &["target"],
        );
    }

    #[test]
    fn nested_target_coordinates_accept_numeric_strings() {
        assert_eq!(
            point_arg(
                &json!({"target":{"coordinate":{"x":"42.5","y":"24"}}}),
                "x",
                "y",
                "target"
            )
            .unwrap(),
            Some((42.5, 24.0))
        );
        assert_eq!(
            point_arg(
                &json!({"target":{"x":"11","y":"22.25"}}),
                "x",
                "y",
                "target"
            )
            .unwrap(),
            Some((11.0, 22.25))
        );
    }

    #[test]
    fn coordinates_reject_non_finite_values() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_element_at","arguments":{"x":"NaN","y":2.0}}),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("sootie_element_at.x must be a number"));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_click","arguments":{"x":"NaN","y":2.0}}),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("sootie_click.x must be a number"));

        let error = point_arg(
            &json!({"target":{"coordinate":{"x":"42","y":"NaN"}}}),
            "x",
            "y",
            "target",
        )
        .unwrap_err();
        assert!(error.to_string().contains("y must be a finite number"));
    }

    #[test]
    fn action_tool_flattens_data_and_returns_context() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_click",
                "arguments":{"x":12.0,"y":34.0,"button":"right","count":2,"app":"Fake"}
            }),
        });
        let result = response.result.unwrap();
        let structured = &result["structuredContent"];
        let data = &structured["data"];
        assert_eq!(data["method"], "fake-click");
        assert_eq!(data["x"], 12.0);
        assert_eq!(data["y"], 34.0);
        assert_eq!(data["button"], "right");
        assert_eq!(data["count"], 2);
        assert!(data.get("details").is_none());
        assert_eq!(structured["context"]["app"], "Fake");
    }

    #[test]
    fn click_tool_falls_back_to_vision_coordinates_after_not_found() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 25.0,
            "y": 35.0,
            "confidence": 0.91,
            "method": "full-screen"
        }));
        let mut server = McpServer::with_vision_config(
            Box::new(VisionOnlyBackend),
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_click",
                "arguments":{"query":"visual send button","button":"right","count":2}
            }),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "vision-grounded-click");
        assert_eq!(data["x"], 35.0);
        assert_eq!(data["y"], 55.0);
        assert_eq!(data["confidence"], 0.91);
        assert_eq!(data["dispatch"]["method"], "fake-click");
        assert_eq!(data["dispatch"]["x"], 35.0);
        assert_eq!(data["dispatch"]["button"], "right");
        assert_eq!(data["dispatch"]["count"], 2);
        let request = handle.join().unwrap();
        assert!(request.contains("\"description\":\"visual send button\""));
    }

    #[test]
    fn vision_only_click_resolves_target_before_platform_action() {
        let (url, handle) = spawn_vision_ground_server(json!({
            "x": 25.0,
            "y": 35.0,
            "confidence": 0.91,
            "method": "full-screen"
        }));
        let mut server = McpServer::with_runtime_config(
            Box::new(FakeBackend),
            ResolutionStrategy::VisionOnly,
            VisionConfig::for_tests(url),
        );

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_click",
                "arguments":{"query":"Submit","button":"right","count":2}
            }),
        });

        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "vision-grounded-click");
        assert_eq!(data["x"], 26.0);
        assert_eq!(data["y"], 37.0);
        assert_eq!(data["dispatch"]["method"], "fake-click");
        assert_eq!(data["dispatch"]["x"], 26.0);
        assert_eq!(data["dispatch"]["button"], "right");
        let request = handle.join().unwrap();
        assert!(request.contains("\"description\":\"Submit\""));
    }

    #[test]
    fn app_scoped_coordinate_click_focuses_before_dispatch() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::new(Box::new(RecordingBackend {
            events: events.clone(),
            fail_focus: false,
            record_context: false,
            record_screenshot: false,
        }));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_click",
                "arguments": { "app": "Safari", "x": 12.0, "y": 34.0 }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["focus:Safari".to_string(), "click".to_string()]
        );
    }

    #[test]
    fn app_scoped_coordinate_click_stops_when_focus_fails() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::new(Box::new(RecordingBackend {
            events: events.clone(),
            fail_focus: true,
            record_context: false,
            record_screenshot: false,
        }));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_click",
                "arguments": { "app": "Safari", "x": 12.0, "y": 34.0 }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["focus:Safari".to_string()]
        );
    }

    #[test]
    fn app_scoped_coordinate_drag_focuses_before_dispatch() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let mut server = McpServer::new(Box::new(RecordingBackend {
            events: events.clone(),
            fail_focus: false,
            record_context: false,
            record_screenshot: false,
        }));

        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_drag",
                "arguments": {
                    "app": "Safari",
                    "from_x": 12.0,
                    "from_y": 34.0,
                    "to_x": 56.0,
                    "to_y": 78.0
                }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["focus:Safari".to_string(), "drag".to_string()]
        );
    }

    #[test]
    fn tool_call_accepts_compatible_argument_envelopes() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        for (id, arguments) in [
            (
                1,
                json!({"data":{"x":12.0,"y":34.0,"button":"right","count":2}}),
            ),
            (
                2,
                json!({"input":{"x":12.0,"y":34.0,"button":"right","count":2}}),
            ),
            (
                3,
                json!({"params":{"x":12.0,"y":34.0,"button":"right","count":2}}),
            ),
        ] {
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(id)),
                method: "tools/call".into(),
                params: json!({
                    "name":"sootie_click",
                    "arguments": arguments
                }),
            });
            let data = &response.result.unwrap()["structuredContent"]["data"];
            assert_eq!(data["x"], 12.0);
            assert_eq!(data["y"], 34.0);
            assert_eq!(data["button"], "right");
            assert_eq!(data["count"], 2);
        }
    }

    #[test]
    fn tool_call_accepts_top_level_data_and_input_envelopes() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        for (id, key) in [(1, "data"), (2, "input")] {
            let mut params = json!({"name":"sootie_click"});
            params[key] = json!({"x":12.0,"y":34.0});
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(id)),
                method: "tools/call".into(),
                params,
            });
            let data = &response.result.unwrap()["structuredContent"]["data"];
            assert_eq!(data["x"], 12.0);
            assert_eq!(data["y"], 34.0);
        }
    }

    #[test]
    fn recipe_run_preserves_params_argument() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        store
            .save(
                &parse_recipe(&json!({
                    "schema_version": 2,
                    "name": "type-param",
                    "params": {
                        "message": {"type":"string","required":true}
                    },
                    "steps": [{
                        "tool": "sootie_type",
                        "args": {"text":"{{message}}"}
                    }]
                }))
                .unwrap(),
            )
            .unwrap();
        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_run",
                "arguments":{"recipe":"type-param","params":{"message":"hello"}}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(
            result["structuredContent"]["data"]["steps"][0]["data"]["text"],
            "hello"
        );
    }

    #[test]
    fn state_tool_returns_app_identity_fields() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_state",
                "arguments":{"app":"Fake"}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["structuredContent"]["data"]["app_count"], 1);
        let app = &result["structuredContent"]["data"]["apps"][0];
        assert_eq!(app["name"], "Fake");
        assert_eq!(app["app_id"], "Fake");
        assert_eq!(app["platform_app_id"], "fake");
        assert_eq!(app["bundle_id"], Value::Null);
    }

    #[test]
    fn type_tool_accepts_clear() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_type",
                "arguments":{"text":"hello","clear":true}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "fake-type");
        assert_eq!(data["text"], "hello");
        assert_eq!(data["clear"], true);
    }

    #[test]
    fn focus_tool_rejects_unadvertised_app_alias() {
        assert_tool_arg_rejected(
            "sootie_focus",
            json!({"to_app":{"name":"Fake","bundle_id":"com.example.fake"}}),
            &["to_app"],
        );
    }

    #[test]
    fn drag_tool_passes_hold_duration() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_drag",
                "arguments":{"from_x":1.0,"from_y":2.0,"to_x":3.0,"to_y":4.0,"duration":0.8,"hold_duration":0.25}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "fake-drag");
        assert_eq!(data["duration"], 0.8);
        assert_eq!(data["hold_duration"], 0.25);
    }

    #[test]
    fn action_tools_reject_unadvertised_millisecond_duration_aliases() {
        assert_tool_arg_rejected(
            "sootie_long_press",
            json!({"x":1.0,"y":2.0,"duration_ms":1500,"button":"right"}),
            &["duration_ms"],
        );
        assert_tool_arg_rejected(
            "sootie_drag",
            json!({"from_x":1.0,"from_y":2.0,"to_x":3.0,"to_y":4.0,"duration_ms":800,"hold_duration_ms":250}),
            &["duration_ms", "hold_duration_ms"],
        );
    }

    #[test]
    fn action_tools_reject_negative_durations() {
        for (tool, arguments) in [
            (
                "sootie_long_press",
                json!({"x":1.0,"y":2.0,"duration":-0.1}),
            ),
            (
                "sootie_drag",
                json!({"from_x":1.0,"from_y":2.0,"to_x":3.0,"to_y":4.0,"hold_duration":-1}),
            ),
        ] {
            let mut server = McpServer::new(Box::new(FakeBackend));
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name": tool,
                    "arguments": arguments
                }),
            });
            let result = response.result.unwrap();
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("non-negative finite duration"));
        }
    }

    #[test]
    fn action_tools_reject_non_positive_counts_and_amounts() {
        for (tool, arguments, expected) in [
            (
                "sootie_click",
                json!({"x":1.0,"y":2.0,"count":0}),
                "count must be a positive integer",
            ),
            (
                "sootie_scroll",
                json!({"direction":"down","amount":-1}),
                "amount must be a positive integer",
            ),
        ] {
            let mut server = McpServer::new(Box::new(FakeBackend));
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name": tool,
                    "arguments": arguments
                }),
            });
            let result = response.result.unwrap();
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn pointer_tools_reject_unknown_mouse_buttons() {
        for tool in ["sootie_click", "sootie_long_press"] {
            let mut server = McpServer::new(Box::new(FakeBackend));
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name": tool,
                    "arguments":{"x":1.0,"y":2.0,"button":"primary"}
                }),
            });
            let result = response.result.unwrap();
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("unsupported mouse button"));
        }
    }

    #[test]
    fn mouse_button_arg_normalizes_supported_values() {
        assert_eq!(
            mouse_button_arg(&json!({"button":" RIGHT "})).unwrap(),
            "right"
        );
        assert_eq!(mouse_button_arg(&json!({})).unwrap(), "left");
    }

    #[test]
    fn scroll_tool_rejects_unknown_directions() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_scroll",
                "arguments":{"direction":"diagonal"}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("unsupported scroll direction"));
    }

    #[test]
    fn scroll_direction_arg_normalizes_supported_values() {
        assert_eq!(
            scroll_direction_arg(&json!({"direction":" UP "})).unwrap(),
            "up"
        );
    }

    #[test]
    fn scroll_tool_rejects_unadvertised_nested_target() {
        assert_tool_arg_rejected(
            "sootie_scroll",
            json!({
                "direction":"down",
                "amount":2,
                "target":{
                    "app":"Fake",
                    "selector":{"name":"Submit"}
                }
            }),
            &["target"],
        );
    }

    #[test]
    fn window_move_fills_missing_size_from_current_window() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_window",
                "arguments":{"action":"move","app":"Fake","x":40.0,"y":50.0}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "fake-window");
        assert_eq!(data["command"], "move");
        assert_eq!(data["platform_app_id"], Value::Null);
        assert_eq!(data["bounds"]["x"], 40.0);
        assert_eq!(data["bounds"]["y"], 50.0);
        assert_eq!(data["bounds"]["width"], 800.0);
        assert_eq!(data["bounds"]["height"], 600.0);
    }

    #[test]
    fn window_resize_fills_missing_position_from_current_window() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_window",
                "arguments":{"action":"resize","app":"Fake","width":300.0,"height":200.0}
            }),
        });
        let result = response.result.unwrap();
        let data = &result["structuredContent"]["data"];
        assert_eq!(data["method"], "fake-window");
        assert_eq!(data["command"], "resize");
        assert_eq!(data["bounds"]["x"], 1.0);
        assert_eq!(data["bounds"]["y"], 2.0);
        assert_eq!(data["bounds"]["width"], 300.0);
        assert_eq!(data["bounds"]["height"], 200.0);
    }

    #[test]
    fn window_resize_rejects_unadvertised_bounds_object_alias() {
        assert_tool_arg_rejected(
            "sootie_window",
            json!({
                "action":"resize",
                "app":"Fake",
                "bounds":{"position":{"x":"10","y":"20"},"size":{"width":"300","height":"200"}}
            }),
            &["bounds"],
        );
    }

    #[test]
    fn window_move_rejects_unadvertised_bounds_array_alias() {
        assert_tool_arg_rejected(
            "sootie_window",
            json!({"action":"move","app":"Fake","bounds":["10","20","300","200"]}),
            &["bounds"],
        );
    }

    #[test]
    fn window_resize_rejects_invalid_bounds() {
        for (arguments, expected) in [
            (
                json!({"action":"resize","app":"Fake","width":0.0,"height":200.0}),
                "width/height must be positive numbers",
            ),
            (
                json!({"action":"resize","app":"Fake","width":300.0,"height":200.0,"x":"NaN"}),
                "sootie_window.x must be a number",
            ),
            (
                json!({"action":"resize","app":"Fake","bounds":{"width":0.0,"height":200.0}}),
                "does not accept argument(s): bounds",
            ),
        ] {
            let mut server = McpServer::new(Box::new(FakeBackend));
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name":"sootie_window",
                    "arguments": arguments
                }),
            });
            let result = response.result.unwrap();
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn wait_title_changed_compares_supplied_baseline() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_wait",
                "arguments":{"condition":"titleChanged","value":"Old","timeout":0.1,"interval":0.05}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["structuredContent"]["data"]["matched"], true);
    }

    #[test]
    fn wait_title_contains_rejects_unadvertised_title_alias() {
        assert_tool_arg_rejected(
            "sootie_wait",
            json!({"condition":"titleContains","title":"Missing","timeout":0.01,"interval":0.01}),
            &["title"],
        );
    }

    #[test]
    fn wait_url_contains_rejects_unadvertised_url_alias() {
        assert_tool_arg_rejected(
            "sootie_wait",
            json!({"condition":"urlContains","url":"not-present","timeout":0.01,"interval":0.01}),
            &["url"],
        );
    }

    #[test]
    fn wait_tool_rejects_unadvertised_millisecond_timeout_aliases() {
        assert_tool_arg_rejected(
            "sootie_wait",
            json!({"condition":"titleChanged","value":"Old","timeout_ms":100,"interval_ms":50}),
            &["timeout_ms", "interval_ms"],
        );
    }

    #[test]
    fn wait_tool_rejects_invalid_timing_arguments() {
        for (arguments, expected) in [
            (
                json!({"condition":"titleContains","value":"Main","timeout":-1}),
                "timeout/timeout_ms must be a non-negative finite duration",
            ),
            (
                json!({"condition":"titleContains","value":"Main","interval":"NaN"}),
                "sootie_wait.interval must be a number",
            ),
        ] {
            let mut server = McpServer::new(Box::new(FakeBackend));
            let response = server.handle_request(JsonRpcRequest {
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: "tools/call".into(),
                params: json!({
                    "name":"sootie_wait",
                    "arguments": arguments
                }),
            });
            let result = response.result.unwrap();
            assert_eq!(result["isError"], true);
            assert!(result["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains(expected));
        }
    }

    #[test]
    fn wait_element_exists_rejects_unadvertised_nested_target() {
        assert_tool_arg_rejected(
            "sootie_wait",
            json!({
                "condition":"elementExists",
                "target":{
                    "app":"Fake",
                    "selector":{"name":"Submit"}
                },
                "timeout":0.1,
                "interval":0.05
            }),
            &["target"],
        );
    }

    #[test]
    fn learning_mode_records_successful_tool_actions() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let start = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_learn_start",
                "arguments":{"task_description":"record a shortcut"}
            }),
        });
        let start_data = &start.result.unwrap()["structuredContent"]["data"];
        assert_eq!(start_data["status"], "recording");
        assert!(start_data.get("active").is_none());
        assert!(start_data.get("action_count").is_none());
        let _ = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_press",
                "arguments":{"key":"tab","modifiers":["shift"],"app":"Fake"}
            }),
        });
        let status = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_learn_status","arguments":{}}),
        });
        assert_eq!(
            status.result.unwrap()["structuredContent"]["data"]["action_count"],
            1
        );

        let stop = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(4)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_learn_stop","arguments":{}}),
        });
        let data = &stop.result.unwrap()["structuredContent"]["data"];
        assert_eq!(data["action_count"], 1);
        let action = &data["actions"][0];
        assert_eq!(action["action_type"], "keyPress");
        assert_eq!(action["key_name"], "tab");
        assert_eq!(action["key_code"], Value::Null);
        assert_eq!(action["modifiers"], json!(["shift"]));
        assert_eq!(action["app"], "Fake");
        assert_eq!(action["window"], "Main");
        assert_eq!(action["url"], "https://example.com/current");
        assert!(action.get("tool").is_none());
        assert!(action.get("duration_ms").is_none());
        assert_eq!(data["task_description"], "record a shortcut");
        assert_eq!(data["apps"], json!(["Fake"]));
        assert_eq!(data["urls"], json!(["https://example.com/current"]));
        assert_eq!(data["recipe"]["name"], "record-a-shortcut");
        assert_eq!(data["recipe"]["app"], "Fake");
        assert_eq!(data["recipe"]["steps"][0]["action"], "press");
        assert_eq!(data["recipe"]["steps"][0]["key"], "tab");
        assert_eq!(
            data["recipe"]["steps"][0]["params"]["modifiers"],
            json!(["shift"])
        );
        assert!(data["recipe_json"]
            .as_str()
            .is_some_and(|text| text.contains("\"record-a-shortcut\"")));
    }

    #[test]
    fn learning_mode_records_clipboard_payload_before_paste_hotkey() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let _ = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_learn_start",
                "arguments":{"task_description":"paste svg"}
            }),
        });
        let _ = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_hotkey",
                "arguments":{"keys":["cmd","v"],"app":"Fake"}
            }),
        });
        let stop = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_learn_stop","arguments":{}}),
        });
        let data = &stop.result.unwrap()["structuredContent"]["data"];
        assert_eq!(data["actions"][0]["clipboard_text"], "<svg/>");
        assert_eq!(data["recipe"]["steps"][0]["action"], "set_clipboard");
        assert_eq!(data["recipe"]["steps"][0]["text"], "<svg/>");
        assert_eq!(data["recipe"]["steps"][1]["action"], "hotkey");
        assert_eq!(data["recipe"]["steps"][1]["keys"], json!(["cmd", "v"]));
    }

    #[test]
    fn learning_mode_records_window_relative_coordinate_metadata() {
        let mut server = McpServer::new(Box::new(FakeBackend));
        let _ = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_learn_start",
                "arguments":{"task_description":"record a click"}
            }),
        });
        let _ = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(2)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_click",
                "arguments":{"app":"Fake","x":41.0,"y":62.0}
            }),
        });

        let stop = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(3)),
            method: "tools/call".into(),
            params: json!({"name":"sootie_learn_stop","arguments":{}}),
        });
        let stop_result = stop.result.unwrap();
        let data = &stop_result["structuredContent"]["data"];
        let action = &data["actions"][0];
        assert_eq!(action["action_type"], "click");
        assert_eq!(action["coordinate_space"], "screen");
        assert_eq!(action["screen_coordinate"], json!({"x":41.0,"y":62.0}));
        assert_eq!(action["window_frame"]["x"], 1.0);
        assert_eq!(action["window_frame"]["y"], 2.0);
        assert_eq!(action["window_coordinate"], json!({"x":40.0,"y":60.0}));
        assert_eq!(
            action["window_normalized_coordinate"],
            json!({"x":0.05,"y":0.1})
        );
        let recipe = &data["recipe"];
        assert_eq!(recipe["name"], "record-a-click");
        assert_eq!(recipe["steps"][0]["action"], "click");
        assert_eq!(
            recipe["steps"][0]["target"]["window_normalized_coordinate"],
            json!({"x":0.05,"y":0.1})
        );
    }

    #[test]
    fn learning_event_maps_scroll_and_focus_shapes() {
        let scroll = learned_action_event(
            "sootie_scroll",
            &json!({"direction":" LEFT ","amount":"4","x":10.0,"y":20.0,"app":"Fake"}),
        )
        .unwrap();
        assert_eq!(scroll["action_type"], "scroll");
        assert_eq!(scroll["delta_x"], -4);
        assert_eq!(scroll["delta_y"], 0);
        assert_eq!(scroll["x"], 10.0);
        assert_eq!(scroll["y"], 20.0);
        assert_eq!(scroll["app"], "Fake");

        let focus = learned_action_event(
            "sootie_focus",
            &json!({"to_app":{"name":"Finder","bundle_id":"com.apple.finder"}}),
        )
        .unwrap();
        assert_eq!(focus["action_type"], "appSwitch");
        assert_eq!(focus["app"], Value::Null);
        assert_eq!(focus["to_app"], "Finder");
        assert_eq!(focus["to_bundle_id"], "com.apple.finder");
        assert_eq!(learned_apps(&[scroll, focus]), vec!["Fake", "Finder"]);
    }

    #[test]
    fn learning_event_maps_pointer_drag_and_window_shapes() {
        let click = learned_action_event(
            "sootie_click",
            &json!({
                "query":"Run",
                "target":{"selector":{"dom_id":"run"}},
                "button":" RIGHT ",
                "count":"2"
            }),
        )
        .unwrap();
        assert_eq!(click["action_type"], "click");
        assert_eq!(click["query"], "Run");
        assert_eq!(click["target"]["selector"]["dom_id"], "run");
        assert_eq!(click["button"], "right");
        assert_eq!(click["count"], 2);

        let hover = learned_action_event(
            "sootie_hover",
            &json!({
                "app":"Fake",
                "target":{"coordinate":{"x":11.0,"y":22.0}},
                "query":"Submit"
            }),
        )
        .unwrap();
        assert_eq!(hover["action_type"], "hover");
        assert_eq!(hover["x"], 11.0);
        assert_eq!(hover["y"], 22.0);
        assert_eq!(hover["query"], "Submit");
        assert_eq!(hover["target"]["coordinate"]["x"], 11.0);

        let long_press = learned_action_event(
            "sootie_long_press",
            &json!({"x":1.0,"y":2.0,"duration_ms":1500,"button":" MIDDLE ","query":"Hold"}),
        )
        .unwrap();
        assert_eq!(long_press["action_type"], "longPress");
        assert_eq!(long_press["duration"], 1.5);
        assert_eq!(long_press["button"], "middle");
        assert_eq!(long_press["query"], "Hold");

        let drag = learned_action_event(
            "sootie_drag",
            &json!({
                "from_target":{"coordinate":{"x":3.0,"y":4.0}},
                "to_target":{"coordinate":{"x":30.0,"y":40.0}},
                "duration_ms":700,
                "hold_duration_ms":200
            }),
        )
        .unwrap();
        assert_eq!(drag["action_type"], "drag");
        assert_eq!(drag["from_x"], 3.0);
        assert_eq!(drag["from_y"], 4.0);
        assert_eq!(drag["to_x"], 30.0);
        assert_eq!(drag["to_y"], 40.0);
        assert_eq!(drag["from_target"]["coordinate"]["x"], 3.0);
        assert_eq!(drag["to_target"]["coordinate"]["x"], 30.0);
        assert_eq!(drag["duration"], 0.7);
        assert_eq!(drag["hold_duration"], 0.2);

        let typed = learned_action_event(
            "sootie_type",
            &json!({"text":"hello","target":{"selector":{"dom_id":"name"}}}),
        )
        .unwrap();
        assert_eq!(typed["action_type"], "typeText");
        assert_eq!(typed["text"], "hello");
        assert_eq!(typed["target"]["selector"]["dom_id"], "name");

        let window = learned_action_event(
            "sootie_window",
            &json!({"app":"Fake","action":"resize","width":300.0,"height":200.0}),
        )
        .unwrap();
        assert_eq!(window["action_type"], "window");
        assert_eq!(window["command"], "resize");
        assert_eq!(window["width"], 300.0);
        assert_eq!(window["height"], 200.0);
    }

    #[test]
    fn recipe_save_requires_recipe_json_parameter() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_recipe_save",
                "arguments":{}
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(
            result["structuredContent"]["error"],
            "invalid arguments: sootie_recipe_save requires argument(s): recipe_json"
        );
    }

    #[test]
    fn recipe_save_rejects_unadvertised_recipe_alias() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_recipe_save",
                "arguments":{
                    "recipe":{
                        "schema_version":2,
                        "name":"legacy-alias",
                        "steps":[]
                    }
                }
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        let error = result["structuredContent"]["error"].as_str().unwrap();
        assert!(error.contains("sootie_recipe_save does not accept argument(s): recipe"));
    }

    #[test]
    fn recipe_save_rejects_recipe_json_object() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name":"sootie_recipe_save",
                "arguments":{
                    "recipe_json":{
                        "schema_version":2,
                        "name":"object-recipe",
                        "steps":[]
                    }
                }
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("sootie_recipe_save.recipe_json must be a string"));
    }

    #[test]
    fn runs_v2_action_recipe() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 2,
            "name": "fake-hotkey",
            "app": "Fake",
            "params": {
                "target_url": { "type": "string", "required": true }
            },
            "steps": [
                {
                    "id": 1,
                    "action": "hotkey",
                    "params": { "keys": "cmd,l" }
                },
                {
                    "id": 2,
                    "action": "type",
                    "params": { "text": "{{target_url}}" },
                    "wait_after": { "condition": "delay", "timeout": 0.0 }
                }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": {
                    "recipe": "fake-hotkey",
                    "params": { "target_url": "https://example.com" }
                }
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["success"], true);
        assert_eq!(
            result["structuredContent"]["data"]["steps"][1]["wait_after"]["data"]["matched"],
            true
        );
    }

    #[test]
    fn recipe_run_suppresses_per_action_context_snapshots() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "fast-click-recipe",
            "app": "Fake",
            "steps": [{
                "id": 1,
                "action": "click",
                "target": { "coordinate": { "x": 41.0, "y": 62.0 } }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "fast-click-recipe" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["structuredContent"]["success"], true);
        let step = &result["structuredContent"]["data"]["steps"][0];
        assert_eq!(step["data"]["x"], 41.0);
        assert_eq!(step["context"], Value::Null);
    }

    #[test]
    fn recipe_run_sets_clipboard_before_paste_hotkey() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "paste-svg",
            "app": "Safari",
            "steps": [
                { "id": 1, "action": "set_clipboard", "text": "<svg/>" },
                { "id": 2, "action": "hotkey", "keys": ["cmd", "v"] }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let events = Arc::new(Mutex::new(Vec::new()));
        let backend = RecordingBackend {
            events: events.clone(),
            fail_focus: false,
            record_context: false,
            record_screenshot: false,
        };
        let mut server = McpServer::with_recipe_store(Box::new(backend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "paste-svg" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["success"], true);
        assert_eq!(
            result["structuredContent"]["data"]["steps"][0]["tool"],
            "__set_clipboard"
        );
        assert_eq!(
            events.lock().unwrap().as_slice(),
            &["clipboard:<svg/>".to_string()]
        );
    }

    #[test]
    fn recipe_run_remaps_window_relative_coordinates_to_current_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "window-relative-click",
            "steps": [{
                "id": 1,
                "action": "click",
                "target": {
                    "app": { "name": "Fake" },
                    "window": "Main",
                    "window_coordinate": { "x": 40.0, "y": 90.0 }
                }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "window-relative-click" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        let data = &result["structuredContent"]["data"]["steps"][0]["data"];
        assert_eq!(data["x"], 41.0);
        assert_eq!(data["y"], 92.0);
    }

    #[test]
    fn recipe_run_remaps_window_normalized_coordinates_to_current_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "window-normalized-drag",
            "steps": [{
                "id": 1,
                "action": "drag",
                "target": {
                    "app": { "name": "Fake" },
                    "window": "Main",
                    "window_normalized_coordinate": { "x": 0.25, "y": 0.5 }
                },
                "to_target": {
                    "window": "Main",
                    "window_normalized_coordinate": { "x": 0.75, "y": 0.8 }
                }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "window-normalized-drag" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        let data = &result["structuredContent"]["data"]["steps"][0]["data"];
        assert_eq!(data["from"][0], 201.0);
        assert_eq!(data["from"][1], 302.0);
        assert_eq!(data["to"][0], 601.0);
        assert_eq!(data["to"][1], 482.0);
    }

    #[test]
    fn recipe_run_prefers_semantic_target_before_coordinate_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "semantic-first-click",
            "steps": [{
                "id": 1,
                "action": "click",
                "target": {
                    "app": { "name": "Fake" },
                    "window": "Main",
                    "dom_id": "submit-button",
                    "window_coordinate": { "x": 40.0, "y": 90.0 }
                }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "semantic-first-click" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(
            result["isError"],
            false,
            "{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
        let step = &result["structuredContent"]["data"]["steps"][0];
        let data = &step["data"];
        assert_eq!(step["fallback_used"], Value::Null);
        assert_eq!(data["x"], Value::Null);
        assert_eq!(data["y"], Value::Null);
    }

    #[test]
    fn recipe_run_falls_back_to_window_coordinate_when_semantic_target_fails() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "semantic-fallback-click",
            "steps": [{
                "id": 1,
                "action": "click",
                "target": {
                    "app": { "name": "Fake" },
                    "window": "Main",
                    "dom_id": "missing-button",
                    "window_coordinate": { "x": 40.0, "y": 90.0 }
                }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(VisionOnlyBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "semantic-fallback-click" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(
            result["isError"],
            false,
            "{}",
            serde_json::to_string_pretty(&result).unwrap()
        );
        let step = &result["structuredContent"]["data"]["steps"][0];
        let data = &step["data"];
        assert_eq!(step["fallback_used"], true);
        assert_eq!(data["x"], 50.0);
        assert_eq!(data["y"], 110.0);
    }

    #[test]
    fn recipe_run_does_not_remap_explicit_window_coordinates_to_another_window() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "missing-window-click",
            "steps": [{
                "id": 1,
                "action": "click",
                "target": {
                    "app": { "name": "Fake" },
                    "window": "Missing",
                    "window_coordinate": { "x": 40.0, "y": 90.0 }
                }
            }]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "missing-window-click" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("no window bounds available"));
    }

    #[test]
    fn recipe_failure_uses_failed_step_suggestion() {
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "visibility-gated",
            "steps": [
                { "tool": "sootie_screenshot", "args": {} }
            ]
        }))
        .unwrap();
        let failed_step = json!({
            "id": 1,
            "tool": "sootie_screenshot",
            "success": false,
            "error": "platform error: macOS screen is locked",
            "suggestion": "Unlock the Mac, verify the target window is visible, then retry."
        });
        let result = recipe_failed_result(&recipe, vec![failed_step.clone()], failed_step);
        assert!(!result.success);
        assert_eq!(
            result.suggestion.as_deref(),
            Some("Unlock the Mac, verify the target window is visible, then retry.")
        );
        let data = result.data.as_ref().unwrap();
        assert_eq!(data["steps_completed"], 0);
        assert_eq!(data["steps_attempted"], 1);
    }

    #[test]
    fn locked_screen_recipe_preflight_blocks_ui_recipes() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "locked-ui",
            "steps": [
                { "tool": "sootie_window", "args": { "app": "Safari", "action": "focus" } },
                { "tool": "sootie_click", "args": { "x": 10, "y": 20 } }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(LockedFakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "locked-ui" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("requires an unlocked macOS screen"));
        assert!(result["structuredContent"]["suggestion"]
            .as_str()
            .unwrap()
            .contains("Unlock the Mac"));
        assert_eq!(result["structuredContent"]["data"]["locked"], true);
        assert_eq!(
            result["structuredContent"]["data"]["blocked_steps"][0]["tool"],
            "sootie_window"
        );
        assert_eq!(
            result["structuredContent"]["data"]["blocked_steps"][0]["step_index"],
            0
        );
        assert_eq!(
            result["structuredContent"]["data"]["blocked_steps"][1]["tool"],
            "sootie_click"
        );
    }

    #[test]
    fn locked_screen_recipe_preflight_allows_read_only_recipes() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "locked-readonly",
            "steps": [
                { "tool": "sootie_context", "args": { "app": "Safari" } }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(LockedFakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "locked-readonly" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["success"], true);
        assert_eq!(result["structuredContent"]["data"]["steps_completed"], 1);
    }

    #[test]
    fn recipe_app_precondition_reports_inaccessible_desktop() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 2,
            "name": "missing-app",
            "app": "MissingApp",
            "preconditions": {
                "app_running": "MissingApp"
            },
            "steps": [
                { "id": 1, "action": "screenshot" }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "missing-app" }
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("not accessible"));
        assert!(result["structuredContent"]["suggestion"]
            .as_str()
            .unwrap()
            .contains("No accessible window"));
    }

    #[test]
    fn recipe_url_precondition_uses_app_running_when_recipe_app_is_absent() {
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "browser-recipe",
            "preconditions": {
                "app_running": "Safari",
                "url_contains": "excalidraw.com"
            },
            "steps": []
        }))
        .unwrap();
        let preconditions = recipe.preconditions.as_ref().unwrap();
        assert_eq!(
            recipe_url_precondition_app(&recipe, preconditions),
            Some("Safari")
        );

        let explicit = parse_recipe(&json!({
            "schema_version": 1,
            "name": "browser-recipe",
            "app": "Google Chrome",
            "preconditions": {
                "app_running": "Safari",
                "url_contains": "excalidraw.com"
            },
            "steps": []
        }))
        .unwrap();
        let explicit_preconditions = explicit.preconditions.as_ref().unwrap();
        assert_eq!(
            recipe_url_precondition_app(&explicit, explicit_preconditions),
            Some("Google Chrome")
        );
    }

    #[test]
    fn recipe_url_precondition_uses_lightweight_browser_url_probe() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let server = McpServer::with_recipe_store(Box::new(FakeBackend), store);

        let context = server
            .recipe_url_precondition_context(Some("Safari"), "excalidraw.com")
            .unwrap();

        assert_eq!(context.app.as_deref(), Some("Safari"));
        assert_eq!(context.url.as_deref(), Some("https://excalidraw.com/"));
        assert!(context.interactive_elements.is_empty());
    }

    #[test]
    fn matching_browser_url_precondition_can_satisfy_app_running_check() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "url-only-browser",
            "preconditions": {
                "app_running": "UrlOnlyBrowser",
                "url_contains": "excalidraw.com"
            },
            "steps": []
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "url-only-browser" }
            }),
        });

        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["success"], true);
    }

    #[test]
    fn recipe_success_surfaces_last_screenshot_artifact() {
        let recipe = parse_recipe(&json!({
            "schema_version": 1,
            "name": "capture-after-draw",
            "steps": [
                { "tool": "sootie_click", "args": {} },
                { "tool": "sootie_screenshot", "args": {} }
            ]
        }))
        .unwrap();
        let payload = recipe_success_payload(
            &recipe,
            vec![
                json!({
                    "id": 1,
                    "tool": "sootie_click",
                    "success": true,
                    "data": {}
                }),
                json!({
                    "id": 2,
                    "tool": "sootie_screenshot",
                    "success": true,
                    "data": {
                        "artifact_path": "/tmp/sootie-artifacts/final.png",
                        "artifact_uri": "file:///tmp/sootie-artifacts/final.png",
                        "width": 800,
                        "height": 600,
                        "window_title": "Excalidraw Whiteboard",
                        "mime_type": "image/png"
                    }
                }),
            ],
        );
        assert_eq!(payload["steps_completed"], 2);
        assert_eq!(payload["last_screenshot"]["step_id"], 2);
        assert_eq!(
            payload["last_screenshot"]["artifact_path"],
            "/tmp/sootie-artifacts/final.png"
        );
        assert_eq!(payload["last_screenshot"]["width"], 800);
        assert_eq!(
            payload["last_screenshot"]["window_title"],
            "Excalidraw Whiteboard"
        );
    }

    #[test]
    fn url_precondition_accepts_domain_from_accessibility_when_url_is_empty() {
        let context = ContextSnapshot {
            app: Some("Safari".into()),
            app_id: Some("Safari".into()),
            platform_app_id: None,
            bundle_id: None,
            pid: Some(42),
            window: Some("Excalidraw Whiteboard".into()),
            url: Some(String::new()),
            focused_element: None,
            interactive_elements: vec![ElementInfo {
                id: Some("ShowPerSitePreferencesMenuItem".into()),
                role: "AXMenuItem".into(),
                title: Some("Settings for excalidraw.com...".into()),
                name: Some("Settings for excalidraw.com...".into()),
                text: None,
                bounds: None,
                actions: vec!["click".into()],
                editable: Some(false),
                enabled: Some(false),
            }],
        };
        assert!(recipe_context_matches_url_precondition(
            &context,
            "excalidraw.com"
        ));
    }

    #[test]
    fn url_precondition_rejects_nonempty_wrong_url() {
        let context = ContextSnapshot {
            app: Some("Safari".into()),
            app_id: Some("Safari".into()),
            platform_app_id: None,
            bundle_id: None,
            pid: Some(42),
            window: Some("Excalidraw Whiteboard".into()),
            url: Some("https://example.com/".into()),
            focused_element: None,
            interactive_elements: vec![ElementInfo {
                id: Some("ShowPerSitePreferencesMenuItem".into()),
                role: "AXMenuItem".into(),
                title: Some("Settings for excalidraw.com...".into()),
                name: Some("Settings for excalidraw.com...".into()),
                text: None,
                bounds: None,
                actions: vec!["click".into()],
                editable: Some(false),
                enabled: Some(false),
            }],
        };
        assert!(!recipe_context_matches_url_precondition(
            &context,
            "excalidraw.com"
        ));
    }

    #[test]
    fn runs_legacy_wait_step_as_delay() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = parse_recipe(&json!({
            "schema_version": 3,
            "name": "legacy-delay",
            "params": [],
            "steps": [
                {
                    "action": "wait",
                    "timeout": 0,
                    "params": null
                }
            ]
        }))
        .unwrap();
        store.save(&recipe).unwrap();

        let mut server = McpServer::with_recipe_store(Box::new(FakeBackend), store);
        let response = server.handle_request(JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "tools/call".into(),
            params: json!({
                "name": "sootie_run",
                "arguments": { "recipe": "legacy-delay" }
            }),
        });
        let result = response.result.unwrap();
        assert_eq!(result["isError"], false);
        assert_eq!(result["structuredContent"]["success"], true);
        assert_eq!(
            result["structuredContent"]["data"]["steps"][0]["data"]["delay_seconds"],
            0.0
        );
        assert!(result["structuredContent"]["data"]["steps"][0]["duration_ms"].is_number());
    }
}
