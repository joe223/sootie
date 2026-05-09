use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use sootie_core::action::{
    ActionProvider, ActionTarget, ClickAction, DragAction, FocusAction, HotkeyAction, HoverAction,
    LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction, WindowOperation,
};
use sootie_core::cascade::Cascade;
use sootie_core::logging::{
    create_duration_ms, sanitize_tool_call_args, LogConfig, SootieLogger, ToolCallLog,
};
use sootie_core::perception::{PerceptionProvider, WaitCondition};
use sootie_core::recipe::{Recipe, RecipeEngine, StepTarget};
use sootie_core::selector::{
    AppSelector, Coordinate, FindTargetResult, MatchStatus, ResolvedElement, ResolvedTarget,
    Selector, Target, WindowSelector, WindowState,
};
use sootie_core::vision::RuntimeVisionProvider;

use crate::tools::{
    all_tools, parse_mouse_button, parse_mouse_button_strict, parse_scroll_direction,
    parse_scroll_direction_strict,
};
use crate::types::{
    CallToolRequest, CallToolResult, InitializeResult, JsonRpcRequest, JsonRpcResponse,
    ListToolsResult, ServerCapabilities, ServerInfo, ToolContent, ToolsCapability,
};

const WINDOW_CONTEXT_ATTEMPTS: usize = 3;
const WINDOW_CONTEXT_RETRY_DELAY: Duration = Duration::from_millis(150);

#[derive(Debug)]
struct ToolInvocationError {
    code: &'static str,
    message: String,
    details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct FindElementWindowScope {
    #[serde(default)]
    app: Option<String>,
    #[serde(default, rename = "windowId")]
    window_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedDescriptionWindowScope {
    app: Option<String>,
    window_id: Option<String>,
    window_title: Option<String>,
    window_index: Option<u32>,
    display_id: Option<u32>,
}

#[derive(Debug, Clone)]
struct SessionWindowContext {
    app_selector: AppSelector,
    scope: ResolvedDescriptionWindowScope,
}

fn scope_has_window(scope: &ResolvedDescriptionWindowScope) -> bool {
    scope.window_id.is_some() || scope.window_title.is_some() || scope.window_index.is_some()
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

    fn execution_with_details(message: impl Into<String>, details: serde_json::Value) -> Self {
        Self {
            code: "execution_failed",
            message: message.into(),
            details: Some(details),
        }
    }

    fn target_not_found(
        message: impl Into<String>,
        backend_attempts: Vec<String>,
        backend_errors: Option<Vec<(String, String)>>,
    ) -> Self {
        Self {
            code: "target_not_found",
            message: message.into(),
            details: Some(serde_json::json!({
                "backend_attempts": backend_attempts,
                "backend": null,
                "backend_errors": backend_errors,
            })),
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

fn success_response(data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "message": "",
        "data": data,
    })
}

fn error_response(error: &ToolInvocationError) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "message": error.message,
        "data": {
            "code": error.code,
            "details": error.details,
        },
    })
}

fn remove_role_phrase(description: &str, phrase: &str) -> String {
    description.replace(phrase, " ")
}

fn parse_description_selector(description: &str) -> Selector {
    let mut selector = Selector::new();
    let mut remaining = description.trim().to_lowercase();

    if remaining.contains("focused") {
        selector = selector.with_state(WindowState {
            visible: None,
            focused: Some(true),
        });
        remaining = remaining.replace("focused", " ");
    }

    let role_aliases = [
        ("text field", "textfield"),
        ("textfield", "textfield"),
        ("input field", "textfield"),
        ("text box", "textfield"),
        ("button", "button"),
        ("link", "link"),
        ("checkbox", "checkbox"),
        ("radio button", "radio"),
        ("tab", "tab"),
    ];

    for (phrase, role) in role_aliases {
        if remaining.contains(phrase) {
            selector = selector.with_role(role);
            remaining = remove_role_phrase(&remaining, phrase);
            break;
        }
    }

    let remaining = remaining.split_whitespace().collect::<Vec<_>>().join(" ");
    if !remaining.is_empty() {
        selector = selector.with_name(&remaining);
    }

    if selector.element.role.is_none()
        && selector.element.state.is_none()
        && selector.element.name.is_none()
    {
        selector = selector.with_name(description.trim());
    }

    selector
}

fn remove_case_insensitive_span(text: &str, start: usize, end: usize) -> String {
    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..start]);
    result.push(' ');
    result.push_str(&text[end..]);
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn parse_target(args: &serde_json::Value) -> Result<Target, ToolInvocationError> {
    let target_value = args
        .get("target")
        .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: target"))?;

    serde_json::from_value(target_value.clone())
        .map_err(|e| ToolInvocationError::invalid_arguments(format!("Invalid target: {}", e)))
}

fn parse_named_target(
    args: &serde_json::Value,
    field: &str,
) -> Result<Target, ToolInvocationError> {
    let target_value = args.get(field).ok_or_else(|| {
        ToolInvocationError::invalid_arguments(format!("Missing required field: {}", field))
    })?;

    serde_json::from_value(target_value.clone())
        .map_err(|e| ToolInvocationError::invalid_arguments(format!("Invalid {}: {}", field, e)))
}

fn parse_selector_args(args: &serde_json::Value) -> Result<Selector, ToolInvocationError> {
    serde_json::from_value(args.clone())
        .map_err(|e| ToolInvocationError::invalid_arguments(format!("Invalid selector: {}", e)))
}

fn parse_find_element_window_scope(
    args: &serde_json::Value,
) -> Result<Option<FindElementWindowScope>, ToolInvocationError> {
    let Some(window_value) = args.get("window") else {
        return Ok(None);
    };

    serde_json::from_value(window_value.clone())
        .map(Some)
        .map_err(|e| ToolInvocationError::invalid_arguments(format!("Invalid window scope: {}", e)))
}

fn parse_window_index_from_id(window_id: &str) -> Option<u32> {
    window_id.strip_prefix("win_")?.parse::<u32>().ok()
}

pub struct SootieServer {
    perception: Arc<Box<dyn PerceptionProvider>>,
    action: Arc<Box<dyn ActionProvider>>,
    recipe_engine: Arc<Mutex<RecipeEngine>>,
    session_window_context: Arc<Mutex<Option<SessionWindowContext>>>,
    logger: SootieLogger,
}

impl SootieServer {
    pub fn new(perception: Box<dyn PerceptionProvider>, action: Box<dyn ActionProvider>) -> Self {
        Self {
            perception: Arc::new(perception),
            action: Arc::new(action),
            recipe_engine: Arc::new(Mutex::new(RecipeEngine::new())),
            session_window_context: Arc::new(Mutex::new(None)),
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
            session_window_context: Arc::new(Mutex::new(None)),
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
            session_window_context: Arc::new(Mutex::new(None)),
            logger: SootieLogger::new(LogConfig::default()),
        }
    }

    async fn run_find_target(
        &self,
        target: &Target,
    ) -> Result<FindTargetResult, ToolInvocationError> {
        target
            .validate()
            .map_err(|e| ToolInvocationError::invalid_arguments(e.to_string()))?;

        let vision = RuntimeVisionProvider::from_env();
        let cascade = Cascade::new(self.perception.as_ref().as_ref(), Some(&vision));
        let result = cascade.find_target(target).await;

        // Log detailed backend attempts for debugging
        if result.status == MatchStatus::None {
            warn!(
                target = ?target,
                backend_attempts = ?result.backend_attempts,
                "Find target returned no matches"
            );
        }

        Ok(result)
    }

    async fn run_find_selector(
        &self,
        selector: &Selector,
    ) -> Result<FindTargetResult, ToolInvocationError> {
        let resolved = self.perception.find(selector).await.map_err(|e| {
            ToolInvocationError::execution(format!("Find element via AT tree failed: {}", e))
        })?;

        Ok(Self::resolved_target_to_find_result(resolved, "at_tree"))
    }

    fn resolved_target_to_find_result(resolved: ResolvedTarget, backend: &str) -> FindTargetResult {
        let elements = resolved
            .elements
            .into_iter()
            .map(|element| {
                let coordinate = Coordinate {
                    x: element.bounds.x + element.bounds.width / 2.0,
                    y: element.bounds.y + element.bounds.height / 2.0,
                };
                ResolvedElement {
                    role: element.role,
                    name: Some(element.name),
                    text: element.text,
                    id: element.id,
                    state: element.state,
                    bounds: element.bounds,
                    coordinate,
                    index: Some(element.index),
                    confidence: None,
                }
            })
            .collect();

        FindTargetResult {
            status: resolved.status,
            backend: backend.to_string(),
            backend_attempts: Some(vec![backend.to_string()]),
            app: resolved.app,
            window: resolved.window,
            elements,
            confidence: None,
            backend_errors: None,
        }
    }

    async fn resolve_description_window_scope(
        &self,
        args: &serde_json::Value,
    ) -> Result<Option<ResolvedDescriptionWindowScope>, ToolInvocationError> {
        let Some(scope) = parse_find_element_window_scope(args)? else {
            return Ok(None);
        };

        let mut resolved = ResolvedDescriptionWindowScope {
            app: scope.app.clone(),
            window_id: scope.window_id.clone(),
            window_title: None,
            window_index: scope
                .window_id
                .as_deref()
                .and_then(parse_window_index_from_id),
            display_id: None,
        };

        let Some(window_id) = scope.window_id.as_deref() else {
            return Ok(Some(resolved));
        };

        let context =
            self.perception.get_context().await.map_err(|e| {
                ToolInvocationError::execution(format!("Failed to get context: {}", e))
            })?;

        for app_context in context.apps {
            if let Some(app_name) = scope.app.as_deref() {
                if app_context.app.name != app_name {
                    continue;
                }
            }

            if let Some(window) = app_context
                .windows
                .into_iter()
                .find(|window| window.id == window_id)
            {
                resolved.app.get_or_insert(app_context.app.name);
                resolved.window_title = Some(window.title);
                resolved.window_index = Some(window.index);
                resolved.display_id = window.display_id;
                return Ok(Some(resolved));
            }
        }

        Ok(Some(resolved))
    }

    async fn resolve_frontmost_window_scope(
        &self,
    ) -> Result<Option<ResolvedDescriptionWindowScope>, ToolInvocationError> {
        let context = self.perception.get_context().await.map_err(|e| {
            ToolInvocationError::execution(format!("Failed to get frontmost context: {}", e))
        })?;

        let Some(app_context) = context.apps.into_iter().find(|app| app.app.is_frontmost) else {
            debug!("No frontmost app found for implicit find_element scope");
            return Ok(None);
        };

        let window = app_context
            .windows
            .iter()
            .find(|window| window.focused)
            .or_else(|| app_context.windows.first())
            .cloned();

        let scope = ResolvedDescriptionWindowScope {
            app: Some(app_context.app.name),
            window_id: window.as_ref().map(|window| window.id.clone()),
            window_title: window.as_ref().map(|window| window.title.clone()),
            window_index: window.as_ref().map(|window| window.index),
            display_id: window.as_ref().and_then(|window| window.display_id),
        };

        debug!(
            app = ?scope.app,
            window_id = ?scope.window_id,
            window_title = ?scope.window_title,
            window_index = ?scope.window_index,
            "Resolved implicit find_element scope from frontmost window"
        );

        Ok(Some(scope))
    }

    async fn resolve_app_selector_window_scope(
        &self,
        app_selector: &AppSelector,
    ) -> Result<Option<ResolvedDescriptionWindowScope>, ToolInvocationError> {
        let context = self.perception.get_context().await.map_err(|e| {
            ToolInvocationError::execution(format!("Failed to get app context: {}", e))
        })?;

        for app_context in context.apps {
            let matches_name = app_selector
                .name
                .as_ref()
                .is_some_and(|name| app_context.app.name == *name);
            let matches_bundle = app_selector
                .bundle_id
                .as_ref()
                .is_some_and(|bundle_id| app_context.app.bundle_id == *bundle_id);
            if !matches_name && !matches_bundle {
                continue;
            }

            let window = app_context
                .windows
                .iter()
                .find(|window| window.focused)
                .or_else(|| app_context.windows.first())
                .cloned();
            return Ok(Some(ResolvedDescriptionWindowScope {
                app: Some(app_context.app.name),
                window_id: window.as_ref().map(|window| window.id.clone()),
                window_title: window.as_ref().map(|window| window.title.clone()),
                window_index: window.as_ref().map(|window| window.index),
                display_id: window.as_ref().and_then(|window| window.display_id),
            }));
        }

        Ok(None)
    }

    async fn set_session_app_context(&self, app_selector: AppSelector) {
        let scope = self
            .resolve_app_selector_window_scope_until_window(&app_selector)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| ResolvedDescriptionWindowScope {
                app: app_selector.name.clone(),
                window_id: None,
                window_title: None,
                window_index: None,
                display_id: None,
            });
        *self.session_window_context.lock().await = Some(SessionWindowContext {
            app_selector,
            scope,
        });
    }

    async fn resolve_app_selector_window_scope_until_window(
        &self,
        app_selector: &AppSelector,
    ) -> Result<Option<ResolvedDescriptionWindowScope>, ToolInvocationError> {
        let mut last_scope = None;

        for attempt in 1..=WINDOW_CONTEXT_ATTEMPTS {
            let scope = self.resolve_app_selector_window_scope(app_selector).await?;
            if scope.as_ref().is_some_and(scope_has_window) {
                return Ok(scope);
            }

            if scope.is_some() {
                last_scope = scope;
            }

            if attempt < WINDOW_CONTEXT_ATTEMPTS {
                tokio::time::sleep(WINDOW_CONTEXT_RETRY_DELAY).await;
            }
        }

        Ok(last_scope)
    }

    async fn resolve_session_window_scope(
        &self,
    ) -> Result<Option<ResolvedDescriptionWindowScope>, ToolInvocationError> {
        let session_context = { self.session_window_context.lock().await.clone() };
        let Some(session_context) = session_context else {
            return Ok(None);
        };

        debug!(
            app = ?session_context.scope.app,
            window_id = ?session_context.scope.window_id,
            window_title = ?session_context.scope.window_title,
            window_index = ?session_context.scope.window_index,
            display_id = ?session_context.scope.display_id,
            "Resolved implicit find_element scope from lightweight session window context"
        );
        Ok(Some(session_context.scope))
    }

    async fn resolve_app_hint_window_scope(
        &self,
        description: &str,
    ) -> Result<(String, Option<ResolvedDescriptionWindowScope>), ToolInvocationError> {
        if let Some(session_context) = self.session_window_context.lock().await.clone() {
            if let Some(app_name) = session_context.app_selector.name.as_deref() {
                let app_name_lower = app_name.to_lowercase();
                let description_lower = description.to_lowercase();
                let candidates = [
                    format!(" in app {}", app_name_lower),
                    format!(" in {}", app_name_lower),
                ];
                if let Some((start, end)) = candidates.iter().find_map(|candidate| {
                    description_lower
                        .find(candidate)
                        .map(|start| (start, start + candidate.len()))
                }) {
                    return Ok((
                        remove_case_insensitive_span(description, start, end),
                        Some(session_context.scope),
                    ));
                }
            }
        }

        let context = self.perception.get_context().await.map_err(|e| {
            ToolInvocationError::execution(format!("Failed to get app context: {}", e))
        })?;
        let description_lower = description.to_lowercase();

        for app_context in context.apps {
            let app_name_lower = app_context.app.name.to_lowercase();
            let candidates = [
                format!(" in app {}", app_name_lower),
                format!(" in {}", app_name_lower),
            ];

            let Some((start, end)) = candidates.iter().find_map(|candidate| {
                description_lower
                    .find(candidate)
                    .map(|start| (start, start + candidate.len()))
            }) else {
                continue;
            };

            let scope = self
                .resolve_app_selector_window_scope_until_window(&AppSelector::from_name(
                    &app_context.app.name,
                ))
                .await?
                .unwrap_or(ResolvedDescriptionWindowScope {
                    app: Some(app_context.app.name),
                    window_id: None,
                    window_title: None,
                    window_index: None,
                    display_id: None,
                });
            let element_description = remove_case_insensitive_span(description, start, end);

            debug!(
                original_description = description,
                element_description,
                app = ?scope.app,
                window_id = ?scope.window_id,
                window_title = ?scope.window_title,
                "Resolved find_element scope from app hint in description"
            );

            return Ok((element_description, Some(scope)));
        }

        Ok((description.to_string(), None))
    }

    fn build_description_window_selector(
        scope: &ResolvedDescriptionWindowScope,
    ) -> Option<WindowSelector> {
        if scope.window_id.is_none() && scope.window_title.is_none() && scope.window_index.is_none()
        {
            return None;
        }

        Some(WindowSelector {
            title: scope.window_title.clone(),
            id: scope.window_id.clone(),
            index: scope.window_index,
            focused: None,
        })
    }

    async fn resolve_description_target_or_error(
        &self,
        description: &str,
        window_scope: Option<ResolvedDescriptionWindowScope>,
        message: &str,
    ) -> Result<FindTargetResult, ToolInvocationError> {
        let (description, inferred_scope) = if window_scope.is_some() {
            (description.to_string(), None)
        } else {
            self.resolve_app_hint_window_scope(description).await?
        };
        let window_scope = match window_scope.or(inferred_scope) {
            Some(scope) => Some(scope),
            None => match self.resolve_session_window_scope().await? {
                Some(scope) => Some(scope),
                None => self.resolve_frontmost_window_scope().await?,
            },
        };
        let parsed_selector = parse_description_selector(&description);

        let mut selector = parsed_selector;
        if let Some(scope) = window_scope.as_ref() {
            if let Some(app) = scope.app.as_deref() {
                selector = selector.with_app(AppSelector::from_name(app));
            }
            if let Some(window) = Self::build_description_window_selector(scope) {
                selector = selector.with_window(window);
            }
        }

        if selector.element.state.is_some() {
            let result = self.run_find_selector(&selector).await?;
            if result.status != MatchStatus::None && !result.elements.is_empty() {
                return Ok(result);
            }
            return Err(ToolInvocationError::target_not_found(
                message,
                result.backend_attempts.clone().unwrap_or_default(),
                result.backend_errors.clone(),
            ));
        }

        use sootie_core::selector::TargetSelector;
        let target = Target {
            app: window_scope
                .as_ref()
                .and_then(|scope| scope.app.as_deref())
                .map(AppSelector::from_name),
            window: window_scope
                .as_ref()
                .and_then(Self::build_description_window_selector),
            selector: TargetSelector {
                role: selector.element.role,
                name: selector.element.name,
                text: selector.element.text,
                id: selector.element.id,
            },
        };

        self.find_target_or_error(&target, message).await
    }

    async fn find_target_or_error(
        &self,
        target: &Target,
        message: &str,
    ) -> Result<FindTargetResult, ToolInvocationError> {
        let result = self.run_find_target(target).await?;
        if result.status == MatchStatus::None || result.elements.is_empty() {
            return Err(ToolInvocationError::target_not_found(
                message,
                result.backend_attempts.clone().unwrap_or_default(),
                result.backend_errors.clone(),
            ));
        }

        Ok(result)
    }

    fn execution_details(result: &FindTargetResult) -> serde_json::Value {
        serde_json::json!({
            "backend": result.backend,
            "element_index": 0
        })
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
                // Return value won't be sent - main.rs checks request.id
                Ok(serde_json::json!(null))
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
            "sootie_find_apps" => self.tool_find_apps(&args).await,
            "sootie_find_element" => self.tool_find_element(&args).await,
            "sootie_tap_by_name" => self.tool_tap_by_name(&args).await,
            "sootie_tap_by_position" => self.tool_tap_by_position(&args).await,
            "sootie_type" => self.tool_type(&args).await,
            "sootie_press_by_name" => self.tool_press_by_name(&args).await,
            "sootie_press_by_position" => self.tool_press_by_position(&args).await,
            "sootie_hotkey" => self.tool_hotkey(&args).await,
            "sootie_scroll" => self.tool_scroll(&args).await,
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
                let response = success_response(value);
                let content_text = present_tool_data(&response).map_err(|e| (-32603, e.message))?;

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
                    structured_content: Some(response),
                };
                serde_json::to_value(call_result).map_err(|e| (-32603, e.to_string()))
            }
            Err(err) => {
                let sanitized_args = sanitize_tool_call_args(&args, self.logger.config());
                let response = error_response(&err);
                let content_text = present_tool_data(&response).map_err(|e| (-32603, e.message))?;

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
                    content: vec![ToolContent::text(&content_text)],
                    is_error: Some(true),
                    structured_content: Some(response),
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

    async fn tool_find_element(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let el_description = args
            .get("el_description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: el_description")
            })?
            .to_string();

        let window_scope = self.resolve_description_window_scope(args).await?;
        let find_result = self
            .resolve_description_target_or_error(
                &el_description,
                window_scope,
                "No element matched the requested element description",
            )
            .await?;

        let elements = find_result
            .elements
            .iter()
            .map(|elem| {
                serde_json::json!({
                    "role": elem.role,
                    "name": elem.name,
                    "text": elem.text,
                    "id": elem.id,
                    "position": {
                        "x": elem.bounds.x,
                        "y": elem.bounds.y,
                        "width": elem.bounds.width,
                        "height": elem.bounds.height
                    },
                    "coordinate": {
                        "x": elem.coordinate.x,
                        "y": elem.coordinate.y
                    },
                    "state": {
                        "visible": elem.state.visible,
                        "focused": elem.state.focused,
                        "enabled": elem.state.enabled
                    },
                    "index": elem.index
                })
            })
            .collect::<Vec<_>>();

        to_json_value(elements)
    }

    async fn tool_tap_by_name(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let el_description = args
            .get("el_description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: el_description")
            })?
            .to_string();
        let window_scope = self.resolve_description_window_scope(args).await?;
        let find_result = self
            .resolve_description_target_or_error(
                &el_description,
                window_scope,
                "No element matched the requested element description",
            )
            .await?;
        let target_element = &find_result.elements[0];
        let coordinate = Coordinate {
            x: target_element.bounds.x + target_element.bounds.width / 2.0,
            y: target_element.bounds.y + target_element.bounds.height / 2.0,
        };

        let button = args
            .get("button")
            .and_then(|v| v.as_str())
            .map(parse_mouse_button_strict)
            .transpose()
            .map_err(ToolInvocationError::invalid_arguments)?;

        let count = args.get("count").and_then(|v| v.as_u64()).map(|v| v as u32);

        let action = ClickAction {
            target: ActionTarget::Coordinate(coordinate.clone()),
            button,
            count,
        };

        let result = self.action.click(&action).await.map_err(|e| {
            ToolInvocationError::execution_with_details(
                format!("Tap by name failed: {}", e),
                Self::execution_details(&find_result),
            )
        })?;
        let mut value = to_json_value(result)?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "target".to_string(),
                serde_json::json!({
                    "coordinate": {
                        "x": coordinate.x,
                        "y": coordinate.y
                    },
                    "position": {
                        "x": target_element.bounds.x,
                        "y": target_element.bounds.y,
                        "width": target_element.bounds.width,
                        "height": target_element.bounds.height
                    },
                    "name": target_element.name,
                    "role": target_element.role,
                    "backend": find_result.backend
                }),
            );
        }
        Ok(value)
    }

    async fn tool_tap_by_position(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        use sootie_core::selector::Coordinate;

        let x = args
            .get("x")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: x"))?;

        let y = args
            .get("y")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: y"))?;

        let button = args
            .get("button")
            .and_then(|v| v.as_str())
            .map(parse_mouse_button_strict)
            .transpose()
            .map_err(ToolInvocationError::invalid_arguments)?;

        let count = args.get("count").and_then(|v| v.as_u64()).map(|v| v as u32);

        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x, y }),
            button,
            count,
        };

        let result = self.action.click(&action).await.map_err(|e| {
            ToolInvocationError::execution(format!("Tap by position failed: {}", e))
        })?;
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

        let clear_first = args.get("clear_first").and_then(|v| v.as_bool());
        let window_scope = self.resolve_description_window_scope(args).await?;

        let target = if let Some(field) = args.get("field").and_then(|v| v.as_str()) {
            let find_result = self
                .resolve_description_target_or_error(
                    field,
                    window_scope,
                    "No element matched the requested field name",
                )
                .await?;
            Some(ActionTarget::Coordinate(
                find_result.elements[0].coordinate.clone(),
            ))
        } else {
            None
        };

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

    async fn tool_press_by_name(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let el_description = args
            .get("el_description")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ToolInvocationError::invalid_arguments("Missing required field: el_description")
            })?
            .to_string();

        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: key"))?
            .to_string();
        let window_scope = self.resolve_description_window_scope(args).await?;
        let find_result = self
            .resolve_description_target_or_error(
                &el_description,
                window_scope,
                "No element matched the requested element description",
            )
            .await?;

        let action = PressAction { key };
        let result = self.action.press(&action).await.map_err(|e| {
            ToolInvocationError::execution_with_details(
                format!("Press by name failed: {}", e),
                Self::execution_details(&find_result),
            )
        })?;
        to_json_value(result)
    }

    async fn tool_press_by_position(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let _x = args
            .get("x")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: x"))?;

        let _y = args
            .get("y")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: y"))?;

        let key = args
            .get("key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolInvocationError::invalid_arguments("Missing required field: key"))?
            .to_string();

        let action = PressAction { key };
        let result = self.action.press(&action).await.map_err(|e| {
            ToolInvocationError::execution(format!("Press by position failed: {}", e))
        })?;
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
        let direction = parse_scroll_direction_strict(&direction)
            .map_err(ToolInvocationError::invalid_arguments)?;

        let target = parse_target(args)?;
        let find_result = self
            .find_target_or_error(&target, "No element matched the requested target")
            .await?;
        let coordinate = find_result.elements[0].coordinate.clone();
        let amount = args
            .get("amount")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(coordinate)),
            direction,
            amount,
        };

        let result = self.action.scroll(&action).await.map_err(|e| {
            ToolInvocationError::execution_with_details(
                format!("Scroll failed: {}", e),
                Self::execution_details(&find_result),
            )
        })?;
        to_json_value(result)
    }

    async fn tool_drag(
        &self,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, ToolInvocationError> {
        let from_target = parse_named_target(args, "from_target")?;
        let to_target = parse_named_target(args, "to_target")?;
        let from_result = self
            .find_target_or_error(&from_target, "No element matched the from_target")
            .await?;
        let to_result = self
            .find_target_or_error(&to_target, "No element matched the to_target")
            .await?;

        let action = DragAction {
            from: ActionTarget::Coordinate(from_result.elements[0].coordinate.clone()),
            to: ActionTarget::Coordinate(to_result.elements[0].coordinate.clone()),
        };
        let result = self.action.drag(&action).await.map_err(|e| {
            ToolInvocationError::execution_with_details(
                format!("Drag failed: {}", e),
                serde_json::json!({
                    "backend": from_result.backend,
                    "element_index": 0,
                    "to_backend": to_result.backend,
                    "to_element_index": 0
                }),
            )
        })?;
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

        let selector = parse_selector_args(args)?;
        let action = FocusAction { selector };
        let result = self
            .action
            .focus(&action)
            .await
            .map_err(|e| ToolInvocationError::execution(format!("Focus failed: {}", e)))?;
        if let Some(app_selector) = action.selector.app.clone() {
            self.set_session_app_context(app_selector).await;
        }
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
        self.set_session_app_context(action.app.clone()).await;
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

        let selector = parse_selector_args(args)?;
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
        if let Some(app_selector) = action.selector.app.clone() {
            self.set_session_app_context(app_selector).await;
        }
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
        ActionError, ActionProvider, ActionResult, ActionTarget, ClickAction, DragAction,
        FocusAction, HotkeyAction, HoverAction, LaunchAction, PressAction, ScrollAction,
        TypeAction, WindowAction,
    };
    use sootie_core::perception::{
        AppContext, Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider,
        ScreenshotData, StubPerceptionProvider, WaitCondition, WaitResult,
    };
    use sootie_core::selector::{App, Bounds, Selector, Window};
    use std::sync::{Arc, Mutex};
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

    struct RecordingActionProvider {
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl ActionProvider for RecordingActionProvider {
        async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError> {
            let mut calls = self.calls.lock().unwrap();
            match &action.target {
                ActionTarget::Coordinate(coordinate) => {
                    calls.push(format!("click x={:.1} y={:.1}", coordinate.x, coordinate.y))
                }
                ActionTarget::Selector(_) => calls.push("click selector".to_string()),
            }
            Ok(ActionResult::success(None, "recording"))
        }
        async fn r#type(&self, _action: &TypeAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn press(&self, _action: &PressAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn hotkey(&self, _action: &HotkeyAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn scroll(&self, _action: &ScrollAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn hover(&self, _action: &HoverAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn drag(&self, _action: &DragAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn focus(&self, action: &FocusAction) -> Result<ActionResult, ActionError> {
            let mut calls = self.calls.lock().unwrap();
            calls.push(format!(
                "focus app={:?} window_id={:?}",
                action
                    .selector
                    .app
                    .as_ref()
                    .and_then(|app| app.name.clone()),
                action
                    .selector
                    .window
                    .as_ref()
                    .and_then(|window| window.id.clone())
            ));
            Ok(ActionResult::success(None, "recording"))
        }
        async fn launch(&self, _action: &LaunchAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
        async fn window_op(&self, _action: &WindowAction) -> Result<ActionResult, ActionError> {
            Ok(ActionResult::success(None, "recording"))
        }
    }

    struct NoopPerceptionProvider;

    struct RecordingPerceptionProvider {
        selectors: Arc<Mutex<Vec<Selector>>>,
    }

    struct ContextPerceptionProvider {
        context: Context,
    }

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

    #[async_trait::async_trait]
    impl PerceptionProvider for RecordingPerceptionProvider {
        async fn get_context(&self) -> Result<Context, PerceptionError> {
            Ok(Context { apps: vec![] })
        }

        async fn find(
            &self,
            selector: &Selector,
        ) -> Result<sootie_core::selector::ResolvedTarget, PerceptionError> {
            self.selectors.lock().unwrap().push(selector.clone());

            if selector.element.role.as_deref() == Some("textfield")
                && selector
                    .element
                    .state
                    .as_ref()
                    .and_then(|state| state.focused)
                    == Some(true)
            {
                return Ok(sootie_core::selector::ResolvedTarget {
                    status: MatchStatus::Unique,
                    total_matches: 1,
                    app: None,
                    window: None,
                    elements: vec![sootie_core::selector::Element {
                        role: "textfield".to_string(),
                        name: "Address and search bar".to_string(),
                        text: None,
                        id: None,
                        state: sootie_core::selector::ElementState {
                            visible: true,
                            focused: Some(true),
                            enabled: Some(true),
                        },
                        bounds: Bounds {
                            x: 10.0,
                            y: 20.0,
                            width: 200.0,
                            height: 30.0,
                        },
                        index: 0,
                    }],
                });
            }

            Err(PerceptionError::TargetNotFound("not found".to_string()))
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
            Err(PerceptionError::NotImplemented("noop".to_string()))
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

    #[async_trait::async_trait]
    impl PerceptionProvider for ContextPerceptionProvider {
        async fn get_context(&self) -> Result<Context, PerceptionError> {
            Ok(self.context.clone())
        }

        async fn find(
            &self,
            _selector: &Selector,
        ) -> Result<sootie_core::selector::ResolvedTarget, PerceptionError> {
            Err(PerceptionError::TargetNotFound("not found".to_string()))
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
            Err(PerceptionError::NotImplemented("noop".to_string()))
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

    fn make_server_with_recording_action(calls: Arc<Mutex<Vec<String>>>) -> SootieServer {
        SootieServer::new_in_memory(
            Box::new(StubPerceptionProvider),
            Box::new(RecordingActionProvider { calls }),
        )
    }

    fn make_server_with_storage_dir(path: PathBuf) -> SootieServer {
        SootieServer::new_with_recipe_storage_dir(
            Box::new(NoopPerceptionProvider),
            Box::new(NoopActionProvider),
            Some(path),
        )
    }

    fn make_server_with_recording_perception(selectors: Arc<Mutex<Vec<Selector>>>) -> SootieServer {
        SootieServer::new_in_memory(
            Box::new(RecordingPerceptionProvider { selectors }),
            Box::new(NoopActionProvider),
        )
    }

    fn make_server_with_context(context: Context) -> SootieServer {
        SootieServer::new_in_memory(
            Box::new(ContextPerceptionProvider { context }),
            Box::new(NoopActionProvider),
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
        assert_eq!(tools.len(), 18);
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
        let content: serde_json::Value = serde_json::from_str(content.as_str().unwrap()).unwrap();
        assert_eq!(content["success"], true);
        assert!(content["data"].to_string().contains("[]"));
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
        assert_eq!(run_result["success"], true);
        assert_eq!(run_result["data"]["recipe"], "test-run");
        assert_eq!(run_result["data"]["status"], "completed");
        assert_eq!(run_result["data"]["results"].as_array().unwrap().len(), 2);
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
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["success"], false);
        assert_eq!(
            result["structuredContent"]["message"],
            "Missing required field: text"
        );
    }

    #[tokio::test]
    async fn test_tool_call_type_without_field() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "text": "user@example.com",
                    "clear_first": true
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_find_element() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find_element",
                "arguments": {
                    "el_description": "Submit"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["success"], false);
        assert_eq!(
            result["structuredContent"]["data"]["code"],
            "target_not_found"
        );
    }

    #[tokio::test]
    async fn test_tool_call_find_element_does_not_focus_before_find() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = make_server_with_recording_action(calls.clone());
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find_element",
                "arguments": {
                    "el_description": "Submit",
                    "window": {
                        "app": "Safari",
                        "windowId": "win_42"
                    }
                }
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let calls = calls.lock().unwrap();
        assert!(!calls.iter().any(|call| call.starts_with("focus")));
    }

    #[tokio::test]
    async fn test_find_element_defaults_to_frontmost_focused_window_scope() {
        let server = make_server_with_context(Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![
                    Window {
                        id: "win_0".to_string(),
                        title: "Other".to_string(),
                        index: 0,
                        focused: false,
                        bounds: Bounds {
                            x: 0.0,
                            y: 0.0,
                            width: 1200.0,
                            height: 800.0,
                        },
                        display_id: Some(1),
                    },
                    Window {
                        id: "win_1".to_string(),
                        title: "Start Page".to_string(),
                        index: 1,
                        focused: true,
                        bounds: Bounds {
                            x: -1600.0,
                            y: 0.0,
                            width: 1600.0,
                            height: 900.0,
                        },
                        display_id: Some(2),
                    },
                ],
            }],
        });

        let scope = server
            .resolve_frontmost_window_scope()
            .await
            .unwrap()
            .unwrap();

        assert_eq!(scope.app.as_deref(), Some("Safari"));
        assert_eq!(scope.window_id.as_deref(), Some("win_1"));
        assert_eq!(scope.window_title.as_deref(), Some("Start Page"));
        assert_eq!(scope.window_index, Some(1));
    }

    #[tokio::test]
    async fn test_find_element_app_hint_overrides_frontmost_scope() {
        let server = make_server_with_context(Context {
            apps: vec![
                AppContext {
                    app: App {
                        name: "Opencode".to_string(),
                        bundle_id: "ai.opencode".to_string(),
                        is_frontmost: true,
                    },
                    windows: vec![Window {
                        id: "win_0".to_string(),
                        title: "Current task".to_string(),
                        index: 0,
                        focused: true,
                        bounds: Bounds {
                            x: 0.0,
                            y: 0.0,
                            width: 1600.0,
                            height: 900.0,
                        },
                        display_id: Some(1),
                    }],
                },
                AppContext {
                    app: App {
                        name: "Safari".to_string(),
                        bundle_id: "com.apple.Safari".to_string(),
                        is_frontmost: false,
                    },
                    windows: vec![Window {
                        id: "win_1".to_string(),
                        title: "Start Page".to_string(),
                        index: 1,
                        focused: false,
                        bounds: Bounds {
                            x: -1600.0,
                            y: 0.0,
                            width: 1600.0,
                            height: 900.0,
                        },
                        display_id: Some(2),
                    }],
                },
            ],
        });

        let (description, scope) = server
            .resolve_app_hint_window_scope("URL input field or address bar in Safari")
            .await
            .unwrap();
        let scope = scope.unwrap();

        assert_eq!(description, "URL input field or address bar");
        assert_eq!(scope.app.as_deref(), Some("Safari"));
        assert_eq!(scope.window_id.as_deref(), Some("win_1"));
        assert_eq!(scope.window_title.as_deref(), Some("Start Page"));
    }

    #[tokio::test]
    async fn test_find_element_window_app_scope_stays_lightweight() {
        let server = make_server_with_context(Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: false,
                },
                windows: vec![Window {
                    id: "win_3".to_string(),
                    title: "Start Page".to_string(),
                    index: 3,
                    focused: true,
                    bounds: Bounds {
                        x: -1600.0,
                        y: 0.0,
                        width: 1600.0,
                        height: 900.0,
                    },
                    display_id: Some(2),
                }],
            }],
        });

        let scope = server
            .resolve_description_window_scope(&serde_json::json!({
                "window": {
                    "app": "Safari"
                }
            }))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(scope.app.as_deref(), Some("Safari"));
        assert_eq!(scope.window_id, None);
        assert_eq!(scope.window_index, None);
        assert_eq!(scope.display_id, None);
    }

    #[tokio::test]
    async fn test_find_element_defaults_to_session_window_context_before_frontmost() {
        let server = make_server_with_context(Context {
            apps: vec![
                AppContext {
                    app: App {
                        name: "Opencode".to_string(),
                        bundle_id: "ai.opencode".to_string(),
                        is_frontmost: true,
                    },
                    windows: vec![Window {
                        id: "win_0".to_string(),
                        title: "Current task".to_string(),
                        index: 0,
                        focused: true,
                        bounds: Bounds {
                            x: 0.0,
                            y: 0.0,
                            width: 1600.0,
                            height: 900.0,
                        },
                        display_id: Some(1),
                    }],
                },
                AppContext {
                    app: App {
                        name: "Safari".to_string(),
                        bundle_id: "com.apple.Safari".to_string(),
                        is_frontmost: false,
                    },
                    windows: vec![Window {
                        id: "win_1".to_string(),
                        title: "Start Page".to_string(),
                        index: 1,
                        focused: false,
                        bounds: Bounds {
                            x: -1600.0,
                            y: 0.0,
                            width: 1600.0,
                            height: 900.0,
                        },
                        display_id: Some(2),
                    }],
                },
            ],
        });
        let launch_req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_launch",
                "arguments": {
                    "app": "Safari"
                }
            })),
        );

        let resp = server.handle_request(launch_req).await;
        assert!(resp.error.is_none());

        let scope = server
            .resolve_session_window_scope()
            .await
            .unwrap()
            .unwrap();

        assert_eq!(scope.app.as_deref(), Some("Safari"));
        assert_eq!(scope.window_id.as_deref(), Some("win_1"));
        assert_eq!(scope.window_title.as_deref(), Some("Start Page"));
        assert_eq!(scope.window_index, Some(1));
        assert_eq!(scope.display_id, Some(2));
    }

    #[test]
    fn test_parse_description_selector_extracts_focused_text_field() {
        let selector = parse_description_selector("focused text field");
        assert_eq!(selector.element.role.as_deref(), Some("textfield"));
        assert!(selector.element.name.is_none());
        assert_eq!(
            selector
                .element
                .state
                .as_ref()
                .and_then(|state| state.focused),
            Some(true)
        );
    }

    #[tokio::test]
    async fn test_tool_call_find_element_uses_structured_selector_for_focused_text_field() {
        let selectors = Arc::new(Mutex::new(Vec::new()));
        let server = make_server_with_recording_perception(selectors.clone());
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find_element",
                "arguments": {
                    "el_description": "focused text field"
                }
            })),
        );

        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());

        let selectors = selectors.lock().unwrap();
        assert_eq!(selectors.len(), 1);
        assert_eq!(selectors[0].element.role.as_deref(), Some("textfield"));
        assert_eq!(
            selectors[0]
                .element
                .state
                .as_ref()
                .and_then(|state| state.focused),
            Some(true)
        );

        let result = resp.result.unwrap();
        assert!(result.get("isError").is_none() || result["isError"] == false);
        assert_eq!(result["structuredContent"]["success"], true);
    }

    #[tokio::test]
    async fn test_tool_call_tap_by_name_does_not_focus_before_find() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = make_server_with_recording_action(calls.clone());
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_tap_by_name",
                "arguments": {
                    "el_description": "Submit",
                    "window": {
                        "app": "Safari",
                        "windowId": "win_42"
                    }
                }
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 0);
    }

    #[tokio::test]
    async fn test_tool_call_tap_by_name() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_tap_by_name",
                "arguments": {
                    "el_description": "Submit",
                    "button": "left",
                    "count": 1
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_tap_by_position() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_tap_by_position",
                "arguments": {
                    "x": 100.0,
                    "y": 200.0,
                    "button": "right"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_type_with_field() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "field": "Email",
                    "text": "user@example.com",
                    "clear_first": true
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_type_with_field_does_not_focus_before_find() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = make_server_with_recording_action(calls.clone());
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "field": "Email",
                    "text": "user@example.com",
                    "window": {
                        "app": "Safari",
                        "windowId": "win_42"
                    }
                }
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 0);
    }

    #[tokio::test]
    async fn test_tool_call_press_by_name() {
        let server = make_server();
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_press_by_name",
                "arguments": {
                    "el_description": "Submit",
                    "key": "Return"
                }
            })),
        );
        let resp = server.handle_request(req).await;
        assert!(resp.error.is_none());
    }

    #[tokio::test]
    async fn test_tool_call_press_by_name_does_not_focus_before_find() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let server = make_server_with_recording_action(calls.clone());
        let req = make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_press_by_name",
                "arguments": {
                    "el_description": "Submit",
                    "key": "Return",
                    "window": {
                        "app": "Safari",
                        "windowId": "win_42"
                    }
                }
            })),
        );
        let resp = server.handle_request(req).await;

        assert!(resp.error.is_none());
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 0);
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
