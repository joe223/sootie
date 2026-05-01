use sootie_core::action::{
    ActionError, ActionResult, ClickAction, DragAction, FocusAction, HotkeyAction,
    HoverAction, LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction,
    ActionProvider,
};
use sootie_core::perception::{
    AppContext, Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use sootie_core::selector::*;
use sootie_mcp::server::SootieServer;
use sootie_mcp::types::JsonRpcRequest;

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================
// Mock Perception Provider - returns realistic data
// ============================================================

struct MockPerceptionProvider {
    context: Context,
    find_results: Vec<ResolvedTarget>,
    call_log: Arc<Mutex<Vec<String>>>,
}

impl MockPerceptionProvider {
    fn new(context: Context, find_results: Vec<ResolvedTarget>) -> Self {
        Self {
            context,
            find_results,
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn gmail_example() -> Self {
        let context = Context {
            apps: vec![
                AppContext {
                    app: App {
                        name: "Google Chrome".to_string(),
                        bundle_id: "com.google.Chrome".to_string(),
                        is_frontmost: true,
                    },
                    windows: vec![
                        Window {
                            id: "win_1042".to_string(),
                            title: "Inbox - user@gmail.com - Gmail".to_string(),
                            index: 0,
                            focused: true,
                            bounds: Bounds {
                                x: 0.0,
                                y: 25.0,
                                width: 1440.0,
                                height: 875.0,
                            },
                        },
                        Window {
                            id: "win_1043".to_string(),
                            title: "GitHub".to_string(),
                            index: 1,
                            focused: false,
                            bounds: Bounds {
                                x: 100.0,
                                y: 100.0,
                                width: 1200.0,
                                height: 800.0,
                            },
                        },
                    ],
                },
                AppContext {
                    app: App {
                        name: "Finder".to_string(),
                        bundle_id: "com.apple.finder".to_string(),
                        is_frontmost: false,
                    },
                    windows: vec![Window {
                        id: "win_2001".to_string(),
                        title: "Documents".to_string(),
                        index: 0,
                        focused: false,
                        bounds: Bounds {
                            x: 200.0,
                            y: 50.0,
                            width: 800.0,
                            height: 600.0,
                        },
                    }],
                },
            ],
        };

        let compose_button = ResolvedTarget {
            status: MatchStatus::Unique,
            total_matches: 1,
            app: Some(App {
                name: "Google Chrome".to_string(),
                bundle_id: "com.google.Chrome".to_string(),
                is_frontmost: true,
            }),
            window: Some(Window {
                id: "win_1042".to_string(),
                title: "Inbox - user@gmail.com - Gmail".to_string(),
                index: 0,
                focused: true,
                bounds: Bounds {
                    x: 0.0,
                    y: 25.0,
                    width: 1440.0,
                    height: 875.0,
                },
            }),
            elements: vec![Element {
                role: "button".to_string(),
                name: "Compose".to_string(),
                text: None,
                id: Some("dom_compose_btn".to_string()),
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: Some(true),
                },
                bounds: Bounds {
                    x: 120.0,
                    y: 85.0,
                    width: 100.0,
                    height: 36.0,
                },
                index: 0,
            }],
        };

        let to_field = ResolvedTarget {
            status: MatchStatus::Unique,
            total_matches: 1,
            app: Some(App {
                name: "Google Chrome".to_string(),
                bundle_id: "com.google.Chrome".to_string(),
                is_frontmost: true,
            }),
            window: Some(Window {
                id: "win_1042".to_string(),
                title: "New Message - Gmail".to_string(),
                index: 0,
                focused: true,
                bounds: Bounds {
                    x: 0.0,
                    y: 25.0,
                    width: 1440.0,
                    height: 875.0,
                },
            }),
            elements: vec![Element {
                role: "textfield".to_string(),
                name: "To".to_string(),
                text: Some("".to_string()),
                id: Some("to_field".to_string()),
                state: ElementState {
                    visible: true,
                    focused: Some(true),
                    enabled: Some(true),
                },
                bounds: Bounds {
                    x: 200.0,
                    y: 150.0,
                    width: 500.0,
                    height: 30.0,
                },
                index: 0,
            }],
        };

        let multiple_buttons = ResolvedTarget {
            status: MatchStatus::Multiple,
            total_matches: 3,
            app: None,
            window: None,
            elements: vec![
                Element {
                    role: "button".to_string(),
                    name: "OK".to_string(),
                    text: None,
                    id: None,
                    state: ElementState { visible: true, focused: None, enabled: Some(true) },
                    bounds: Bounds { x: 100.0, y: 200.0, width: 80.0, height: 30.0 },
                    index: 0,
                },
                Element {
                    role: "button".to_string(),
                    name: "OK".to_string(),
                    text: None,
                    id: None,
                    state: ElementState { visible: true, focused: None, enabled: Some(true) },
                    bounds: Bounds { x: 100.0, y: 300.0, width: 80.0, height: 30.0 },
                    index: 1,
                },
                Element {
                    role: "button".to_string(),
                    name: "OK".to_string(),
                    text: None,
                    id: None,
                    state: ElementState { visible: true, focused: None, enabled: Some(true) },
                    bounds: Bounds { x: 100.0, y: 400.0, width: 80.0, height: 30.0 },
                    index: 2,
                },
            ],
        };

        Self::new(context, vec![compose_button, to_field, multiple_buttons])
    }
}

#[async_trait]
impl PerceptionProvider for MockPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        self.call_log.lock().await.push("get_context".to_string());
        Ok(self.context.clone())
    }

    async fn find(&self, selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
        self.call_log.lock().await.push(format!(
            "find:role={:?},name={:?}",
            selector.element.role, selector.element.name
        ));

        if selector.element.name.as_deref() == Some("Compose") {
            Ok(self.find_results[0].clone())
        } else if selector.element.name.as_deref() == Some("To") {
            Ok(self.find_results[1].clone())
        } else if selector.element.name.as_deref() == Some("OK") {
            Ok(self.find_results[2].clone())
        } else {
            Err(PerceptionError::TargetNotFound(format!(
                "No element with role={:?}, name={:?}",
                selector.element.role, selector.element.name
            )))
        }
    }

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        self.call_log.lock().await.push("inspect".to_string());
        let result = self.find(selector).await?;
        let element = result.elements.into_iter().next().unwrap();
        Ok(DeepInspection {
            element,
            children: vec![],
            backend: "at_tree".to_string(),
            actions: vec!["click".to_string(), "hover".to_string()],
            raw_metadata: Some(serde_json::json!({"role": "AXButton"})),
        })
    }

    async fn wait(
        &self,
        selector: &Selector,
        condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        self.call_log.lock().await.push(format!(
            "wait:timeout={}",
            condition.timeout_ms
        ));
        let result = self.find(selector).await?;
        Ok(WaitResult {
            matched: true,
            element: result.elements.into_iter().next(),
            timed_out: false,
        })
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        _region: Option<&Bounds>,
    ) -> Result<ScreenshotData, PerceptionError> {
        self.call_log.lock().await.push("screenshot".to_string());
        Ok(ScreenshotData {
            format: sootie_core::perception::ScreenshotFormat::Png,
            data: vec![0x89, 0x50, 0x4E, 0x47],
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }),
        })
    }
}

// ============================================================
// Mock Action Provider - records calls
// ============================================================

struct MockActionProvider {
    call_log: Arc<Mutex<Vec<String>>>,
}

impl MockActionProvider {
    fn new() -> Self {
        Self {
            call_log: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl ActionProvider for MockActionProvider {
    async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!(
            "click:button={:?},count={:?}",
            action.button, action.count
        ));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn r#type(&self, action: &TypeAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!(
            "type:text={},clear_first={:?}",
            action.text, action.clear_first
        ));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn press(&self, action: &PressAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!("press:key={}", action.key));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn hotkey(&self, action: &HotkeyAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!("hotkey:keys={:?}", action.keys));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn scroll(&self, action: &ScrollAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!(
            "scroll:dir={:?},amount={:?}",
            action.direction, action.amount
        ));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn hover(&self, _action: &HoverAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push("hover".to_string());
        Ok(ActionResult::success(None, "mock"))
    }

    async fn drag(&self, _action: &DragAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push("drag".to_string());
        Ok(ActionResult::success(None, "mock"))
    }

    async fn focus(&self, _action: &FocusAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push("focus".to_string());
        Ok(ActionResult::success(None, "mock"))
    }

    async fn window_op(&self, action: &WindowAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!(
            "window_op:{:?}",
            action.operation
        ));
        Ok(ActionResult::success(None, "mock"))
    }

    async fn launch(&self, action: &LaunchAction) -> Result<ActionResult, ActionError> {
        self.call_log.lock().await.push(format!(
            "launch:app={:?}",
            action.app.name
        ));
        Ok(ActionResult::success(None, "mock"))
    }
}

// ============================================================
// Helper
// ============================================================

fn make_request(method: &str, id: i64, params: Option<serde_json::Value>) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::Value::Number(id.into())),
        method: method.to_string(),
        params,
    }
}

async fn make_server() -> (SootieServer, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vec<String>>>) {
    let perception_log = Arc::new(Mutex::new(Vec::new()));
    let action_log = Arc::new(Mutex::new(Vec::new()));

    let example = MockPerceptionProvider::gmail_example();

    let perception = MockPerceptionProvider {
        context: example.context,
        find_results: example.find_results,
        call_log: perception_log.clone(),
    };
    let action = MockActionProvider {
        call_log: action_log.clone(),
    };

    (SootieServer::new(Box::new(perception), Box::new(action)), perception_log, action_log)
}

// ============================================================
// E2E Tests
// ============================================================

#[tokio::test]
async fn e2e_full_mcp_handshake() {
    let (server, _, _) = make_server().await;

    // Step 1: Initialize
    let resp = server
        .handle_request(make_request("initialize", 1, None))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["serverInfo"]["name"], "sootie");
    assert_eq!(result["protocolVersion"], "2024-11-05");
    assert!(result["capabilities"]["tools"].is_object());

    // Step 2: Initialized notification
    let resp = server
        .handle_request(make_request("notifications/initialized", 2, None))
        .await;
    assert!(resp.error.is_none());

    // Step 3: List tools
    let resp = server
        .handle_request(make_request("tools/list", 3, None))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let tools = result["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 19);

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"sootie_context"));
    assert!(tool_names.contains(&"sootie_find"));
    assert!(tool_names.contains(&"sootie_click"));
    assert!(tool_names.contains(&"sootie_recipe_save"));
}

#[tokio::test]
async fn e2e_context_returns_app_tree() {
    let (server, p_log, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_context",
                "arguments": {}
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], serde_json::Value::Null);

    let content: Context = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(content.apps.len(), 2);
    assert_eq!(content.apps[0].app.name, "Google Chrome");
    assert_eq!(content.apps[0].windows.len(), 2);
    assert_eq!(content.apps[1].app.name, "Finder");

    let logs = p_log.lock().await;
    assert!(logs.contains(&"get_context".to_string()));
}

#[tokio::test]
async fn e2e_find_returns_matching_elements() {
    let (server, p_log, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "app": "Chrome",
                    "window": "Gmail",
                    "role": "button",
                    "name": "Compose"
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let target: ResolvedTarget =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();

    assert_eq!(target.status, MatchStatus::Unique);
    assert_eq!(target.total_matches, 1);
    assert_eq!(target.elements[0].name, "Compose");
    assert_eq!(target.elements[0].role, "button");
    assert!(target.app.is_some());
    assert!(target.window.is_some());

    let logs = p_log.lock().await;
    assert!(logs.iter().any(|l| l.starts_with("find:")));
}

#[tokio::test]
async fn e2e_find_multiple_matches() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "role": "button",
                    "name": "OK"
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let target: ResolvedTarget =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();

    assert_eq!(target.status, MatchStatus::Multiple);
    assert_eq!(target.total_matches, 3);
    assert_eq!(target.elements.len(), 3);
}

#[tokio::test]
async fn e2e_find_not_found() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "role": "button",
                    "name": "NonexistentButton"
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], true);
}

#[tokio::test]
async fn e2e_click_with_target() {
    let (server, p_log, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_click",
                "arguments": {
                    "app": "Chrome",
                    "window": "Gmail",
                    "role": "button",
                    "name": "Compose",
                    "button": "left",
                    "count": 1
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let action_result: ActionResult =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(action_result.success);
    assert_eq!(action_result.backend_used, Some("mock".to_string()));

    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.starts_with("click:")));
}

#[tokio::test]
async fn e2e_click_with_coordinate() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_click",
                "arguments": {
                    "coordinate": { "x": 500.0, "y": 300.0 },
                    "button": "right"
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("Right")), "No Right log in: {:?}", *a_logs);
}

#[tokio::test]
async fn e2e_type_into_field() {
    let (server, p_log, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "app": "Chrome",
                    "role": "textfield",
                    "name": "To",
                    "text": "user@example.com",
                    "clear_first": true
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let action_result: ActionResult =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(action_result.success);

    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("user@example.com")));
    assert!(a_logs.iter().any(|l| l.contains("true")));
}

#[tokio::test]
async fn e2e_press_key() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_press",
                "arguments": { "key": "Return" }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("Return")));
}

#[tokio::test]
async fn e2e_hotkey() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_hotkey",
                "arguments": { "keys": ["Cmd", "Shift", "S"] }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("Cmd")));
    assert!(a_logs.iter().any(|l| l.contains("Shift")));
}

#[tokio::test]
async fn e2e_scroll() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_scroll",
                "arguments": {
                    "direction": "down",
                    "amount": 5
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("Down")));
    assert!(a_logs.iter().any(|l| l.contains("5")));
}

#[tokio::test]
async fn e2e_hover() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_hover",
                "arguments": {
                    "coordinate": { "x": 100.0, "y": 200.0 }
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l == "hover"));
}

#[tokio::test]
async fn e2e_drag() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_drag",
                "arguments": {
                    "from": { "x": 100.0, "y": 100.0 },
                    "to": { "x": 200.0, "y": 200.0 }
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l == "drag"));
}

#[tokio::test]
async fn e2e_focus() {
    let (server, _, a_log) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_focus",
                "arguments": { "app": "Chrome" }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l == "focus"));
}

#[tokio::test]
async fn e2e_window_operations() {
    let (server, _, a_log) = make_server().await;

    for (op, expected) in &[
        ("minimize", "Minimize"),
        ("maximize", "Maximize"),
        ("close", "Close"),
    ] {
        a_log.lock().await.clear();

        let resp = server
            .handle_request(make_request(
                "tools/call",
                1,
                Some(serde_json::json!({
                    "name": "sootie_window",
                    "arguments": {
                        "app": "Chrome",
                        "operation": op
                    }
                })),
            ))
            .await;

        assert!(resp.error.is_none(), "Failed for {}", op);
        let a_logs = a_log.lock().await;
        assert!(a_logs.iter().any(|l| l.contains(expected)), "No log for {}", op);
    }
}

#[tokio::test]
async fn e2e_window_move_and_resize() {
    let (server, _, a_log) = make_server().await;

    // Move
    let resp = server
        .handle_request(make_request(
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
        ))
        .await;
    assert!(resp.error.is_none());

    // Resize
    let resp = server
        .handle_request(make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_window",
                "arguments": {
                    "app": "Chrome",
                    "operation": "resize",
                    "width": 800,
                    "height": 600
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.contains("Move")), "No Move log found in: {:?}", *a_logs);
    assert!(a_logs.iter().any(|l| l.contains("Resize")), "No Resize log found in: {:?}", *a_logs);
}

#[tokio::test]
async fn e2e_recipe_lifecycle() {
    let (server, _, _) = make_server().await;

    // 1. Save a recipe
    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "e2e-test-recipe",
                        "platforms": ["macos", "linux"],
                        "params": [
                            { "name": "email", "type": "string", "required": true },
                            { "name": "subject", "type": "string", "required": false, "default": "Hello" }
                        ],
                        "steps": [
                            { "action": "click", "target": { "role": "button", "name": "Compose" } },
                            { "action": "type", "target": { "role": "textfield", "name": "To" }, "text": "${email}" },
                            { "action": "type", "target": { "role": "textfield", "name": "Subject" }, "text": "${subject}" }
                        ]
                    }
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], serde_json::Value::Null);
    assert!(result["content"][0]["text"].as_str().unwrap().contains("saved"));

    // 2. List recipes
    let resp = server
        .handle_request(make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_recipes",
                "arguments": {}
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert!(result["content"][0]["text"].as_str().unwrap().contains("e2e-test-recipe"));

    // 3. Run recipe
    let resp = server
        .handle_request(make_request(
            "tools/call",
            3,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "e2e-test-recipe",
                    "params": { "email": "user@example.com" }
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let run_result: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(run_result["recipe"], "e2e-test-recipe");
    assert_eq!(run_result["status"], "ready");
    assert_eq!(run_result["steps"].as_array().unwrap().len(), 3);

    // 4. Delete recipe
    let resp = server
        .handle_request(make_request(
            "tools/call",
            4,
            Some(serde_json::json!({
                "name": "sootie_recipe_delete",
                "arguments": { "name": "e2e-test-recipe" }
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert!(result["content"][0]["text"].as_str().unwrap().contains("deleted"));

    // 5. Verify deletion
    let resp = server
        .handle_request(make_request(
            "tools/call",
            5,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "e2e-test-recipe",
                    "params": {}
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], true);
}

#[tokio::test]
async fn e2e_recipe_run_missing_required_param() {
    let (server, _, _) = make_server().await;

    // Save recipe
    server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "needs-params",
                        "params": [
                            { "name": "required_field", "type": "string", "required": true }
                        ],
                        "steps": [
                            { "action": "click" }
                        ]
                    }
                }
            })),
        ))
        .await;

    // Run without required param
    let resp = server
        .handle_request(make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "needs-params",
                    "params": {}
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], true);
    assert!(result["content"][0]["text"].as_str().unwrap().contains("required"));
}

#[tokio::test]
async fn e2e_recipe_with_substitution() {
    let (server, _, _) = make_server().await;

    // Save recipe with template
    server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_recipe_save",
                "arguments": {
                    "recipe": {
                        "schema_version": 3,
                        "name": "template-recipe",
                        "params": [
                            { "name": "name", "type": "string", "required": true },
                            { "name": "app", "type": "string", "required": true }
                        ],
                        "steps": [
                            { "action": "type", "text": "Hello ${name}, welcome to ${app}!" }
                        ]
                    }
                }
            })),
        ))
        .await;

    // Run with params
    let resp = server
        .handle_request(make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_run",
                "arguments": {
                    "name": "template-recipe",
                    "params": { "name": "World", "app": "Sootie" }
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let run_result: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    let step_text = run_result["steps"][0].as_str().unwrap();
    assert!(step_text.contains("Hello World, welcome to Sootie!"));
}

#[tokio::test]
async fn e2e_screenshot() {
    let (server, p_log, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_screenshot",
                "arguments": {}
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    assert_eq!(result["isError"], serde_json::Value::Null);

    let logs = p_log.lock().await;
    assert!(logs.iter().any(|l| l == "screenshot"));
}

#[tokio::test]
async fn e2e_inspect_element() {
    let (server, p_log, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_inspect",
                "arguments": {
                    "app": "Chrome",
                    "role": "button",
                    "name": "Compose"
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let inspection: DeepInspection =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();

    assert_eq!(inspection.element.name, "Compose");
    assert_eq!(inspection.backend, "at_tree");
    assert!(inspection.actions.contains(&"click".to_string()));

    let logs = p_log.lock().await;
    assert!(logs.iter().any(|l| l == "inspect"));
}

#[tokio::test]
async fn e2e_wait_for_element() {
    let (server, p_log, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_wait",
                "arguments": {
                    "role": "button",
                    "name": "Compose",
                    "timeout": 3000
                }
            })),
        ))
        .await;

    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let wait_result: WaitResult =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(wait_result.matched);
    assert!(!wait_result.timed_out);

    let logs = p_log.lock().await;
    assert!(logs.iter().any(|l| l.contains("wait:timeout=3000")));
}

#[tokio::test]
async fn e2e_unknown_method_returns_error() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request("unknown/method", 1, None))
        .await;

    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32601);
}

#[tokio::test]
async fn e2e_unknown_tool_returns_error() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_nonexistent",
                "arguments": {}
            })),
        ))
        .await;

    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32601);
}

#[tokio::test]
async fn e2e_missing_tool_name_returns_error() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "arguments": {}
            })),
        ))
        .await;

    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32602);
}

#[tokio::test]
async fn e2e_null_params_returns_error() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request("tools/call", 1, None))
        .await;

    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, -32602);
}

#[tokio::test]
async fn e2e_response_preserves_request_id() {
    let (server, _, _) = make_server().await;

    for id in [1, 42, 999] {
        let resp = server
            .handle_request(make_request("initialize", id, None))
            .await;
        assert_eq!(resp.id, Some(serde_json::Value::Number(id.into())));
    }
}

#[tokio::test]
async fn e2e_response_has_jsonrpc_version() {
    let (server, _, _) = make_server().await;

    let resp = server
        .handle_request(make_request("initialize", 1, None))
        .await;
    assert_eq!(resp.jsonrpc, "2.0");
}

#[tokio::test]
async fn e2e_full_gmail_workflow() {
    let (server, p_log, a_log) = make_server().await;

    // Step 1: Get context
    let resp = server
        .handle_request(make_request(
            "tools/call",
            1,
            Some(serde_json::json!({
                "name": "sootie_context",
                "arguments": {}
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Step 2: Find Compose button
    let resp = server
        .handle_request(make_request(
            "tools/call",
            2,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "app": "Chrome",
                    "window": "Gmail",
                    "role": "button",
                    "name": "Compose"
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());
    let result = resp.result.unwrap();
    let target: ResolvedTarget =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(target.status, MatchStatus::Unique);

    // Step 3: Click Compose
    let resp = server
        .handle_request(make_request(
            "tools/call",
            3,
            Some(serde_json::json!({
                "name": "sootie_click",
                "arguments": {
                    "app": "Chrome",
                    "window": "Gmail",
                    "role": "button",
                    "name": "Compose",
                    "button": "left",
                    "count": 1
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Step 4: Find To field
    let resp = server
        .handle_request(make_request(
            "tools/call",
            4,
            Some(serde_json::json!({
                "name": "sootie_find",
                "arguments": {
                    "role": "textfield",
                    "name": "To"
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Step 5: Type email
    let resp = server
        .handle_request(make_request(
            "tools/call",
            5,
            Some(serde_json::json!({
                "name": "sootie_type",
                "arguments": {
                    "app": "Chrome",
                    "role": "textfield",
                    "name": "To",
                    "text": "colleague@company.com",
                    "clear_first": true
                }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Step 6: Press Tab to move to subject
    let resp = server
        .handle_request(make_request(
            "tools/call",
            6,
            Some(serde_json::json!({
                "name": "sootie_press",
                "arguments": { "key": "Tab" }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Step 7: Hotkey to send (Cmd+Enter)
    let resp = server
        .handle_request(make_request(
            "tools/call",
            7,
            Some(serde_json::json!({
                "name": "sootie_hotkey",
                "arguments": { "keys": ["Cmd", "Return"] }
            })),
        ))
        .await;
    assert!(resp.error.is_none());

    // Verify all steps were logged
    let p_logs = p_log.lock().await;
    assert!(p_logs.iter().any(|l| l == "get_context"));
    assert!(p_logs.iter().filter(|l| l.starts_with("find:")).count() >= 2);

    let a_logs = a_log.lock().await;
    assert!(a_logs.iter().any(|l| l.starts_with("click:")));
    assert!(a_logs.iter().any(|l| l.contains("colleague@company.com")));
    assert!(a_logs.iter().any(|l| l.contains("Tab")));
    assert!(a_logs.iter().any(|l| l.contains("Cmd")));
}
