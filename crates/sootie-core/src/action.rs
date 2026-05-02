use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::selector::{AppSelector, Coordinate, Element, Selector};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ActionTarget {
    Coordinate(Coordinate),
    Selector(Selector),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickAction {
    pub target: ActionTarget,
    #[serde(default)]
    pub button: Option<MouseButton>,
    #[serde(default)]
    pub count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeAction {
    pub target: Option<ActionTarget>,
    pub text: String,
    #[serde(default)]
    pub clear_first: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressAction {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyAction {
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrollAction {
    pub target: Option<ActionTarget>,
    pub direction: ScrollDirection,
    #[serde(default)]
    pub amount: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverAction {
    pub target: ActionTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DragAction {
    pub from: ActionTarget,
    pub to: ActionTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum WindowOperation {
    Minimize,
    Maximize,
    Close,
    Move { x: f64, y: f64 },
    Resize { width: f64, height: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FocusAction {
    pub selector: Selector,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAction {
    pub app: AppSelector,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowAction {
    pub selector: Selector,
    pub operation: WindowOperation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    #[serde(default)]
    pub element: Option<Element>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub backend_used: Option<String>,
}

impl ActionResult {
    pub fn success(element: Option<Element>, backend: &str) -> Self {
        Self {
            success: true,
            element,
            error: None,
            backend_used: Some(backend.to_string()),
        }
    }

    pub fn failure(error: &str) -> Self {
        Self {
            success: false,
            element: None,
            error: Some(error.to_string()),
            backend_used: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("target not found: {0}")]
    TargetNotFound(String),

    #[error("action failed: {0}")]
    ActionFailed(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("platform error: {0}")]
    PlatformError(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[async_trait]
pub trait ActionProvider: Send + Sync {
    async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError>;

    async fn r#type(&self, action: &TypeAction) -> Result<ActionResult, ActionError>;

    async fn press(&self, action: &PressAction) -> Result<ActionResult, ActionError>;

    async fn hotkey(&self, action: &HotkeyAction) -> Result<ActionResult, ActionError>;

    async fn scroll(&self, action: &ScrollAction) -> Result<ActionResult, ActionError>;

    async fn hover(&self, action: &HoverAction) -> Result<ActionResult, ActionError>;

    async fn drag(&self, action: &DragAction) -> Result<ActionResult, ActionError>;

    async fn focus(&self, action: &FocusAction) -> Result<ActionResult, ActionError>;

    async fn launch(&self, action: &LaunchAction) -> Result<ActionResult, ActionError>;

    async fn window_op(&self, action: &WindowAction) -> Result<ActionResult, ActionError>;
}

pub struct StubActionProvider;

#[async_trait]
impl ActionProvider for StubActionProvider {
    async fn click(&self, _action: &ClickAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn r#type(&self, _action: &TypeAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn press(&self, _action: &PressAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn hotkey(&self, _action: &HotkeyAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn scroll(&self, _action: &ScrollAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn hover(&self, _action: &HoverAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn drag(&self, _action: &DragAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn focus(&self, _action: &FocusAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn launch(&self, _action: &LaunchAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }

    async fn window_op(&self, _action: &WindowAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented("stub provider".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::AppSelector;

    // ========== ActionTarget Tests ==========

    #[test]
    fn test_action_target_serialize_selector() {
        let target = ActionTarget::Selector(
            Selector::new()
                .with_app(AppSelector::from_name("Chrome"))
                .with_role("button")
                .with_name("Submit"),
        );

        let json = serde_json::to_string(&target).unwrap();
        assert!(json.contains("Chrome"));
        assert!(json.contains("button"));
    }

    #[test]
    fn test_action_target_serialize_coordinate() {
        let target = ActionTarget::Coordinate(Coordinate { x: 500.0, y: 300.0 });
        let json = serde_json::to_string(&target).unwrap();
        assert!(json.contains("500"));
        assert!(json.contains("300"));
    }

    #[test]
    fn test_action_target_deserialize_coordinate() {
        let json = r#"{"x": 100.0, "y": 200.0}"#;
        let target: ActionTarget = serde_json::from_str(json).unwrap();
        match target {
            ActionTarget::Coordinate(c) => {
                assert_eq!(c.x, 100.0);
                assert_eq!(c.y, 200.0);
            }
            _ => panic!("expected coordinate"),
        }
    }

    #[test]
    fn test_action_target_deserialize_selector() {
        let json = r#"{"role": "button", "name": "Submit"}"#;
        let target: ActionTarget = serde_json::from_str(json).unwrap();
        match target {
            ActionTarget::Selector(s) => {
                assert_eq!(s.element.role, Some("button".to_string()));
                assert_eq!(s.element.name, Some("Submit".to_string()));
            }
            _ => panic!("expected selector"),
        }
    }

    // ========== ClickAction Tests ==========

    #[test]
    fn test_click_action_deserialize_coordinate() {
        let json = r#"{
            "target": {"x": 100, "y": 200},
            "button": "left",
            "count": 1
        }"#;

        let action: ClickAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.button, Some(MouseButton::Left));
        assert_eq!(action.count, Some(1));
    }

    #[test]
    fn test_click_action_deserialize_selector() {
        let json = r#"{
            "target": {"role": "button", "name": "Submit"},
            "button": "right",
            "count": 2
        }"#;

        let action: ClickAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.button, Some(MouseButton::Right));
        assert_eq!(action.count, Some(2));
    }

    #[test]
    fn test_click_action_minimal() {
        let json = r#"{
            "target": {"x": 100, "y": 200}
        }"#;

        let action: ClickAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.button, None);
        assert_eq!(action.count, None);
    }

    // ========== TypeAction Tests ==========

    #[test]
    fn test_type_action_deserialize() {
        let json = r#"{
            "text": "hello world",
            "clear_first": true
        }"#;

        let action: TypeAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.text, "hello world");
        assert_eq!(action.clear_first, Some(true));
        assert!(action.target.is_none());
    }

    #[test]
    fn test_type_action_with_target() {
        let json = r#"{
            "target": {"role": "textfield", "name": "Email"},
            "text": "user@example.com"
        }"#;

        let action: TypeAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.text, "user@example.com");
        assert!(action.target.is_some());
        assert_eq!(action.clear_first, None);
    }

    // ========== PressAction Tests ==========

    #[test]
    fn test_press_action_deserialize() {
        let json = r#"{ "key": "Return" }"#;
        let action: PressAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.key, "Return");
    }

    #[test]
    fn test_press_action_serialize() {
        let action = PressAction {
            key: "Tab".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("Tab"));
    }

    // ========== HotkeyAction Tests ==========

    #[test]
    fn test_hotkey_action_deserialize() {
        let json = r#"{ "keys": ["Cmd", "C"] }"#;
        let action: HotkeyAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.keys, vec!["Cmd", "C"]);
    }

    #[test]
    fn test_hotkey_action_serialize() {
        let action = HotkeyAction {
            keys: vec!["Ctrl".to_string(), "Alt".to_string(), "Delete".to_string()],
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("Ctrl"));
        assert!(json.contains("Alt"));
        assert!(json.contains("Delete"));
    }

    // ========== ScrollAction Tests ==========

    #[test]
    fn test_scroll_action_deserialize() {
        let json = r#"{
            "direction": "down",
            "amount": 3
        }"#;

        let action: ScrollAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.direction, ScrollDirection::Down);
        assert_eq!(action.amount, Some(3));
    }

    #[test]
    fn test_scroll_action_all_directions() {
        for (dir_str, dir_enum) in [
            ("up", ScrollDirection::Up),
            ("down", ScrollDirection::Down),
            ("left", ScrollDirection::Left),
            ("right", ScrollDirection::Right),
        ] {
            let json = format!(r#"{{"direction": "{}"}}"#, dir_str);
            let action: ScrollAction = serde_json::from_str(&json).unwrap();
            assert_eq!(action.direction, dir_enum);
        }
    }

    // ========== HoverAction Tests ==========

    #[test]
    fn test_hover_action_deserialize() {
        let json = r#"{"target": {"x": 100, "y": 200}}"#;
        let action: HoverAction = serde_json::from_str(json).unwrap();
        match action.target {
            ActionTarget::Coordinate(c) => {
                assert_eq!(c.x, 100.0);
                assert_eq!(c.y, 200.0);
            }
            _ => panic!("expected coordinate"),
        }
    }

    // ========== DragAction Tests ==========

    #[test]
    fn test_drag_action_deserialize() {
        let json = r#"{
            "from": {"x": 100, "y": 100},
            "to": {"x": 200, "y": 200}
        }"#;

        let action: DragAction = serde_json::from_str(json).unwrap();
        match action.from {
            ActionTarget::Coordinate(c) => assert_eq!(c.x, 100.0),
            _ => panic!("expected coordinate"),
        }
        match action.to {
            ActionTarget::Coordinate(c) => assert_eq!(c.x, 200.0),
            _ => panic!("expected coordinate"),
        }
    }

    #[test]
    fn test_drag_action_with_selectors() {
        let json = r#"{
            "from": {"role": "button", "name": "Start"},
            "to": {"role": "button", "name": "End"}
        }"#;

        let action: DragAction = serde_json::from_str(json).unwrap();
        match action.from {
            ActionTarget::Selector(s) => assert_eq!(s.element.name, Some("Start".to_string())),
            _ => panic!("expected selector"),
        }
        match action.to {
            ActionTarget::Selector(s) => assert_eq!(s.element.name, Some("End".to_string())),
            _ => panic!("expected selector"),
        }
    }

    // ========== WindowOperation Tests ==========

    #[test]
    fn test_window_operation_minimize() {
        let op = WindowOperation::Minimize;
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, "\"minimize\"");
    }

    #[test]
    fn test_window_operation_maximize() {
        let op = WindowOperation::Maximize;
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, "\"maximize\"");
    }

    #[test]
    fn test_window_operation_close() {
        let op = WindowOperation::Close;
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(json, "\"close\"");
    }

    #[test]
    fn test_window_operation_move() {
        let op = WindowOperation::Move { x: 100.0, y: 200.0 };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("move"));
        assert!(json.contains("100"));
        assert!(json.contains("200"));
    }

    #[test]
    fn test_window_operation_resize() {
        let op = WindowOperation::Resize {
            width: 800.0,
            height: 600.0,
        };
        let json = serde_json::to_string(&op).unwrap();
        assert!(json.contains("resize"));
        assert!(json.contains("800"));
        assert!(json.contains("600"));
    }

    #[test]
    fn test_window_operation_deserialize() {
        let json = "\"minimize\"";
        let op: WindowOperation = serde_json::from_str(json).unwrap();
        assert_eq!(op, WindowOperation::Minimize);
    }

    #[test]
    fn test_window_operation_move_deserialize() {
        let json = r#"{"move": {"x": 100, "y": 200}}"#;
        let op: WindowOperation = serde_json::from_str(json).unwrap();
        assert_eq!(op, WindowOperation::Move { x: 100.0, y: 200.0 });
    }

    // ========== FocusAction Tests ==========

    #[test]
    fn test_focus_action_deserialize() {
        let json = r#"{"selector": {"app": "Chrome"}}"#;
        let action: FocusAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.selector.app.unwrap().name, Some("Chrome".to_string()));
    }

    // ========== WindowAction Tests ==========

    #[test]
    fn test_window_action_deserialize() {
        let json = r#"{
            "selector": {"app": "Chrome"},
            "operation": "minimize"
        }"#;
        let action: WindowAction = serde_json::from_str(json).unwrap();
        assert_eq!(action.operation, WindowOperation::Minimize);
    }

    // ========== ActionResult Tests ==========

    #[test]
    fn test_action_result_success() {
        let result = ActionResult::success(None, "at_tree");
        assert!(result.success);
        assert_eq!(result.backend_used, Some("at_tree".to_string()));
        assert!(result.error.is_none());
        assert!(result.element.is_none());
    }

    #[test]
    fn test_action_result_failure() {
        let result = ActionResult::failure("element not found");
        assert!(!result.success);
        assert_eq!(result.error, Some("element not found".to_string()));
        assert!(result.backend_used.is_none());
    }

    #[test]
    fn test_action_result_serialize() {
        let result = ActionResult::success(None, "cgevent");
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("true"));
        assert!(json.contains("cgevent"));
    }

    #[test]
    fn test_action_result_deserialize() {
        let json = r#"{
            "success": true,
            "backend_used": "at_tree"
        }"#;
        let result: ActionResult = serde_json::from_str(json).unwrap();
        assert!(result.success);
        assert_eq!(result.backend_used, Some("at_tree".to_string()));
    }

    // ========== ActionError Tests ==========

    #[test]
    fn test_error_target_not_found() {
        let err = ActionError::TargetNotFound("button Submit".to_string());
        assert!(err.to_string().contains("target not found"));
    }

    #[test]
    fn test_error_action_failed() {
        let err = ActionError::ActionFailed("click failed".to_string());
        assert!(err.to_string().contains("action failed"));
    }

    #[test]
    fn test_error_invalid_input() {
        let err = ActionError::InvalidInput("missing target".to_string());
        assert!(err.to_string().contains("invalid input"));
    }

    #[test]
    fn test_error_platform_error() {
        let err = ActionError::PlatformError("permission denied".to_string());
        assert!(err.to_string().contains("platform error"));
    }

    #[test]
    fn test_error_not_implemented() {
        let err = ActionError::NotImplemented("stub".to_string());
        assert!(err.to_string().contains("not implemented"));
    }

    // ========== MouseButton Tests ==========

    #[test]
    fn test_mouse_button_serialize() {
        assert_eq!(serde_json::to_string(&MouseButton::Left).unwrap(), "\"left\"");
        assert_eq!(serde_json::to_string(&MouseButton::Right).unwrap(), "\"right\"");
        assert_eq!(serde_json::to_string(&MouseButton::Middle).unwrap(), "\"middle\"");
    }

    #[test]
    fn test_mouse_button_deserialize() {
        let left: MouseButton = serde_json::from_str("\"left\"").unwrap();
        assert_eq!(left, MouseButton::Left);

        let right: MouseButton = serde_json::from_str("\"right\"").unwrap();
        assert_eq!(right, MouseButton::Right);

        let middle: MouseButton = serde_json::from_str("\"middle\"").unwrap();
        assert_eq!(middle, MouseButton::Middle);
    }

    // ========== ScrollDirection Tests ==========

    #[test]
    fn test_scroll_direction_serialize() {
        assert_eq!(serde_json::to_string(&ScrollDirection::Up).unwrap(), "\"up\"");
        assert_eq!(serde_json::to_string(&ScrollDirection::Down).unwrap(), "\"down\"");
        assert_eq!(serde_json::to_string(&ScrollDirection::Left).unwrap(), "\"left\"");
        assert_eq!(serde_json::to_string(&ScrollDirection::Right).unwrap(), "\"right\"");
    }

    #[test]
    fn test_scroll_direction_deserialize() {
        let up: ScrollDirection = serde_json::from_str("\"up\"").unwrap();
        assert_eq!(up, ScrollDirection::Up);

        let down: ScrollDirection = serde_json::from_str("\"down\"").unwrap();
        assert_eq!(down, ScrollDirection::Down);

        let left: ScrollDirection = serde_json::from_str("\"left\"").unwrap();
        assert_eq!(left, ScrollDirection::Left);

        let right: ScrollDirection = serde_json::from_str("\"right\"").unwrap();
        assert_eq!(right, ScrollDirection::Right);
    }

    // ========== StubActionProvider Tests ==========

    #[tokio::test]
    async fn test_stub_click() {
        let provider = StubActionProvider;
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
            button: Some(MouseButton::Left),
            count: Some(1),
        };
        assert!(provider.click(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_type() {
        let provider = StubActionProvider;
        let action = TypeAction {
            target: None,
            text: "hello".to_string(),
            clear_first: Some(false),
        };
        assert!(provider.r#type(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_press() {
        let provider = StubActionProvider;
        let action = PressAction {
            key: "Return".to_string(),
        };
        assert!(provider.press(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_hotkey() {
        let provider = StubActionProvider;
        let action = HotkeyAction {
            keys: vec!["Cmd".to_string(), "C".to_string()],
        };
        assert!(provider.hotkey(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_scroll() {
        let provider = StubActionProvider;
        let action = ScrollAction {
            target: None,
            direction: ScrollDirection::Down,
            amount: Some(3),
        };
        assert!(provider.scroll(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_hover() {
        let provider = StubActionProvider;
        let action = HoverAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
        };
        assert!(provider.hover(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_drag() {
        let provider = StubActionProvider;
        let action = DragAction {
            from: ActionTarget::Coordinate(Coordinate { x: 0.0, y: 0.0 }),
            to: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 100.0 }),
        };
        assert!(provider.drag(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_focus() {
        let provider = StubActionProvider;
        let action = FocusAction {
            selector: Selector::new().with_app(AppSelector::from_name("Chrome")),
        };
        assert!(provider.focus(&action).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_window_op() {
        let provider = StubActionProvider;
        let action = WindowAction {
            selector: Selector::new().with_app(AppSelector::from_name("Chrome")),
            operation: WindowOperation::Minimize,
        };
        assert!(provider.window_op(&action).await.is_err());
    }
}
