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
use sootie_core::logging::{create_duration_ms, LogConfig, SootieLogger, ToolCallLog, sanitize_tool_call_args};
use sootie_core::perception::{PerceptionProvider, WaitCondition};
use sootie_core::recipe::{Recipe, RecipeEngine, StepTarget};
use sootie_core::selector::AppSelector;

use crate::tools::{
    all_tools, parse_action_target, parse_mouse_button, parse_scroll_direction,
    parse_selector_from_args, parse_step_target,
};
use crate::types::{
    CallToolResult, InitializeResult, JsonRpcRequest, JsonRpcResponse, ListToolsResult,
    ServerCapabilities, ServerInfo, ToolContent, ToolsCapability,
};

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

        let name = params
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or((-32602, "Missing tool name".to_string()))?;

        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::json!({}));

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
                    content: vec![ToolContent::text(&value)],
                    is_error: None,
                };
                serde_json::to_value(call_result).map_err(|e| (-32603, e.to_string()))
            }
            Err(msg) => {
                let sanitized_args = sanitize_tool_call_args(&args, self.logger.config());

                self.logger.log_tool_call(&ToolCallLog {
                    tool_name: name.to_string(),
                    request_id,
                    arguments: sanitized_args,
                    success: false,
                    error_message: Some(msg.clone()),
                    duration_ms,
                    backend_used: None,
                });

                let call_result = CallToolResult {
                    content: vec![ToolContent::text(&msg)],
                    is_error: Some(true),
                };
                serde_json::to_value(call_result).map_err(|e| (-32603, e.to_string()))
            }
        }
    }

    async fn tool_context(&self) -> Result<String, String> {
        let ctx = self
            .perception
            .get_context()
            .await
            .map_err(|e| format!("Failed to get context: {}", e))?;
        serde_json::to_string_pretty(&ctx).map_err(|e| e.to_string())
    }

    async fn tool_find(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = parse_selector_from_args(args);
        let result = self
            .perception
            .find(&selector)
            .await
            .map_err(|e| format!("Find failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_inspect(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = parse_selector_from_args(args);
        let result = self
            .perception
            .inspect(&selector)
            .await
            .map_err(|e| format!("Inspect failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_wait(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = parse_selector_from_args(args);
        let timeout = args.get("timeout").and_then(|v| v.as_u64()).unwrap_or(5000);

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
            .map_err(|e| format!("Wait failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_screenshot(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = if args.get("app").is_some() || args.get("window").is_some() {
            Some(parse_selector_from_args(args))
        } else {
            None
        };

        let region = args.get("region").and_then(|r| {
            let x = r.get("x")?.as_f64()?;
            let y = r.get("y")?.as_f64()?;
            let width = r.get("width")?.as_f64()?;
            let height = r.get("height")?.as_f64()?;
            Some(sootie_core::selector::Bounds {
                x,
                y,
                width,
                height,
            })
        });

        let result = self
            .perception
            .screenshot(selector.as_ref(), region.as_ref())
            .await
            .map_err(|e| format!("Screenshot failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_find_apps(&self, args: &serde_json::Value) -> Result<String, String> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: pattern")?;

        let limit = args.get("limit").and_then(|v| v.as_u64()).map(|v| v as u32);

        let result = self
            .perception
            .find_apps(pattern, limit)
            .await
            .map_err(|e| format!("Find apps failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_click(&self, args: &serde_json::Value) -> Result<String, String> {
        let target = parse_action_target(args).ok_or("Must provide target or coordinate")?;

        let button = args
            .get("button")
            .and_then(|v| v.as_str())
            .map(parse_mouse_button);

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
            .map_err(|e| format!("Click failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_type(&self, args: &serde_json::Value) -> Result<String, String> {
        let text = args
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: text")?
            .to_string();

        let target = parse_action_target(args);
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
            .map_err(|e| format!("Type failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_press(&self, args: &serde_json::Value) -> Result<String, String> {
        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: key")?
            .to_string();

        let action = PressAction { key };
        let result = self
            .action
            .press(&action)
            .await
            .map_err(|e| format!("Press failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_hotkey(&self, args: &serde_json::Value) -> Result<String, String> {
        let keys = args
            .get("keys")
            .and_then(|v| v.as_array())
            .ok_or("Missing required field: keys")?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        let action = HotkeyAction { keys };
        let result = self
            .action
            .hotkey(&action)
            .await
            .map_err(|e| format!("Hotkey failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_scroll(&self, args: &serde_json::Value) -> Result<String, String> {
        let direction = args
            .get("direction")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: direction")?
            .to_string();

        let target = parse_action_target(args);
        let amount = args
            .get("amount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let action = ScrollAction {
            target,
            direction: parse_scroll_direction(&direction),
            amount,
        };

        let result = self
            .action
            .scroll(&action)
            .await
            .map_err(|e| format!("Scroll failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_hover(&self, args: &serde_json::Value) -> Result<String, String> {
        let target = parse_action_target(args).ok_or("Must provide target or coordinate")?;

        let action = HoverAction { target };
        let result = self
            .action
            .hover(&action)
            .await
            .map_err(|e| format!("Hover failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_drag(&self, args: &serde_json::Value) -> Result<String, String> {
        let from_val = args.get("from").ok_or("Missing required field: from")?;
        let to_val = args.get("to").ok_or("Missing required field: to")?;

        let from = parse_step_target(from_val)
            .map(|st| match st {
                StepTarget::Coordinate(c) => sootie_core::action::ActionTarget::Coordinate(c),
                StepTarget::Selector(s) => sootie_core::action::ActionTarget::Selector(s),
            })
            .ok_or("Invalid 'from' target")?;

        let to = parse_step_target(to_val)
            .map(|st| match st {
                StepTarget::Coordinate(c) => sootie_core::action::ActionTarget::Coordinate(c),
                StepTarget::Selector(s) => sootie_core::action::ActionTarget::Selector(s),
            })
            .ok_or("Invalid 'to' target")?;

        let action = DragAction { from, to };
        let result = self
            .action
            .drag(&action)
            .await
            .map_err(|e| format!("Drag failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_focus(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = parse_selector_from_args(args);
        let action = FocusAction { selector };
        let result = self
            .action
            .focus(&action)
            .await
            .map_err(|e| format!("Focus failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_launch(&self, args: &serde_json::Value) -> Result<String, String> {
        let app = args.get("app").ok_or("Missing required field: app")?;

        let app_selector = if let Some(s) = app.as_str() {
            AppSelector::from_name(s)
        } else {
            serde_json::from_value::<AppSelector>(app.clone())
                .map_err(|e| format!("Invalid app selector: {}", e))?
        };

        let args_list: Vec<String> = args
            .get("args")
            .and_then(|a| serde_json::from_value::<Vec<String>>(a.clone()).ok())
            .unwrap_or_default();

        let action = LaunchAction {
            app: app_selector,
            args: args_list,
        };

        let result = self
            .action
            .launch(&action)
            .await
            .map_err(|e| format!("Launch failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_window(&self, args: &serde_json::Value) -> Result<String, String> {
        let selector = parse_selector_from_args(args);
        let op_str = args
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: operation")?;

        let operation = match op_str {
            "minimize" => WindowOperation::Minimize,
            "maximize" => WindowOperation::Maximize,
            "close" => WindowOperation::Close,
            "move" => {
                let x = args.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let y = args.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0);
                WindowOperation::Move { x, y }
            }
            "resize" => {
                let width = args.get("width").and_then(|v| v.as_f64()).unwrap_or(800.0);
                let height = args.get("height").and_then(|v| v.as_f64()).unwrap_or(600.0);
                WindowOperation::Resize { width, height }
            }
            _ => return Err(format!("Unknown window operation: {}", op_str)),
        };

        let action = WindowAction {
            selector,
            operation,
        };
        let result = self
            .action
            .window_op(&action)
            .await
            .map_err(|e| format!("Window operation failed: {}", e))?;
        serde_json::to_string_pretty(&result).map_err(|e| e.to_string())
    }

    async fn tool_recipes(&self) -> Result<String, String> {
        let engine = self.recipe_engine.lock().await;
        let recipes: Vec<&Recipe> = engine.list();
        let names: Vec<&str> = recipes.iter().map(|r| r.name.as_str()).collect();
        serde_json::to_string_pretty(&names).map_err(|e| e.to_string())
    }

    async fn tool_run(&self, args: &serde_json::Value) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: name")?;

        let params: HashMap<String, serde_json::Value> = args
            .get("params")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let (recipe, substituted_steps) = {
            let engine = self.recipe_engine.lock().await;
            let recipe = engine
                .get(name)
                .ok_or_else(|| format!("Recipe not found: {}", name))?
                .clone();

            let resolved_params = engine
                .resolve_params(&recipe, &params)
                .map_err(|e| format!("Parameter error: {}", e))?;

            let steps = recipe
                .steps
                .iter()
                .map(|step| engine.substitute_step(step, &resolved_params))
                .collect::<Vec<_>>();

            (recipe, steps)
        };

        let mut results = Vec::with_capacity(substituted_steps.len());
        for (index, step) in substituted_steps.iter().enumerate() {
            let result = self
                .execute_recipe_step(index, step)
                .await
                .map_err(|e| format!("Recipe execution failed: {}", e))?;
            results.push(result);
        }

        Ok(serde_json::json!({
            "recipe": recipe.name,
            "status": "completed",
            "results": results,
        })
        .to_string())
    }

    async fn tool_recipe_save(&self, args: &serde_json::Value) -> Result<String, String> {
        let recipe_val = args.get("recipe").ok_or("Missing required field: recipe")?;

        let recipe: Recipe = serde_json::from_value(recipe_val.clone())
            .map_err(|e| format!("Invalid recipe: {}", e))?;

        let mut engine = self.recipe_engine.lock().await;
        engine
            .load(recipe.clone())
            .map_err(|e| format!("Failed to save recipe: {}", e))?;

        Ok(serde_json::json!({
            "status": "saved",
            "name": recipe.name
        })
        .to_string())
    }

    async fn tool_recipe_delete(&self, args: &serde_json::Value) -> Result<String, String> {
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or("Missing required field: name")?;

        let mut engine = self.recipe_engine.lock().await;
        engine
            .delete(name)
            .map_err(|e| format!("Failed to delete recipe: {}", e))?;

        Ok(serde_json::json!({
            "status": "deleted",
            "name": name
        })
        .to_string())
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
                    .screenshot(selector.as_ref(), None)
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
