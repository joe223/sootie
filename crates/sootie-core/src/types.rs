use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SootieError {
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("platform error: {0}")]
    Platform(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type SootieResult<T> = Result<T, SootieError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    pub fn center(&self) -> Point {
        Point {
            x: self.x + self.width / 2.0,
            y: self.y + self.height / 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub app_id: Option<String>,
    pub platform_app_id: Option<String>,
    pub pid: Option<u32>,
    pub bundle_id: Option<String>,
    pub is_frontmost: bool,
    pub windows: Vec<WindowInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowInfo {
    pub id: Option<String>,
    pub title: String,
    pub bounds: Option<Bounds>,
    pub focused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElementInfo {
    pub id: Option<String>,
    pub role: String,
    pub title: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub bounds: Option<Bounds>,
    pub actions: Vec<String>,
    pub editable: Option<bool>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextSnapshot {
    pub app: Option<String>,
    pub app_id: Option<String>,
    pub platform_app_id: Option<String>,
    pub bundle_id: Option<String>,
    pub pid: Option<u32>,
    pub window: Option<String>,
    pub url: Option<String>,
    pub focused_element: Option<ElementInfo>,
    pub interactive_elements: Vec<ElementInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Screenshot {
    pub mime_type: String,
    pub data_base64: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub window_title: Option<String>,
    pub window_frame: Option<Bounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionResult {
    pub method: String,
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeDiagnostic {
    pub name: String,
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

impl ToolResult {
    pub fn ok(data: impl Serialize) -> Self {
        Self {
            success: true,
            data: Some(serde_json::to_value(data).unwrap_or(Value::Null)),
            error: None,
            suggestion: None,
            context: None,
        }
    }

    pub fn ok_with_context(data: impl Serialize, context: impl Serialize) -> Self {
        Self {
            success: true,
            data: Some(serde_json::to_value(data).unwrap_or(Value::Null)),
            error: None,
            suggestion: None,
            context: Some(serde_json::to_value(context).unwrap_or(Value::Null)),
        }
    }

    pub fn empty_ok() -> Self {
        Self {
            success: true,
            data: Some(json!({})),
            error: None,
            suggestion: None,
            context: None,
        }
    }

    pub fn error(error: impl Into<String>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(error.into()),
            suggestion: None,
            context: None,
        }
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FindQuery {
    pub query: Option<String>,
    pub role: Option<String>,
    pub dom_id: Option<String>,
    pub dom_class: Option<String>,
    pub identifier: Option<String>,
    pub app: Option<String>,
    pub depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowCommand {
    List,
    Focus,
    Minimize,
    Maximize,
    Restore,
    Close,
    Move,
    Resize,
}
