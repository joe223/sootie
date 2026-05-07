use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tracing::{error, info, warn};

use sootie_core::action::{
    ActionProvider, ActionTarget, ClickAction, DragAction, FocusAction, HotkeyAction, HoverAction,
    LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction, WindowOperation,
};
use sootie_core::logging::{
    create_duration_ms, sanitize_tool_call_args, LogConfig, SootieLogger, ToolCallLog,
};
use sootie_core::perception::{PerceptionProvider, WaitCondition};
use sootie_core::recipe::{Recipe, RecipeEngine, StepTarget};
use sootie_core::selector::AppSelector;

use crate::tools::{
    all_tools, parse_action_target, parse_mouse_button, parse_mouse_button_strict,
    parse_optional_action_target, parse_scroll_direction, parse_scroll_direction_strict,
    parse_selector_from_args_strict, parse_step_target, selector_field_keys_present,
    validate_action_selector, validate_query_selector,
};
use crate::types::{
    CallToolRequest, CallToolResult, InitializeResult, JsonRpcRequest, JsonRpcResponse,
    ListToolsResult, ServerCapabilities, ServerInfo, ToolContent, ToolsCapability,
};

#[derive(Debug)]
struct ToolInvocationError {
    code: &'static str,
    message: String,
    details: Option<serde_json::Value>,
}

impl ToolInvocationError {
    fn invalid_arguments(message: impl Into<String>) -> Self {
        Self {
            code: "invalid_arguments",
            message: message.into(),
            details: None,
        }
    }

    fn execution(message: impl Into<String>) -> Self {
        Self {
            code: "execution_failed",
            message: message.into(),
            details: None,
        }
    }
}

fn to_json_value<T: serde::Serialize>(value: T) -> Result<serde_json::Value, ToolInvocationError> {
    serde_json::to_value(value).map_err(|e| ToolInvocationError::execution(e.to_string()))
}

fn present_tool_data(value: &serde_json::Value) -> Result<String, ToolInvocationError> {
    match value {
        serde_json::Value::String(text) => Ok(text.clone()),
        _ => serde_json::to_string_pretty(value)
            .map_err(|e| ToolInvocationError::execution(e.to_string())),
    }
}

fn tool_compatibility_warnings(name: &str, args: &serde_json::Value) -> Vec<serde_json::Value> {
    let uses_legacy_top_level_target = args.get("target").is_none()
        && (args.get("coordinate").is_some() || selector_field_keys_present(args));

    match name {
        "sootie_click" | "sootie_type" | "sootie_hover" | "sootie_scroll"
            if uses_legacy_top_level_target =>
        {
            vec![serde_json::json!({
                "code": "legacy_argument_shape",
                "message": "Top-level selector and coordinate fields are deprecated; use the canonical target object."
            })]
        }
        _ => vec![],
    }
}

pub struct SootieServer {
    perception: Arc<Box<dyn PerceptionProvider>>,
    action: Arc<Box<dyn ActionProvider>>,
    recipe_engine: Arc<Mutex<RecipeEngine>>,
    logger: SootieLogger,
}

impl SootieServer {
    pub fn new(perception: Box<dyn PerceptionProvider>, action: Box<dyn ActionProvider>) -> Self {
        Self {
            perception: Arc::new(perception),
            action: Arc::new(action),
            recipe_engine: Arc::new(Mutex::new(RecipeEngine::new())),
            logger: SootieLogger::new(LogConfig::default()),
        }
    }

    pub fn new_in_memory(
        perception: Box<dyn PerceptionProvider>,
        action: Box<dyn ActionProvider>,
    ) -> Self {
        Self {
            perception: Arc::new(perception),
            action: Arc::new(action),
            recipe_engine: Arc::new(Mutex::new(RecipeEngine::new_in_memory())),
            logger: SootieLogger::new(LogConfig::default()),
        }
    }

    pub fn new_with_recipe_storage_dir(
        perception: Box<dyn PerceptionProvider>,
        action: Box<dyn ActionProvider>,
        recipe_storage_dir: Option<PathBuf>,
    ) -> Self {
        let recipe_engine = recipe_storage_dir
            .map(RecipeEngine::new_with_storage_dir)
            .unwrap_or_else(RecipeEngine::new_in_memory);

        Self {
            perception: Arc::new(perception),
            action: Arc::new(action),
            recipe_engine: Arc::new(Mutex::new(recipe_engine)),
            logger: SootieLogger::new(LogConfig::default()),
        }
    }

    pub async fn handle_request(&self, request: JsonRpcRequest) -> JsonRpcResponse {
        let id = request.id.clone();
        let start = Instant::now();
        self.logger.log_mcp_request(&request.method, &id);

        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(),
            "tools/list" => self.handle_list_tools(),
            "tools/call" => {
                if let Some(params) = request.params {
                    self.handle_tool_call(params).await
                } else {
                    Err((-32602, "Missing params for tools/call".to_string()))
                }
            }
            "notifications/initialized" => {
                info!("Client initialized notification received");
                return JsonRpcResponse::success(id, serde_json::json!(null));
            }
            _ => {
                warn!(method = %request.method, "Unknown MCP method");
                Err((-32601, format!("Unknown method: {}", request.method)))
            }
        };

        let duration = start.elapsed();
        match result {
            Ok(value) => {
                self.logger
                    .log_mcp_response(&request.method, true, duration);
                JsonRpcResponse::success(id, value)
            }
            Err((code, msg)) => {
                error!(method = %request.method, error = %msg, "MCP request failed");
                self.logger
                    .log_mcp_response(&request.method, false, duration);
                JsonRpcResponse::error(id, code, msg)
            }
        }
    }

    fn handle_initialize(&self) -> Result<serde_json::Value, (i64, String)> {
        let result = InitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {
                    list_changed: false,
                }),
            },
            server_info: ServerInfo {
                name: "sootie".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        serde_json::to_value(result).map_err(|e| (-32603, e.to_string()))
    }

    fn handle_list_tools(&self) -> Result<serde_json::Value, (i64, String)> {
        let result = ListToolsResult { tools: all_tools() };
        serde_json::to_value(result).map_err(|e| (-32603, e.to_string()))
    }

    async fn handle_tool_call(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, (i64, String)> {
        let start = Instant::now();

        let request: CallToolRequest = serde_json::from_value(params.clone())
            .map_err(|e| (-32602, format!("Invalid tools/call params: {}", e)))?;
        let name = request.name.as_str();
        let args = request.arguments.unwrap_or(serde_json::json!({}));

        let request_id = params.get("id").cloned();
        info!(tool = %name, args = %args, "Tool call started");

        let result = match name {
            "sootie_context" => self.tool_context().await,
            "sootie_find" => self.tool_find(&args).await,
            "sootie_inspect" => self.tool_inspect(&args).await,
            "sootie_wait" => self.tool_wait(&args).await,
            "sootie_screenshot" => self.tool_screenshot(&args).await,
            "sootie_find_apps" => self.tool_find_apps(&args).await,
            "sootie_click" => self.tool_click(&args).await,
            "sootie_type" => self.tool_type(&args).await,
            "sootie_press" => self.tool_press(&args).await,
            "sootie_hotkey" => self.tool_hotkey(&args).await,
            "sootie_scroll" => self.tool_scroll(&args).await,
            "sootie_hover" => self.tool_hover(&args).await,
            "sootie_drag" => self.tool_drag(&args).await,
            "sootie_launch" => self.tool_launch(&args).await,
            "sootie_focus" => self.tool_focus(&args).await,
            "sootie_window" => self.tool_window(&args).await,
            "sootie_recipes" => self.tool_recipes().await,
            "sootie_run" => self.tool_run(&args).await,
            "sootie_recipe_save" => self.tool_recipe_save(&args).await,
            "sootie_recipe_delete" => self.tool_recipe_delete(&args).await,
            _ => {
                warn!(tool = %name, "Unknown tool requested");
                return Err((-32601, format!("Unknown tool: {}", name)));
            }
        };

        let duration_ms = create_duration_ms(start);

        match result {
            Ok(value) => {
                let sanitized_args = sanitize_tool_call_args(&args, self.logger.config());
                let content_text = present_tool_data(&value).map_err(|e| (-32603, e.message))?;
                let warnings = tool_compatibility_warnings(name, &args);

                self.logger.log_tool_call(&ToolCallLog {
                    tool_name: name.to_string(),
                    request_id,
                    arguments: sanitized_args,
                    success: true,
                    error_message: None,
                    duration_ms,
                    backend_used: None,
                });

                let call_result = CallToolResult {
                    content: vec![ToolContent::text(&content_text)],
                    is_error: None,
                    structured_content: Some(serde_json::json!({
                        "ok": true,
                        "data": value,
                        "warnings": warnings
                    })),
                };
                serde_json::to_value(call_result).map_err(|e| (-32603, e.to_string()))
            }
            Err(err) => {
                let sanitized_args = sanitize_tool_call_args(&args, self.logger.config());

                self.logger.log_tool_call(&ToolCallLog {
                    tool_name: name.to_string(),
                    request_id,
                    arguments: sanitized_args,
                    success: false,
                    error_message: Some(err.message.clone()),
                    duration_ms,
                    backend_used: None,
                });

                let call_result = CallToolResult {
                    content: vec![ToolContent::text(&err.message)],
                    is_error: Some(true),
                    structured_content: Some(serde_json::json!({
                        "ok": false,
                        "error": {
                            "code": err.code,
                            "message": err.message,
                            "details": err.details,
                        },
                        "warnings": []
                    })),
                };
                serde_json::to_value(call_result).map_err(|e| (-32603, e.to_string()))
            }
        }
    }

    async fn tool_context(&self) -> Result<serde_json::Value, ToolInvocationError> {
        let ctx =
            self.perception.get_context().await.map_err(|e| {
                ToolInvocationError::execution(format!("Failed to get context: {}", e))
            })?;
        to_json_value(ctx)
    }

    async fn tool_find(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let selector = parse_selector_from_args_strict(args)
            .map_err(ToolInvocationError::invalid_arguments)?;
        validate_query_selector(&selector).map_err(ToolInvocationError::invalid_arguments)?;
        let result = self
            .perception
            .find(&selector)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Find failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_inspect(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let selector = parse_selector_from_args_strict(args)
            .map_err(ToolInvocationError::invalid_arguments)?;
        validate_query_selector(&selector).map_err(ToolInvocationError::invalid_arguments)?;
        let result = self
            .perception
            .inspect(&selector)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Inspect failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_wait(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let selector = parse_selector_from_args_strict(args)
            .map_err(ToolInvocationError::invalid_arguments)?;
        validate_query_selector(&selector).map_err(ToolInvocationError::invalid_arguments)?;
        let timeout = match args.get("timeout") {
            Some(value) => value.as_u64().ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Timeout must be a non-negative integer")
            })?,
            None => 5000,
        };

        let state = args
            .get("state")
            .and_then(|v| {
                serde_json::from_value::<HashMap<String, serde_json::Value>>(v.clone()).ok()
            })
            .unwrap_or_default();

        let condition = WaitCondition {
            state,
            timeout_ms: timeout,
        };

        let result = self
            .perception
            .wait(&selector, &condition)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Wait failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_screenshot(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let selector = if args.get("app").is_some() || args.get("window").is_some() {
            Some(
                parse_selector_from_args_strict(args)
                    .map_err(ToolInvocationError::invalid_arguments)?,
            )
        } else {
            None
        };

        let region = args
            .get("region")
            .map(|r| {
                let x = r.get("x").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Screenshot region requires numeric x")
                })?;
                let y = r.get("y").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Screenshot region requires numeric y")
                })?;
                let width = r.get("width").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments(
                        "Screenshot region requires numeric width",
                    )
                })?;
                let height = r.get("height").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments(
                        "Screenshot region requires numeric height",
                    )
                })?;

                Ok(sootie_core::selector::Bounds {
                    x,
                    y,
                    width,
                    height,
                })
            })
            .transpose()?;

        let display_id = args
            .get("display_id")
            .and_then(|d| d.as_u64())
            .map(|d| d as u32);

        let result = self
            .perception
            .screenshot(selector.as_ref(), region.as_ref(), display_id)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Screenshot failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_find_apps(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: pattern")
            })?;

        let limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as u32);

        let result = self
            .perception
            .find_apps(pattern, limit)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Find apps failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_click(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let target = parse_action_target(args).map_err(ToolInvocationError::invalid_arguments)?;

        if let ActionTarget::Selector(selector) = &target {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }

        let button = args
            .get("button")
            .and_then(|v| v.as_str())
            .map(parse_mouse_button_strict)
            .transpose()
            .map_err(ToolInvocationError::invalid_arguments)?;

        let count = args.get("count").and_then(|v| v.as_u64()).map(|v| v as u32);

        let action = ClickAction {
            target,
            button,
            count,
        };

        let result = self
            .action
            .click(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Click failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_type(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: text"))?
            .to_string();

        let target =
            parse_optional_action_target(args).map_err(ToolInvocationError::invalid_arguments)?;

        if let Some(ActionTarget::Selector(selector)) = &target {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }

        let clear_first = args.get("clear_first").and_then(|v| v.as_bool());

        let action = TypeAction {
            target,
            text,
            clear_first,
        };

        let result = self
            .action
            .r#type(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Type failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_press(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: key"))?
            .to_string();

        let action = PressAction { key };
        let result = self
            .action
            .press(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Press failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_hotkey(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let keys = args
            .get("keys")
            .cloned()
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: keys"))
            .and_then(|value| {
                serde_json::from_value::<Vec<String>>(value).map_err(|e| {
                    ToolInvocationError::invalid_arguments(format!("Invalid keys: {}", e))
                })
            })?;

        if keys.is_empty() {
            return Err(ToolInvocationError::invalid_arguments(
                "Hotkey requires at least one key",
            ));
        }

        let action = HotkeyAction { keys };
        let result = self
            .action
            .hotkey(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Hotkey failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_scroll(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: direction")
            })?
            .to_string();

        let target =
            parse_optional_action_target(args).map_err(ToolInvocationError::invalid_arguments)?;

        if let Some(ActionTarget::Selector(selector)) = &target {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }
        let amount = args
            .get("amount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let action = ScrollAction {
            target,
            direction: parse_scroll_direction_strict(&direction)
                .map_err(ToolInvocationError::invalid_arguments)?,
            amount,
        };

        let result = self
            .action
            .scroll(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Scroll failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_hover(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let target = parse_action_target(args).map_err(ToolInvocationError::invalid_arguments)?;

        if let ActionTarget::Selector(selector) = &target {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }

        let action = HoverAction { target };
        let result = self
            .action
            .hover(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Hover failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_drag(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let from_val = args.get("from").ok_or_else(|| {
            ToolInvocationError::invalid_arguments("Missing required field: from")
        })?;
        let to_val = args
            .get("to")
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: to"))?;

        let from = parse_step_target(from_val)
            .map(|st| match st {
                StepTarget::Coordinate(c) => sootie_core::action::ActionTarget::Coordinate(c),
                StepTarget::Selector(s) => sootie_core::action::ActionTarget::Selector(s),
            })
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Invalid 'from' target"))?;

        let to = parse_step_target(to_val)
            .map(|st| match st {
                StepTarget::Coordinate(c) => sootie_core::action::ActionTarget::Coordinate(c),
                StepTarget::Selector(s) => sootie_core::action::ActionTarget::Selector(s),
            })
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Invalid 'to' target"))?;

        if let ActionTarget::Selector(selector) = &from {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }

        if let ActionTarget::Selector(selector) = &to {
            validate_action_selector(selector).map_err(ToolInvocationError::invalid_arguments)?;
        }

        let action = DragAction { from, to };
        let result = self
            .action
            .drag(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Drag failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_focus(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        if args.get("app").is_none() {
            return Err(ToolInvocationError::invalid_arguments(
                "Missing required field: app. Must specify which application to focus",
            ));
        }

        let selector = parse_selector_from_args_strict(args)
            .map_err(ToolInvocationError::invalid_arguments)?;
        let action = FocusAction { selector };
        let result = self
            .action
            .focus(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Focus failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_launch(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let app = args
            .get("app")
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: app"))?;

        let app_selector = if let Some(s) = app.as_str() {
            AppSelector::from_name(s)
        } else {
            serde_json::from_value::<AppSelector>(app.clone()).map_err(|e| {
                ToolInvocationError::invalid_arguments(format!("Invalid app selector: {}", e))
            })?
        };

        let args_list: Vec<String> = match args.get("args") {
            Some(value) => serde_json::from_value::<Vec<String>>(value.clone()).map_err(|e| {
                ToolInvocationError::invalid_arguments(format!("Invalid launch args: {}", e))
            })?,
            None => Vec::new(),
        };

        let action = LaunchAction {
            app: app_selector,
            args: args_list,
        };

        let result = self
            .action
            .launch(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Launch failed: {}", e)))?;
        to_json_value(result)
    }

    async fn tool_window(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        if args.get("app").is_none() {
            return Err(ToolInvocationError::invalid_arguments(
            "Missing required field: app. Must specify which application's window to operate on",
        ));
        }

        let selector = parse_selector_from_args_strict(args)
            .map_err(ToolInvocationError::invalid_arguments)?;
        let op_str = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: operation")
            })?;

        let operation = match op_str {
            "minimize" => WindowOperation::Minimize,
            "maximize" => WindowOperation::Maximize,
            "close" => WindowOperation::Close,
            "move" => {
                let x = args.get("x").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Window move requires numeric x")
                })?;
                let y = args.get("y").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Window move requires numeric y")
                })?;
                WindowOperation::Move { x, y }
            }
            "resize" => {
                let width = args.get("width").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Window resize requires numeric width")
                })?;
                let height = args.get("height").and_then(|v| v.as_f64()).ok_or_else(|| {
                    ToolInvocationError::invalid_arguments("Window resize requires numeric height")
                })?;
                WindowOperation::Resize { width, height }
            }
            _ => {
                return Err(ToolInvocationError::invalid_arguments(format!(
                    "Unknown window operation: {}",
                    op_str
                )))
            }
        };

        let action = WindowAction {
            selector,
            operation,
        };
        let result = self.action.window_op(&action).await.map_err(|e| {
            ToolInvocationError::execution(format!("Window operation failed: {}", e))
        })?;
        to_json_value(result)
    }

    async fn tool_recipes(&self) -> Result<serde_json::Value, ToolInvocationError> {
        let engine = self.recipe_engine.lock().await;
        let recipes: Vec<&Recipe> = engine.list();
        let names: Vec<&str> = recipes.iter().map(|r| r.name.as_str()).collect();
        to_json_value(names)
    }

    async fn tool_run(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolInvocationError::invalid_arguments("Missing required field: name")
        })?;

        let params: HashMap<String, serde_json::Value> = match args.get("params") {
            Some(value) => serde_json::from_value(value.clone()).map_err(|e| {
                ToolInvocationError::invalid_arguments(format!("Invalid params: {}", e))
            })?,
            None => HashMap::new(),
        };

        let (recipe, substituted_steps) = {
            let engine = self.recipe_engine.lock().await;
            let recipe = engine
                .get(name)
                .ok_or_else(|| {
                    ToolInvocationError::execution(format!("Recipe not found: {}", name))
                })?
                .clone();

            let resolved_params = engine.resolve_params(&recipe, &params).map_err(|e| {
                ToolInvocationError::invalid_arguments(format!("Parameter error: {}", e))
            })?;

            let steps = recipe
                .steps
                .iter()
                .map(|step| engine.substitute_step(step, &resolved_params))
                .collect::<Vec<_>>();

            (recipe, steps)
        };

        let mut results = Vec::with_capacity(substituted_steps.len());
        for (index, step) in substituted_steps.iter().enumerate() {
            let result = self.execute_recipe_step(index, step).await.map_err(|e| {
                ToolInvocationError::execution(format!("Recipe execution failed: {}", e))
            })?;
            results.push(result);
        }

        Ok(serde_json::json!({
            "recipe": recipe.name,
            "status": "completed",
            "results": results,
        }))
    }

    async fn tool_recipe_save(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let recipe_val = args.get("recipe").ok_or_else(|| {
            ToolInvocationError::invalid_arguments("Missing required field: recipe")
        })?;

        let recipe: Recipe = serde_json::from_value(recipe_val.clone()).map_err(|e| {
            ToolInvocationError::invalid_arguments(format!("Invalid recipe: {}", e))
        })?;

        let mut engine = self.recipe_engine.lock().await;
        engine
            .load(recipe.clone())
            .map_err(|e| ToolInvocationError::execution(format!("Failed to save recipe: {}", e)))?;

        Ok(serde_json::json!({
            "status": "saved",
            "name": recipe.name
        }))
    }

    async fn tool_recipe_delete(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let name = args.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolInvocationError::invalid_arguments("Missing required field: name")
        })?;

        let mut engine = self.recipe_engine.lock().await;
        engine.delete(name).map_err(|e| {
            ToolInvocationError::execution(format!("Failed to delete recipe: {}", e))
        })?;

        Ok(serde_json::json!({
            "status": "deleted",
            "name": name
        }))
    }

    async fn execute_recipe_step(
        &self,
        index: usize,
        step: &sootie_core::recipe::RecipeStep,
    ) -> Result<serde_json::Value, sootie_core::recipe::RecipeError> {
        use sootie_core::recipe::RecipeError;

        let result = match step.action.as_str() {
            "click" => {
                let target = step
                    .target
                    .as_ref()
                    .map(step_target_to_action_target)
                    .ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "click requires target".to_string(),
                    })?;

                let action = ClickAction {
                    target,
                    button: step.button.as_deref().map(parse_mouse_button),
                    count: step.count,
                };
                let result =
                    self.action
                        .click(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "type" => {
                let action = TypeAction {
                    target: step.target.as_ref().map(step_target_to_action_target),
                    text: step.text.clone().ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "type requires text".to_string(),
                    })?,
                    clear_first: None,
                };
                let result =
                    self.action
                        .r#type(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "press" => {
                let action = PressAction {
                    key: step.key.clone().ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "press requires key".to_string(),
                    })?,
                };
                let result =
                    self.action
                        .press(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "hotkey" => {
                let action = HotkeyAction {
                    keys: step.keys.clone().ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "hotkey requires keys".to_string(),
                    })?,
                };
                let result =
                    self.action
                        .hotkey(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "scroll" => {
                let direction = step
                    .direction
                    .clone()
                    .ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "scroll requires direction".to_string(),
                    })?;
                let action = ScrollAction {
                    target: step.target.as_ref().map(step_target_to_action_target),
                    direction: parse_scroll_direction(&direction),
                    amount: step.amount,
                };
                let result =
                    self.action
                        .scroll(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "hover" => {
                let target = step
                    .target
                    .as_ref()
                    .map(step_target_to_action_target)
                    .ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "hover requires target".to_string(),
                    })?;
                let action = HoverAction { target };
                let result =
                    self.action
                        .hover(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "drag" => {
                let from = step
                    .target
                    .as_ref()
                    .map(step_target_to_action_target)
                    .ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "drag requires target".to_string(),
                    })?;
                let to = step
                    .to_target
                    .as_ref()
                    .map(step_target_to_action_target)
                    .ok_or_else(|| RecipeError::StepFailed {
                        step: index,
                        error: "drag requires to_target".to_string(),
                    })?;
                let action = DragAction { from, to };
                let result =
                    self.action
                        .drag(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "focus" => {
                let selector = step_target_to_selector(step.target.as_ref()).ok_or_else(|| {
                    RecipeError::StepFailed {
                        step: index,
                        error: "focus requires selector target".to_string(),
                    }
                })?;
                let action = FocusAction { selector };
                let result =
                    self.action
                        .focus(&action)
                        .await
                        .map_err(|e| RecipeError::StepFailed {
                            step: index,
                            error: e.to_string(),
                        })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "wait" => {
                let selector = step_target_to_selector(step.target.as_ref()).ok_or_else(|| {
                    RecipeError::StepFailed {
                        step: index,
                        error: "wait requires selector target".to_string(),
                    }
                })?;
                let condition = wait_condition_from_selector(&selector, step.timeout);
                let result = self
                    .perception
                    .wait(&selector, &condition)
                    .await
                    .map_err(|e| RecipeError::StepFailed {
                        step: index,
                        error: e.to_string(),
                    })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            "screenshot" => {
                let selector = step_target_to_selector(step.target.as_ref());
                let result = self
                    .perception
                    .screenshot(selector.as_ref(), None, None)
                    .await
                    .map_err(|e| RecipeError::StepFailed {
                        step: index,
                        error: e.to_string(),
                    })?;
                serde_json::to_value(result).map_err(|e| RecipeError::StepFailed {
                    step: index,
                    error: e.to_string(),
                })?
            }
            other => {
                return Err(RecipeError::StepFailed {
                    step: index,
                    error: format!("unsupported action: {}", other),
                });
            }
        };

        Ok(serde_json::json!({
            "step": index,
            "action": step.action,
            "result": result,
        }))
    }
}

fn step_target_to_action_target(target: &StepTarget) -> ActionTarget {
    match target {
        StepTarget::Coordinate(coord) => ActionTarget::Coordinate(coord.clone()),
        StepTarget::Selector(selector) => ActionTarget::Selector(selector.clone()),
    }
}

fn step_target_to_selector(target: Option<&StepTarget>) -> Option<sootie_core::selector::Selector> {
    match target {
        Some(StepTarget::Selector(selector)) => Some(selector.clone()),
        _ => None,
    }
}

fn wait_condition_from_selector(
    selector: &sootie_core::selector::Selector,
    timeout: Option<u64>,
) -> WaitCondition {
    let mut state = HashMap::new();
    if let Some(selector_state) = selector.element.state.as_ref() {
        if let Some(visible) = selector_state.visible {
            state.insert("visible".to_string(), serde_json::Value::Bool(visible));
        }
        if let Some(focused) = selector_state.focused {
            state.insert("focused".to_string(), serde_json::Value::Bool(focused));
        }
    }

    WaitCondition {
        state,
        timeout_ms: timeout.unwrap_or(5000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sootie_core::action::{
        ActionError, ActionProvider, ActionResult, ClickAction, DragAction, FocusAction,
        HotkeyAction, HoverAction, LaunchAction, PressAction, ScrollAction, TypeAction,
        WindowAction,
    };
    use sootie_core::perception::{
        Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider,
        ScreenshotData, StubPerceptionProvider, WaitCondition, WaitResult,
    };
    use sootie_core::selector::{Bounds, Selector};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct NoopActionProvider;

    #[async_trait::async_trait]
    impl ActionProvider for NoopActionProvider {
        async fn click(&self, _action: &ClickAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn r#type(&self, _action: &TypeAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn press(&self, _action: &PressAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn hotkey(&self, _action: &HotkeyAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn scroll(&self, _action: &ScrollAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn hover(&self, _action: &HoverAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn drag(&self, _action: &DragAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn focus(&self, _action: &FocusAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn launch(&self, _action: &LaunchAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
        async fn window_op(&self, _action: &WindowAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "noop"))
        }
    }

    struct NoopPerceptionProvider;

    #[async_trait::async_trait]
    impl PerceptionProvider for NoopPerceptionProvider {
        async fn get_context(&self) -> Result<Context, PerceptionError> {
            Ok(Context { apps: vec![] })
        }
        async fn find(
            &self,
            _selector: &Selector,
        ) -> Result<sootie_core::selector::ResolvedTarget, PerceptionError> {
            Err(PerceptionError::TargetNotFound("noop".to_string()))
        }
        async fn inspect(&self, _selector: &Selector) -> Result<DeepInspection, PerceptionError> {
            Err(PerceptionError::NotImplemented("noop".to_string()))
        }
        async fn wait(
            &self,
            _selector: &Selector,
            condition: &WaitCondition,
        ) -> Result<WaitResult, PerceptionError> {
            Ok(WaitResult {
                matched: condition.state.is_empty(),
                element: None,
                timed_out: !condition.state.is_empty(),
            })
        }
        async fn screenshot(
            &self,
            _target: Option<&Selector>,
            _region: Option<&Bounds>,
            _display_id: Option<u32>,
        ) -> Result<ScreenshotData, PerceptionError> {
            Ok(ScreenshotData {
                format: sootie_core::perception::ScreenshotFormat::Png,
                data: vec![1, 2, 3],
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1.0,
                    height: 1.0,
                }),
            })
        }
        async fn find_apps(
            &self,
            _pattern: &str,
            _limit: Option<u32>,
        ) -> Result<FindAppsResult, PerceptionError> {
            Ok(FindAppsResult {
                apps: vec![],
                total: 0,
            })
        }
    }

    fn make_server() -> SootieServer {
        SootieServer::new_in_memory(
            Box::new(StubPerceptionProvider),
            Box::new(NoopActionProvider),
        )
    }

    fn make_server_with_storage_dir(path: PathBuf) -> SootieServer {
        SootieServer::new_with_recipe_storage_dir(
            Box::new(NoopPerceptionProvider),
            Box::new(NoopActionProvider),
            Some(path),
        )
    }

    fn unique_temp_recipe_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "sootie-recipe-test-{}-{}",
            std::process::id(),
            nanos
        ))
    }

    fn make_request(method: &str, id: i64, params: Option<serde_json::Value>) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::Value::Number(id.into())),
            method: method.to_string(),
            params,
        }
    }

    #[tokio::test]
    async fn test_initialize() {
        let server = make_server();
        let req = make_request("initialize", 1, None);
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "sootie");
    }

    #[tokio::test]
    async fn test_list_tools() {
        let server = make_server();
        let req = make_request("tools/list", 1, None);
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 20);
    }

    #[tokio::test]
    async fn test_unknown_method() {
        let server = make_server();
        let req = make_request("unknown/method", 1, None);
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_tool_call_context() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_context",
                "arguments": {}
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let content = &resp.result.unwrap()["content"][0]["text"];
        assert!(content.as_str().unwrap().contains("[]"));
    }

    #[tokio::test]
    async fn test_tool_call_unknown_tool() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_nonexistent",
                "arguments": {}
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn test_tool_call_recipe_save_and_list() {
        let server = make_server();

        let save_req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "test-recipe",
                        "platforms": ["macos"],
                        "params": [],
                        "steps": [{ "action": "click" }]
                    }
                }
            })),
        );
        let resp = server.handle_request(save_req).await;
        assert!(resp.error.is_none());

        let list_req = make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_recipes",
                "arguments": {}
            })),
        );
        let resp = server.handle_request(list_req).await;
        assert!(resp.error.is_none());
        let content = &resp.result.unwrap()["content"][0]["text"];
        assert!(content.as_str().unwrap().contains("test-recipe"));
    }

    #[tokio::test]
    async fn test_tool_call_recipe_delete() {
        let server = make_server();

        let save_req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "to-delete",
                        "steps": [{ "action": "click" }]
                    }
                }
            })),
        );
        server.handle_request(save_req).await;

        let delete_req = make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_recipe_delete",
                "arguments": { "name": "to-delete" }
            })),
        );
        let resp = server.handle_request(delete_req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_recipe_run() {
        let server = make_server();

        let save_req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "test-run",
                        "params": [
                            { "name": "to", "type": "string", "required": true }
                        ],
                        "steps": [
                            { "action": "click", "target": { "role": "button", "name": "Compose" } },
                            { "action": "type", "text": "${to}" }
                        ]
                    }
                }
            })),
        );
        server.handle_request(save_req).await;

        let run_req = make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "test-run",
                    "params": { "to": "user@example.com" }
                }
            })),
        );
        let resp = server.handle_request(run_req).await;
        assert!(resp.error.is_none());
        let content = &resp.result.unwrap()["content"][0]["text"];
        let run_result: serde_json::Value =
            serde_json::from_str(content.as_str().unwrap()).unwrap();
        assert_eq!(run_result["recipe"], "test-run");
        assert_eq!(run_result["status"], "completed");
        assert_eq!(run_result["results"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_tool_call_recipe_save_persists_to_disk() {
        let recipe_dir = unique_temp_recipe_dir();
        let server = make_server_with_storage_dir(recipe_dir.clone());

        let save_req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "persisted-recipe",
                        "steps": [{ "action": "click", "target": { "role": "button", "name": "Compose" } }]
                    }
                }
            })),
        );
        let resp = server.handle_request(save_req).await;
        assert!(resp.error.is_none());

        let recipe_file = recipe_dir.join("persisted-recipe.json");
        assert!(recipe_file.exists());

        let server_reloaded = make_server_with_storage_dir(recipe_dir.clone());
        let list_req = make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_recipes",
                "arguments": {}
            })),
        );
        let resp = server_reloaded.handle_request(list_req).await;
        assert!(resp.error.is_none());
        let content = &resp.result.unwrap()["content"][0]["text"];
        assert!(content.as_str().unwrap().contains("persisted-recipe"));

        let _ = std::fs::remove_dir_all(recipe_dir);
    }

    #[tokio::test]
    async fn test_tool_call_type_missing_text() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "role": "textfield"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let is_error = resp.result.unwrap()["isError"].as_bool();
        assert_eq!(is_error, Some(true));
    }

    #[tokio::test]
    async fn test_tool_call_press() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_press",
                "arguments": { "key": "Return" }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_hotkey() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_hotkey",
                "arguments": { "keys": ["Cmd", "C"] }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_window() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_window",
                "arguments": {
                    "app": "Chrome",
                    "operation": "minimize"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_window_invalid_operation() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_window",
                "arguments": {
                    "app": "Chrome",
                    "operation": "fly"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let is_error = resp.result.unwrap()["isError"].as_bool();
        assert_eq!(is_error, Some(true));
    }

    #[tokio::test]
    async fn test_notifications_initialized() {
        let server = make_server();
        let req = make_request("notifications/initialized", 1, None);
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    // ========== Additional MCP Integration Tests ==========

    #[tokio::test]
    async fn test_tool_call_find() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "app": "Chrome",
                    "role": "button",
                    "name": "Submit"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_inspect() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_inspect",
                "arguments": {
                    "role": "button",
                    "name": "Submit"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_wait() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_wait",
                "arguments": {
                    "role": "button",
                    "name": "Submit",
                    "timeout": 1000
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_screenshot() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_screenshot",
                "arguments": {}
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_screenshot_with_region() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_screenshot",
                "arguments": {
                    "region": { "x": 0, "y": 0, "width": 100, "height": 100 }
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_click_with_target() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_click",
                "arguments": {
                    "target": { "role": "button", "name": "Submit" },
                    "button": "left",
                    "count": 1
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_click_with_coordinate() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_click",
                "arguments": {
                    "coordinate": { "x": 100, "y": 200 },
                    "button": "right"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_type_with_target() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "target": { "role": "textfield", "name": "Email" },
                    "text": "user@example.com",
                    "clear_first": true
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_scroll() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_scroll",
                "arguments": {
                    "direction": "down",
                    "amount": 5
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_hover() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_hover",
                "arguments": {
                    "coordinate": { "x": 100, "y": 200 }
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_drag() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_drag",
                "arguments": {
                    "from": { "x": 100, "y": 100 },
                    "to": { "x": 200, "y": 200 }
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_focus() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_focus",
                "arguments": {
                    "app": "Chrome"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_recipe_run_not_found() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "nonexistent",
                    "params": {}
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let is_error = resp.result.unwrap()["isError"].as_bool();
        assert_eq!(is_error, Some(true));
    }

    #[tokio::test]
    async fn test_tool_call_recipe_delete_not_found() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_delete",
                "arguments": { "name": "nonexistent" }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let is_error = resp.result.unwrap()["isError"].as_bool();
        assert_eq!(is_error, Some(true));
    }

    #[tokio::test]
    async fn test_tool_call_recipe_save_invalid() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "name": "",
                        "steps": []
                    }
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let is_error = resp.result.unwrap()["isError"].as_bool();
        assert_eq!(is_error, Some(true));
    }

    #[tokio::test]
    async fn test_tool_call_missing_arguments() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_context"
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_no_params() {
        let server = make_server();
        let req = make_request("tools/call", 1, None);
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32602);
    }

    #[tokio::test]
    async fn test_response_has_jsonrpc_version() {
        let server = make_server();
        let req = make_request("initialize", 1, None);
        let resp = server.handle_request(req).await;
        assert_eq!(resp.jsonrpc, "2.0");
    }

    #[tokio::test]
    async fn test_response_preserves_id() {
        let server = make_server();
        let req = make_request("initialize", 42, None);
        let resp = server.handle_request(req).await;
        assert_eq!(resp.id, Some(serde_json::Value::Number(42.into())));
    }

    #[tokio::test]
    async fn test_tool_call_window_all_operations() {
        let server = make_server();

        for op in &["minimize", "maximize", "close"] {
            let req = make_request(
                "tools/call",
                1,
                Some(serde_json::json!({
                    "name": "sootie_window",
                    "arguments": {
                        "app": "Chrome",
                        "operation": op
                    }
                })),
            );
            let resp = server.handle_request(req).await;
            assert!(resp.error.is_none(), "Failed for operation: {}", op);
        }
    }

    #[tokio::test]
    async fn test_tool_call_window_move() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_window",
                "arguments": {
                    "app": "Chrome",
                    "operation": "move",
                    "x": 100,
                    "y": 200
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_window_resize() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_window",
                "arguments": {
                    "app": "Chrome",
                    "operation": "resize",
                    "width": 800,
                    "height": 600
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }
}
