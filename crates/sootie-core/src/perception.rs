use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use crate::selector::DeepInspection;
use crate::selector::{App, Bounds, Element, ResolvedTarget, Selector, Window};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Context {
    pub apps: Vec<AppContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppContext {
    pub app: App,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FindAppsResult {
    pub apps: Vec<AppInfo>,
    pub total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenshotData {
    pub format: ScreenshotFormat,
    pub data: Vec<u8>,
    pub bounds: Option<Bounds>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotFormat {
    Png,
    Jpeg,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitCondition {
    pub state: std::collections::HashMap<String, serde_json::Value>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WaitResult {
    pub matched: bool,
    pub element: Option<Element>,
    pub timed_out: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PerceptionError {
    #[error("target not found: {0}")]
    TargetNotFound(String),

    #[error("platform error: {0}")]
    PlatformError(String),

    #[error("timeout waiting for target: {0}")]
    Timeout(String),

    #[error("screenshot failed: {0}")]
    ScreenshotFailed(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[async_trait]
pub trait PerceptionProvider: Send + Sync {
    async fn get_context(&self) -> Result<Context, PerceptionError>;

    async fn find(&self, selector: &Selector) -> Result<ResolvedTarget, PerceptionError>;

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError>;

    async fn wait(
        &self,
        selector: &Selector,
        condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError>;

    async fn screenshot(
        &self,
        target: Option<&Selector>,
        region: Option<&Bounds>,
        display_id: Option<u32>,
    ) -> Result<ScreenshotData, PerceptionError>;

    async fn find_apps(
        &self,
        pattern: &str,
        limit: Option<u32>,
    ) -> Result<FindAppsResult, PerceptionError>;
}

pub struct StubPerceptionProvider;

#[async_trait]
impl PerceptionProvider for StubPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        Ok(Context { apps: vec![] })
    }

    async fn find(&self, _selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
        Err(PerceptionError::NotImplemented("stub provider".to_string()))
    }

    async fn inspect(&self, _selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        Err(PerceptionError::NotImplemented("stub provider".to_string()))
    }

    async fn wait(
        &self,
        _selector: &Selector,
        _condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        Err(PerceptionError::NotImplemented("stub provider".to_string()))
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        _region: Option<&Bounds>,
        _display_id: Option<u32>,
    ) -> Result<ScreenshotData, PerceptionError> {
        Err(PerceptionError::NotImplemented("stub provider".to_string()))
    }

    async fn find_apps(
        &self,
        _pattern: &str,
        _limit: Option<u32>,
    ) -> Result<FindAppsResult, PerceptionError> {
        Err(PerceptionError::NotImplemented("stub provider".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::{Bounds, ElementState};

    // ========== Context Tests ==========

    #[test]
    fn test_context_empty() {
        let ctx = Context { apps: vec![] };
        assert!(ctx.apps.is_empty());
    }

    #[test]
    fn test_context_serialize() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Chrome".to_string(),
                    bundle_id: "com.google.Chrome".to_string(),
                    is_frontmost: true,
                },
                windows: vec![Window {
                    id: "win_1".to_string(),
                    title: "Gmail".to_string(),
                    index: 0,
                    focused: true,
                    bounds: Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1440.0,
                        height: 900.0,
                    },
                }],
            }],
        };

        let json = serde_json::to_string_pretty(&ctx).unwrap();
        assert!(json.contains("Chrome"));
        assert!(json.contains("Gmail"));

        let deserialized: Context = serde_json::from_str(&json).unwrap();
        assert_eq!(ctx, deserialized);
    }

    #[test]
    fn test_context_multiple_apps() {
        let ctx = Context {
            apps: vec![
                AppContext {
                    app: App {
                        name: "Chrome".to_string(),
                        bundle_id: "com.google.Chrome".to_string(),
                        is_frontmost: true,
                    },
                    windows: vec![],
                },
                AppContext {
                    app: App {
                        name: "Firefox".to_string(),
                        bundle_id: "org.mozilla.firefox".to_string(),
                        is_frontmost: false,
                    },
                    windows: vec![],
                },
            ],
        };
        assert_eq!(ctx.apps.len(), 2);
        assert_eq!(ctx.apps[0].app.name, "Chrome");
        assert_eq!(ctx.apps[1].app.name, "Firefox");
    }

    #[test]
    fn test_context_deserialize_empty() {
        let json = r#"{"apps": []}"#;
        let ctx: Context = serde_json::from_str(json).unwrap();
        assert!(ctx.apps.is_empty());
    }

    // ========== ScreenshotData Tests ==========

    #[test]
    fn test_screenshot_data_png() {
        let data = ScreenshotData {
            format: ScreenshotFormat::Png,
            data: vec![0x89, 0x50, 0x4E, 0x47],
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
            }),
        };

        let json = serde_json::to_string(&data).unwrap();
        let deserialized: ScreenshotData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, deserialized);
    }

    #[test]
    fn test_screenshot_data_jpeg() {
        let data = ScreenshotData {
            format: ScreenshotFormat::Jpeg,
            data: vec![0xFF, 0xD8, 0xFF],
            bounds: None,
        };

        let json = serde_json::to_string(&data).unwrap();
        assert!(json.contains("jpeg"));
        let deserialized: ScreenshotData = serde_json::from_str(&json).unwrap();
        assert_eq!(data, deserialized);
    }

    #[test]
    fn test_screenshot_format_serialize() {
        assert_eq!(
            serde_json::to_string(&ScreenshotFormat::Png).unwrap(),
            "\"png\""
        );
        assert_eq!(
            serde_json::to_string(&ScreenshotFormat::Jpeg).unwrap(),
            "\"jpeg\""
        );
    }

    #[test]
    fn test_screenshot_format_deserialize() {
        let png: ScreenshotFormat = serde_json::from_str("\"png\"").unwrap();
        assert_eq!(png, ScreenshotFormat::Png);

        let jpeg: ScreenshotFormat = serde_json::from_str("\"jpeg\"").unwrap();
        assert_eq!(jpeg, ScreenshotFormat::Jpeg);
    }

    // ========== WaitCondition Tests ==========

    #[test]
    fn test_wait_condition_deserialize() {
        let json = r#"{
            "state": { "visible": true, "enabled": true },
            "timeout_ms": 5000
        }"#;

        let condition: WaitCondition = serde_json::from_str(json).unwrap();
        assert_eq!(condition.timeout_ms, 5000);
        assert_eq!(
            condition.state.get("visible").unwrap(),
            &serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn test_wait_condition_empty_state() {
        let json = r#"{
            "state": {},
            "timeout_ms": 1000
        }"#;

        let condition: WaitCondition = serde_json::from_str(json).unwrap();
        assert_eq!(condition.timeout_ms, 1000);
        assert!(condition.state.is_empty());
    }

    #[test]
    fn test_wait_condition_serialize() {
        let mut state = std::collections::HashMap::new();
        state.insert("visible".to_string(), serde_json::Value::Bool(true));

        let condition = WaitCondition {
            state,
            timeout_ms: 3000,
        };

        let json = serde_json::to_string(&condition).unwrap();
        assert!(json.contains("3000"));
        assert!(json.contains("visible"));
    }

    // ========== WaitResult Tests ==========

    #[test]
    fn test_wait_result_matched() {
        let result = WaitResult {
            matched: true,
            element: Some(Element {
                role: "button".to_string(),
                name: "Submit".to_string(),
                text: None,
                id: None,
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: Some(true),
                },
                bounds: Bounds {
                    x: 10.0,
                    y: 20.0,
                    width: 100.0,
                    height: 30.0,
                },
                index: 0,
            }),
            timed_out: false,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: WaitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_wait_result_timed_out() {
        let result = WaitResult {
            matched: false,
            element: None,
            timed_out: true,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("true"));
        let deserialized: WaitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_wait_result_no_match() {
        let result = WaitResult {
            matched: false,
            element: None,
            timed_out: false,
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: WaitResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    // ========== PerceptionError Tests ==========

    #[test]
    fn test_error_target_not_found() {
        let err = PerceptionError::TargetNotFound("button Submit".to_string());
        assert!(err.to_string().contains("target not found"));
        assert!(err.to_string().contains("button Submit"));
    }

    #[test]
    fn test_error_platform_error() {
        let err = PerceptionError::PlatformError("access denied".to_string());
        assert!(err.to_string().contains("platform error"));
        assert!(err.to_string().contains("access denied"));
    }

    #[test]
    fn test_error_timeout() {
        let err = PerceptionError::Timeout("element not visible".to_string());
        assert!(err.to_string().contains("timeout"));
        assert!(err.to_string().contains("element not visible"));
    }

    #[test]
    fn test_error_screenshot_failed() {
        let err = PerceptionError::ScreenshotFailed("no display".to_string());
        assert!(err.to_string().contains("screenshot failed"));
        assert!(err.to_string().contains("no display"));
    }

    #[test]
    fn test_error_not_implemented() {
        let err = PerceptionError::NotImplemented("stub provider".to_string());
        assert!(err.to_string().contains("not implemented"));
        assert!(err.to_string().contains("stub provider"));
    }

    // ========== StubPerceptionProvider Tests ==========

    #[tokio::test]
    async fn test_stub_context_returns_empty() {
        let provider = StubPerceptionProvider;
        let ctx = provider.get_context().await.unwrap();
        assert!(ctx.apps.is_empty());
    }

    #[tokio::test]
    async fn test_stub_find_returns_not_implemented() {
        let provider = StubPerceptionProvider;
        let selector = Selector::new().with_role("button");
        let result = provider.find(&selector).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stub_inspect_returns_not_implemented() {
        let provider = StubPerceptionProvider;
        let selector = Selector::new().with_role("button");
        let result = provider.inspect(&selector).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stub_wait_returns_not_implemented() {
        let provider = StubPerceptionProvider;
        let selector = Selector::new().with_role("button");
        let condition = WaitCondition {
            state: std::collections::HashMap::new(),
            timeout_ms: 1000,
        };
        let result = provider.wait(&selector, &condition).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_stub_screenshot_returns_not_implemented() {
        let provider = StubPerceptionProvider;
        let result = provider.screenshot(None, None).await;
        assert!(result.is_err());
    }

    // ========== AppContext Tests ==========

    #[test]
    fn test_app_context_serialize() {
        let app_ctx = AppContext {
            app: App {
                name: "Safari".to_string(),
                bundle_id: "com.apple.Safari".to_string(),
                is_frontmost: false,
            },
            windows: vec![
                Window {
                    id: "win_1".to_string(),
                    title: "Apple".to_string(),
                    index: 0,
                    focused: true,
                    bounds: Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1440.0,
                        height: 900.0,
                    },
                },
                Window {
                    id: "win_2".to_string(),
                    title: "GitHub".to_string(),
                    index: 1,
                    focused: false,
                    bounds: Bounds {
                        x: 100.0,
                        y: 100.0,
                        width: 800.0,
                        height: 600.0,
                    },
                },
            ],
        };

        let json = serde_json::to_string(&app_ctx).unwrap();
        let deserialized: AppContext = serde_json::from_str(&json).unwrap();
        assert_eq!(app_ctx, deserialized);
    }
}
