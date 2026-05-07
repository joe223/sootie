use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppSelector {
    pub name: Option<String>,
    pub bundle_id: Option<String>,
    pub is_frontmost: Option<bool>,
}

impl AppSelector {
    pub fn from_name(name: &str) -> Self {
        Self {
            name: Some(name.to_string()),
            bundle_id: None,
            is_frontmost: None,
        }
    }

    pub fn with_bundle_id(mut self, bundle_id: &str) -> Self {
        self.bundle_id = Some(bundle_id.to_string());
        self
    }

    pub fn with_frontmost(mut self, frontmost: bool) -> Self {
        self.is_frontmost = Some(frontmost);
        self
    }
}

impl From<&str> for AppSelector {
    fn from(s: &str) -> Self {
        Self::from_name(s)
    }
}

impl From<String> for AppSelector {
    fn from(s: String) -> Self {
        Self::from_name(&s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowSelector {
    pub title: Option<String>,
    pub id: Option<String>,
    pub index: Option<u32>,
    pub focused: Option<bool>,
}

impl WindowSelector {
    pub fn from_title(title: &str) -> Self {
        Self {
            title: Some(title.to_string()),
            id: None,
            index: None,
            focused: None,
        }
    }

    pub fn with_id(mut self, id: &str) -> Self {
        self.id = Some(id.to_string());
        self
    }

    pub fn with_index(mut self, index: u32) -> Self {
        self.index = Some(index);
        self
    }

    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }
}

impl From<&str> for WindowSelector {
    fn from(s: &str) -> Self {
        Self::from_title(s)
    }
}

impl From<String> for WindowSelector {
    fn from(s: String) -> Self {
        Self::from_title(&s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElementSelector {
    pub role: Option<String>,
    pub name: Option<String>,
    pub text: Option<String>,
    pub id: Option<String>,
    pub state: Option<WindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Selector {
    #[serde(default, deserialize_with = "deserialize_app_field")]
    pub app: Option<AppSelector>,
    #[serde(default, deserialize_with = "deserialize_window_field")]
    pub window: Option<WindowSelector>,
    #[serde(flatten)]
    pub element: ElementSelector,
}

fn deserialize_app_field<'de, D>(deserializer: D) -> Result<Option<AppSelector>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(AppSelector::from_name(&s))),
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn deserialize_window_field<'de, D>(deserializer: D) -> Result<Option<WindowSelector>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => Ok(Some(WindowSelector::from_title(&s))),
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

impl Selector {
    pub fn new() -> Self {
        Self {
            app: None,
            window: None,
            element: ElementSelector {
                role: None,
                name: None,
                text: None,
                id: None,
                state: None,
            },
        }
    }

    pub fn with_app(mut self, app: AppSelector) -> Self {
        self.app = Some(app);
        self
    }

    pub fn with_window(mut self, window: WindowSelector) -> Self {
        self.window = Some(window);
        self
    }

    pub fn with_role(mut self, role: &str) -> Self {
        self.element.role = Some(role.to_string());
        self
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.element.name = Some(name.to_string());
        self
    }

    pub fn with_text(mut self, text: &str) -> Self {
        self.element.text = Some(text.to_string());
        self
    }

    pub fn with_id(mut self, id: &str) -> Self {
        self.element.id = Some(id.to_string());
        self
    }

    pub fn with_state(mut self, state: WindowState) -> Self {
        self.element.state = Some(state);
        self
    }
}

impl Default for Selector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct App {
    pub name: String,
    pub bundle_id: String,
    pub is_frontmost: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Window {
    pub id: String,
    pub title: String,
    pub index: u32,
    pub focused: bool,
    pub bounds: Bounds,
    #[serde(default)]
    pub display_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Element {
    pub role: String,
    pub name: String,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    pub state: ElementState,
    pub bounds: Bounds,
    pub index: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ElementState {
    pub visible: bool,
    #[serde(default)]
    pub focused: Option<bool>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowState {
    pub visible: Option<bool>,
    pub focused: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Bounds {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    pub fn area(&self) -> f64 {
        self.width * self.height
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Coordinate {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolvedTarget {
    pub status: MatchStatus,
    pub total_matches: u32,
    pub app: Option<App>,
    pub window: Option<Window>,
    pub elements: Vec<Element>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Unique,
    Multiple,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeepInspection {
    pub element: Element,
    pub children: Vec<Element>,
    pub backend: String,
    pub actions: Vec<String>,
    pub raw_metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== AppSelector Tests ==========

    #[test]
    fn test_app_selector_from_name() {
        let sel = AppSelector::from_name("Chrome");
        assert_eq!(sel.name, Some("Chrome".to_string()));
        assert_eq!(sel.bundle_id, None);
        assert_eq!(sel.is_frontmost, None);
    }

    #[test]
    fn test_app_selector_from_str_ref() {
        let sel: AppSelector = "Chrome".into();
        assert_eq!(sel.name, Some("Chrome".to_string()));
    }

    #[test]
    fn test_app_selector_from_string() {
        let sel: AppSelector = "Chrome".to_string().into();
        assert_eq!(sel.name, Some("Chrome".to_string()));
    }

    #[test]
    fn test_app_selector_with_bundle_id() {
        let sel = AppSelector::from_name("Chrome").with_bundle_id("com.google.Chrome");
        assert_eq!(sel.name, Some("Chrome".to_string()));
        assert_eq!(sel.bundle_id, Some("com.google.Chrome".to_string()));
    }

    #[test]
    fn test_app_selector_with_frontmost() {
        let sel = AppSelector::from_name("Chrome").with_frontmost(true);
        assert_eq!(sel.is_frontmost, Some(true));
    }

    #[test]
    fn test_app_selector_all_fields() {
        let sel = AppSelector::from_name("Chrome")
            .with_bundle_id("com.google.Chrome")
            .with_frontmost(true);
        assert_eq!(sel.name, Some("Chrome".to_string()));
        assert_eq!(sel.bundle_id, Some("com.google.Chrome".to_string()));
        assert_eq!(sel.is_frontmost, Some(true));
    }

    #[test]
    fn test_app_selector_serialize() {
        let sel = AppSelector::from_name("Chrome").with_bundle_id("com.google.Chrome");
        let json = serde_json::to_string(&sel).unwrap();
        assert!(json.contains("Chrome"));
        assert!(json.contains("com.google.Chrome"));
    }

    #[test]
    fn test_app_selector_deserialize() {
        let json = r#"{"name": "Chrome", "bundle_id": "com.google.Chrome", "is_frontmost": true}"#;
        let sel: AppSelector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.name, Some("Chrome".to_string()));
        assert_eq!(sel.bundle_id, Some("com.google.Chrome".to_string()));
        assert_eq!(sel.is_frontmost, Some(true));
    }

    // ========== WindowSelector Tests ==========

    #[test]
    fn test_window_selector_from_title() {
        let sel = WindowSelector::from_title("Gmail");
        assert_eq!(sel.title, Some("Gmail".to_string()));
        assert_eq!(sel.id, None);
        assert_eq!(sel.index, None);
        assert_eq!(sel.focused, None);
    }

    #[test]
    fn test_window_selector_from_str_ref() {
        let sel: WindowSelector = "Gmail".into();
        assert_eq!(sel.title, Some("Gmail".to_string()));
    }

    #[test]
    fn test_window_selector_from_string() {
        let sel: WindowSelector = "Gmail".to_string().into();
        assert_eq!(sel.title, Some("Gmail".to_string()));
    }

    #[test]
    fn test_window_selector_with_id() {
        let sel = WindowSelector::from_title("Gmail").with_id("win_42");
        assert_eq!(sel.id, Some("win_42".to_string()));
    }

    #[test]
    fn test_window_selector_with_index() {
        let sel = WindowSelector::from_title("Gmail").with_index(0);
        assert_eq!(sel.index, Some(0));
    }

    #[test]
    fn test_window_selector_with_focused() {
        let sel = WindowSelector::from_title("Gmail").with_focused(true);
        assert_eq!(sel.focused, Some(true));
    }

    #[test]
    fn test_window_selector_all_fields() {
        let sel = WindowSelector::from_title("Gmail")
            .with_id("win_42")
            .with_index(0)
            .with_focused(true);
        assert_eq!(sel.title, Some("Gmail".to_string()));
        assert_eq!(sel.id, Some("win_42".to_string()));
        assert_eq!(sel.index, Some(0));
        assert_eq!(sel.focused, Some(true));
    }

    #[test]
    fn test_window_selector_serialize() {
        let sel = WindowSelector::from_title("Gmail").with_focused(true);
        let json = serde_json::to_string(&sel).unwrap();
        assert!(json.contains("Gmail"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_window_selector_deserialize() {
        let json = r#"{"title": "Gmail", "id": "win_42", "index": 0, "focused": true}"#;
        let sel: WindowSelector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.title, Some("Gmail".to_string()));
        assert_eq!(sel.id, Some("win_42".to_string()));
        assert_eq!(sel.index, Some(0));
        assert_eq!(sel.focused, Some(true));
    }

    // ========== Selector Builder Tests ==========

    #[test]
    fn test_selector_new() {
        let sel = Selector::new();
        assert_eq!(sel.app, None);
        assert_eq!(sel.window, None);
        assert_eq!(sel.element.role, None);
        assert_eq!(sel.element.name, None);
        assert_eq!(sel.element.text, None);
        assert_eq!(sel.element.id, None);
        assert_eq!(sel.element.state, None);
    }

    #[test]
    fn test_selector_default() {
        let sel = Selector::default();
        assert_eq!(sel.app, None);
        assert_eq!(sel.window, None);
    }

    #[test]
    fn test_selector_builder_chain() {
        let sel = Selector::new()
            .with_app(AppSelector::from_name("Chrome"))
            .with_window(WindowSelector::from_title("Gmail"))
            .with_role("button")
            .with_name("Compose")
            .with_text("Compose")
            .with_id("compose_btn")
            .with_state(WindowState {
                visible: Some(true),
                focused: Some(false),
            });

        assert_eq!(sel.app.unwrap().name, Some("Chrome".to_string()));
        assert_eq!(sel.window.unwrap().title, Some("Gmail".to_string()));
        assert_eq!(sel.element.role, Some("button".to_string()));
        assert_eq!(sel.element.name, Some("Compose".to_string()));
        assert_eq!(sel.element.text, Some("Compose".to_string()));
        assert_eq!(sel.element.id, Some("compose_btn".to_string()));
        assert!(sel.element.state.is_some());
    }

    #[test]
    fn test_selector_deserialize_full() {
        let json = r#"{
            "app": "Chrome",
            "window": { "title": "Gmail", "focused": true },
            "role": "button",
            "name": "Compose",
            "state": { "visible": true }
        }"#;

        let sel: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.app.unwrap().name, Some("Chrome".to_string()));
        assert_eq!(sel.window.unwrap().focused, Some(true));
        assert_eq!(sel.element.role, Some("button".to_string()));
        assert_eq!(sel.element.name, Some("Compose".to_string()));
    }

    #[test]
    fn test_selector_deserialize_minimal() {
        let json = r#"{ "role": "button" }"#;
        let sel: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.element.role, Some("button".to_string()));
        assert_eq!(sel.app, None);
        assert_eq!(sel.window, None);
    }

    #[test]
    fn test_selector_deserialize_app_as_struct() {
        let json = r#"{
            "app": { "name": "Chrome", "is_frontmost": true },
            "role": "button"
        }"#;

        let sel: Selector = serde_json::from_str(json).unwrap();
        let app = sel.app.unwrap();
        assert_eq!(app.name, Some("Chrome".to_string()));
        assert_eq!(app.is_frontmost, Some(true));
    }

    #[test]
    fn test_selector_deserialize_window_as_string() {
        let json = r#"{
            "window": "Gmail",
            "role": "button"
        }"#;

        let sel: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.window.unwrap().title, Some("Gmail".to_string()));
    }

    #[test]
    fn test_selector_deserialize_empty_json() {
        let json = r#"{}"#;
        let sel: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.app, None);
        assert_eq!(sel.window, None);
        assert_eq!(sel.element.role, None);
    }

    #[test]
    fn test_selector_serialize() {
        let sel = Selector::new()
            .with_app(AppSelector::from_name("Chrome"))
            .with_role("button")
            .with_name("Submit");
        let json = serde_json::to_string(&sel).unwrap();
        assert!(json.contains("Chrome"));
        assert!(json.contains("button"));
        assert!(json.contains("Submit"));
    }

    // ========== Bounds Tests ==========

    #[test]
    fn test_bounds_center() {
        let bounds = Bounds {
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 20.0,
        };
        let (cx, cy) = bounds.center();
        assert_eq!(cx, 125.0);
        assert_eq!(cy, 210.0);
    }

    #[test]
    fn test_bounds_center_at_origin() {
        let bounds = Bounds {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let (cx, cy) = bounds.center();
        assert_eq!(cx, 50.0);
        assert_eq!(cy, 50.0);
    }

    #[test]
    fn test_bounds_center_zero_size() {
        let bounds = Bounds {
            x: 10.0,
            y: 20.0,
            width: 0.0,
            height: 0.0,
        };
        let (cx, cy) = bounds.center();
        assert_eq!(cx, 10.0);
        assert_eq!(cy, 20.0);
    }

    #[test]
    fn test_bounds_contains() {
        let bounds = Bounds {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        assert!(bounds.contains(50.0, 50.0));
        assert!(bounds.contains(0.0, 0.0));
        assert!(bounds.contains(100.0, 100.0));
        assert!(!bounds.contains(101.0, 50.0));
        assert!(!bounds.contains(50.0, 101.0));
        assert!(!bounds.contains(-1.0, 50.0));
    }

    #[test]
    fn test_bounds_area() {
        let bounds = Bounds {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 20.0,
        };
        assert_eq!(bounds.area(), 200.0);
    }

    #[test]
    fn test_bounds_area_zero() {
        let bounds = Bounds {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(bounds.area(), 0.0);
    }

    #[test]
    fn test_bounds_serialize() {
        let bounds = Bounds {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        };
        let json = serde_json::to_string(&bounds).unwrap();
        let deserialized: Bounds = serde_json::from_str(&json).unwrap();
        assert_eq!(bounds, deserialized);
    }

    // ========== Coordinate Tests ==========

    #[test]
    fn test_coordinate_serialize() {
        let coord = Coordinate { x: 100.0, y: 200.0 };
        let json = serde_json::to_string(&coord).unwrap();
        assert!(json.contains("100"));
        assert!(json.contains("200"));
    }

    #[test]
    fn test_coordinate_deserialize() {
        let json = r#"{"x": 100.0, "y": 200.0}"#;
        let coord: Coordinate = serde_json::from_str(json).unwrap();
        assert_eq!(coord.x, 100.0);
        assert_eq!(coord.y, 200.0);
    }

    #[test]
    fn test_coordinate_negative() {
        let coord = Coordinate {
            x: -100.0,
            y: -200.0,
        };
        let json = serde_json::to_string(&coord).unwrap();
        let deserialized: Coordinate = serde_json::from_str(&json).unwrap();
        assert_eq!(coord, deserialized);
    }

    // ========== Element Tests ==========

    #[test]
    fn test_element_serialize() {
        let element = Element {
            role: "button".to_string(),
            name: "Submit".to_string(),
            text: Some("Click me".to_string()),
            id: Some("btn1".to_string()),
            state: ElementState {
                visible: true,
                focused: Some(false),
                enabled: Some(true),
            },
            bounds: Bounds {
                x: 10.0,
                y: 20.0,
                width: 100.0,
                height: 30.0,
            },
            index: 0,
        };
        let json = serde_json::to_string_pretty(&element).unwrap();
        let deserialized: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(element, deserialized);
    }

    #[test]
    fn test_element_minimal() {
        let element = Element {
            role: "text".to_string(),
            name: "Label".to_string(),
            text: None,
            id: None,
            state: ElementState {
                visible: true,
                focused: None,
                enabled: None,
            },
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            index: 0,
        };
        let json = serde_json::to_string(&element).unwrap();
        let deserialized: Element = serde_json::from_str(&json).unwrap();
        assert_eq!(element, deserialized);
    }

    // ========== ElementState Tests ==========

    #[test]
    fn test_element_state_all_fields() {
        let state = ElementState {
            visible: true,
            focused: Some(true),
            enabled: Some(false),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: ElementState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn test_element_state_minimal() {
        let state = ElementState {
            visible: false,
            focused: None,
            enabled: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("false"));
    }

    // ========== WindowState Tests ==========

    #[test]
    fn test_window_state_serialize() {
        let state = WindowState {
            visible: Some(true),
            focused: Some(false),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WindowState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn test_window_state_none() {
        let state = WindowState {
            visible: None,
            focused: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WindowState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    // ========== MatchStatus Tests ==========

    #[test]
    fn test_match_status_unique() {
        let status = MatchStatus::Unique;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"unique\"");
    }

    #[test]
    fn test_match_status_multiple() {
        let status = MatchStatus::Multiple;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"multiple\"");
    }

    #[test]
    fn test_match_status_none() {
        let status = MatchStatus::None;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"none\"");
    }

    #[test]
    fn test_match_status_deserialize() {
        let json = "\"unique\"";
        let status: MatchStatus = serde_json::from_str(json).unwrap();
        assert_eq!(status, MatchStatus::Unique);
    }

    // ========== App Tests ==========

    #[test]
    fn test_app_serialize() {
        let app = App {
            name: "Google Chrome".to_string(),
            bundle_id: "com.google.Chrome".to_string(),
            is_frontmost: true,
        };
        let json = serde_json::to_string(&app).unwrap();
        let deserialized: App = serde_json::from_str(&json).unwrap();
        assert_eq!(app, deserialized);
    }

    // ========== Window Tests ==========

    #[test]
    fn test_window_serialize() {
        let window = Window {
            id: "win_42".to_string(),
            title: "Gmail".to_string(),
            index: 0,
            focused: true,
            bounds: Bounds {
                x: 0.0,
                y: 25.0,
                width: 1440.0,
                height: 875.0,
            },
        };
        let json = serde_json::to_string(&window).unwrap();
        let deserialized: Window = serde_json::from_str(&json).unwrap();
        assert_eq!(window, deserialized);
    }

    // ========== ResolvedTarget Tests ==========

    #[test]
    fn test_resolved_target_serialize() {
        let target = ResolvedTarget {
            status: MatchStatus::Unique,
            total_matches: 1,
            app: Some(App {
                name: "Google Chrome".to_string(),
                bundle_id: "com.google.Chrome".to_string(),
                is_frontmost: true,
            }),
            window: Some(Window {
                id: "win_1042".to_string(),
                title: "Gmail".to_string(),
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

        let json = serde_json::to_string_pretty(&target).unwrap();
        let deserialized: ResolvedTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(target, deserialized);
    }

    #[test]
    fn test_resolved_target_multiple() {
        let target = ResolvedTarget {
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
                    state: ElementState {
                        visible: true,
                        focused: None,
                        enabled: None,
                    },
                    bounds: Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    index: 0,
                },
                Element {
                    role: "button".to_string(),
                    name: "OK".to_string(),
                    text: None,
                    id: None,
                    state: ElementState {
                        visible: true,
                        focused: None,
                        enabled: None,
                    },
                    bounds: Bounds {
                        x: 0.0,
                        y: 20.0,
                        width: 10.0,
                        height: 10.0,
                    },
                    index: 1,
                },
            ],
        };
        let json = serde_json::to_string(&target).unwrap();
        let deserialized: ResolvedTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(target, deserialized);
    }

    #[test]
    fn test_resolved_target_empty_elements() {
        let target = ResolvedTarget {
            status: MatchStatus::None,
            total_matches: 0,
            app: None,
            window: None,
            elements: vec![],
        };
        let json = serde_json::to_string(&target).unwrap();
        assert!(json.contains("none"));
        assert!(json.contains("0"));
    }

    // ========== DeepInspection Tests ==========

    #[test]
    fn test_deep_inspection_serialize() {
        let inspection = DeepInspection {
            element: Element {
                role: "button".to_string(),
                name: "Submit".to_string(),
                text: None,
                id: None,
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: None,
                },
                bounds: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 10.0,
                    height: 10.0,
                },
                index: 0,
            },
            children: vec![],
            backend: "at_tree".to_string(),
            actions: vec!["click".to_string(), "hover".to_string()],
            raw_metadata: Some(serde_json::json!({"role": "AXButton"})),
        };
        let json = serde_json::to_string_pretty(&inspection).unwrap();
        let deserialized: DeepInspection = serde_json::from_str(&json).unwrap();
        assert_eq!(inspection, deserialized);
    }

    #[test]
    fn test_deep_inspection_with_children() {
        let inspection = DeepInspection {
            element: Element {
                role: "group".to_string(),
                name: "Form".to_string(),
                text: None,
                id: None,
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: None,
                },
                bounds: Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
                index: 0,
            },
            children: vec![Element {
                role: "textfield".to_string(),
                name: "Email".to_string(),
                text: None,
                id: None,
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: None,
                },
                bounds: Bounds {
                    x: 10.0,
                    y: 10.0,
                    width: 80.0,
                    height: 20.0,
                },
                index: 0,
            }],
            backend: "cdp".to_string(),
            actions: vec!["click".to_string()],
            raw_metadata: None,
        };
        let json = serde_json::to_string(&inspection).unwrap();
        let deserialized: DeepInspection = serde_json::from_str(&json).unwrap();
        assert_eq!(inspection, deserialized);
    }

    // ========== README Example Tests ==========

    #[test]
    fn test_readme_selector_input_example() {
        let json = r#"{
            "app": "Chrome",
            "window": { "title": "Gmail", "focused": true },
            "role": "button",
            "name": "Compose"
        }"#;
        let sel: Selector = serde_json::from_str(json).unwrap();
        assert_eq!(sel.app.as_ref().unwrap().name, Some("Chrome".to_string()));
        assert_eq!(
            sel.window.as_ref().unwrap().title,
            Some("Gmail".to_string())
        );
        assert_eq!(sel.window.as_ref().unwrap().focused, Some(true));
        assert_eq!(sel.element.role, Some("button".to_string()));
        assert_eq!(sel.element.name, Some("Compose".to_string()));
    }

    #[test]
    fn test_readme_find_output_example() {
        let json = r#"{
            "status": "unique",
            "total_matches": 1,
            "app": {
                "name": "Google Chrome",
                "bundle_id": "com.google.Chrome",
                "is_frontmost": true
            },
            "window": {
                "id": "win_1042",
                "title": "Inbox - user@gmail.com - Gmail",
                "index": 0,
                "focused": true,
                "bounds": { "x": 0, "y": 25, "width": 1440, "height": 875 }
            },
            "elements": [
                {
                    "role": "button",
                    "name": "Compose",
                    "id": "dom_compose_btn",
                    "state": { "visible": true, "enabled": true },
                    "bounds": { "x": 120, "y": 85, "width": 100, "height": 36 },
                    "index": 0
                }
            ]
        }"#;
        let target: ResolvedTarget = serde_json::from_str(json).unwrap();
        assert_eq!(target.status, MatchStatus::Unique);
        assert_eq!(target.total_matches, 1);
        assert_eq!(target.elements.len(), 1);
        assert_eq!(target.elements[0].role, "button");
    }
}
