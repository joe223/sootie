use sootie_core::action::{ActionTarget, MouseButton, ScrollDirection};
use sootie_core::recipe::StepTarget;
#[cfg(test)]
use sootie_core::selector::{AppSelector, WindowSelector, WindowState};
use sootie_core::selector::{Coordinate, Selector};

use crate::types::ToolDefinition;

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        perception_context(),
        perception_find(),
        perception_inspect(),
        perception_wait(),
        perception_screenshot(),
        perception_find_apps(),
        action_click(),
        action_type(),
        action_press(),
        action_hotkey(),
        action_scroll(),
        action_hover(),
        action_drag(),
        app_launch(),
        window_focus(),
        window_op(),
        workflow_recipes(),
        workflow_run(),
        workflow_recipe_save(),
        workflow_recipe_delete(),
    ]
}

fn perception_context() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_context".to_string(),
        description:
            "Get the macro environment state: a tree of running apps and their open windows"
                .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn perception_find() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_find".to_string(),
        description:
            "Resolve UI targets across desktop apps and web apps with the unified selector scheme"
                .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": app_selector_schema(),
                "window": window_selector_schema(),
                "role": {
                    "type": "string",
                    "description": "UI role (button, textfield, link, etc.)"
                },
                "name": {
                    "type": "string",
                    "description": "Accessible label or computed name"
                },
                "text": {
                    "type": "string",
                    "description": "Visible text content"
                },
                "id": {
                    "type": "string",
                    "description": "Backend-specific ID"
                }
            }
        }),
    }
}

fn perception_inspect() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_inspect".to_string(),
        description: "Return normalized metadata and full sub-tree for one resolved target"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": app_selector_schema(),
                "window": window_selector_schema(),
                "role": { "type": "string" },
                "name": { "type": "string" },
                "text": { "type": "string" },
                "id": { "type": "string" }
            }
        }),
    }
}

fn perception_wait() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_wait".to_string(),
        description: "Pause execution until a target matches a specific state or timeout"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": app_selector_schema(),
                "window": window_selector_schema(),
                "role": { "type": "string" },
                "name": { "type": "string" },
                "state": {
                    "type": "object",
                    "description": "State to wait for (e.g. {visible: true})"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds",
                    "default": 5000
                }
            }
        }),
    }
}

fn perception_screenshot() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_screenshot".to_string(),
        description: "Capture a screen, window, or region screenshot. Uses JPEG format for efficient transmission (80-90% smaller than PNG).".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": app_selector_schema(),
                "window": window_selector_schema(),
                "region": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "width": { "type": "number" },
                        "height": { "type": "number" }
                    }
                },
                "display_id": {
                    "type": "integer",
                    "description": "Display ID (macOS: 1=main, 2=secondary, etc)"
                }
            }
        }),
    }
}

fn perception_find_apps() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_find_apps".to_string(),
        description:
            "Find installed applications by name pattern (supports wildcards like '*Chrome*')"
                .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "App name pattern (supports wildcards: 'Chrome', '*Chrome*', 'Google*')"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return",
                    "default": 10
                }
            },
            "required": ["pattern"]
        }),
    }
}

fn coordinate_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "x": { "type": "number" },
            "y": { "type": "number" }
        },
        "required": ["x", "y"]
    })
}

fn app_selector_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            { "type": "string" },
            {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "bundle_id": { "type": "string" },
                    "is_frontmost": { "type": "boolean" }
                }
            }
        ]
    })
}

fn window_selector_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            { "type": "string" },
            {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "id": { "type": "string" },
                    "index": { "type": "integer" },
                    "focused": { "type": "boolean" }
                }
            }
        ]
    })
}

fn action_selector_target_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "app": app_selector_schema(),
            "window": window_selector_schema(),
            "role": { "type": "string" },
            "name": { "type": "string" },
            "text": { "type": "string" },
            "id": { "type": "string" },
            "state": {
                "type": "object",
                "properties": {
                    "visible": { "type": "boolean" },
                    "focused": { "type": "boolean" }
                }
            }
        },
        "required": ["app"],
        "anyOf": [
            { "required": ["role"] },
            { "required": ["name"] },
            { "required": ["text"] },
            { "required": ["id"] }
        ]
    })
}

fn canonical_target_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            action_selector_target_schema(),
            {
                "type": "object",
                "properties": {
                    "coordinate": coordinate_schema()
                },
                "required": ["coordinate"]
            }
        ]
    })
}

fn action_click() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_click".to_string(),
        description: "Click a resolved target or coordinates".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical target object: selector fields or a nested coordinate",
                    "allOf": [canonical_target_schema()]
                },
                "button": {
                    "type": "string",
                    "enum": ["left", "right", "middle"],
                    "default": "left"
                },
                "count": {
                    "type": "integer",
                    "default": 1
                }
            },
            "required": ["target"]
        }),
    }
}

fn action_type() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_type".to_string(),
        description: "Type text into a UI element. Use sootie_find to locate elements first. Requires target with app and at least one element identifier (role, name, id).".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to type"
                },
                "target": {
                    "description": "Canonical target object: selector fields or a nested coordinate",
                    "allOf": [canonical_target_schema()]
                },
                "clear_first": {
                    "type": "boolean",
                    "default": false,
                    "description": "Clear existing text before typing"
                }
            },
            "required": ["text"]
        }),
    }
}

fn action_press() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_press".to_string(),
        description: "Press a single key".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Key to press (e.g. Return, Tab, Escape)"
                }
            },
            "required": ["key"]
        }),
    }
}

fn action_hotkey() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_hotkey".to_string(),
        description: "Press key combinations".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "keys": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Keys to press together (e.g. [\"Cmd\", \"C\"])"
                }
            },
            "required": ["keys"]
        }),
    }
}

fn action_scroll() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_scroll".to_string(),
        description: "Scroll up, down, left, or right".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical target object: selector fields or a nested coordinate",
                    "allOf": [canonical_target_schema()]
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"]
                },
                "amount": {
                    "type": "integer",
                    "default": 3
                }
            },
            "required": ["direction"]
        }),
    }
}

fn action_hover() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_hover".to_string(),
        description: "Hover over a resolved target or coordinates".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical target object: selector fields or a nested coordinate",
                    "allOf": [canonical_target_schema()]
                }
            },
            "required": ["target"]
        }),
    }
}

fn action_drag() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_drag".to_string(),
        description: "Drag from one resolved target or point to another".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "from": {
                    "description": "Source selector or coordinate",
                    "allOf": [canonical_target_schema()]
                },
                "to": {
                    "description": "Destination selector or coordinate",
                    "allOf": [canonical_target_schema()]
                }
            },
            "required": ["from", "to"]
        }),
    }
}

fn window_focus() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_focus".to_string(),
        description: "Bring an application or specific window to the front".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "Application name to focus (required)"
                },
                "window": {
                    "type": "string",
                    "description": "Window title to focus specific window (optional)"
                }
            },
            "required": ["app"]
        }),
    }
}

fn app_launch() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_launch".to_string(),
        description: "Launch or open an application".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": {
                    "allOf": [app_selector_schema()],
                    "description": "App name or bundle identifier"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional arguments to pass to the app"
                }
            },
            "required": ["app"]
        }),
    }
}

fn window_op() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_window".to_string(),
        description: "Minimize, maximize, close, move, or resize a window. Requires app to specify which application's window to operate on.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "app": {
                    "type": "string",
                    "description": "Application name (required)"
                },
                "window": {
                    "type": "string",
                    "description": "Window title (optional, defaults to frontmost window)"
                },
                "operation": {
                    "type": "string",
                    "enum": ["minimize", "maximize", "close", "move", "resize"],
                    "description": "Window operation to perform"
                },
                "x": { "type": "number", "description": "X position for move" },
                "y": { "type": "number", "description": "Y position for move" },
                "width": { "type": "number", "description": "Width for resize" },
                "height": { "type": "number", "description": "Height for resize" }
            },
            "required": ["operation", "app"]
        }),
    }
}

fn workflow_recipes() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_recipes".to_string(),
        description: "List all installed workflows".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn workflow_run() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_run".to_string(),
        description: "Execute a workflow with parameters".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Recipe name to execute"
                },
                "params": {
                    "type": "object",
                    "description": "Parameters for the recipe"
                }
            },
            "required": ["name"]
        }),
    }
}

fn workflow_recipe_save() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_recipe_save".to_string(),
        description: "Save a new workflow".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "recipe": {
                    "type": "object",
                    "description": "Recipe JSON to save"
                }
            },
            "required": ["recipe"]
        }),
    }
}

fn workflow_recipe_delete() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_recipe_delete".to_string(),
        description: "Remove a workflow".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Recipe name to delete"
                }
            },
            "required": ["name"]
        }),
    }
}

#[cfg(test)]
pub fn parse_selector_from_args(args: &serde_json::Value) -> Selector {
    let mut sel = Selector::new();

    if let Some(app) = args.get("app") {
        if let Some(s) = app.as_str() {
            sel.app = Some(AppSelector::from_name(s));
        } else if let Ok(app_sel) = serde_json::from_value::<AppSelector>(app.clone()) {
            sel.app = Some(app_sel);
        }
    }

    if let Some(window) = args.get("window") {
        if let Some(s) = window.as_str() {
            sel.window = Some(WindowSelector::from_title(s));
        } else if let Ok(win_sel) = serde_json::from_value::<WindowSelector>(window.clone()) {
            sel.window = Some(win_sel);
        }
    }

    if let Some(role) = args.get("role").and_then(|v| v.as_str()) {
        sel.element.role = Some(role.to_string());
    }

    if let Some(name) = args.get("name").and_then(|v| v.as_str()) {
        sel.element.name = Some(name.to_string());
    }

    if let Some(text) = args.get("text").and_then(|v| v.as_str()) {
        sel.element.text = Some(text.to_string());
    }

    if let Some(id) = args.get("id").and_then(|v| v.as_str()) {
        sel.element.id = Some(id.to_string());
    }

    if let Some(state) = args.get("state") {
        if let Ok(state) = serde_json::from_value::<WindowState>(state.clone()) {
            sel.element.state = Some(state);
        }
    }

    sel
}

pub fn parse_selector_from_args_strict(args: &serde_json::Value) -> Result<Selector, String> {
    serde_json::from_value::<Selector>(args.clone()).map_err(|e| format!("Invalid selector: {}", e))
}

pub fn parse_action_target(args: &serde_json::Value) -> Result<ActionTarget, String> {
    let has_top_level_coordinate = args.get("coordinate").is_some();
    let has_target = args.get("target").is_some();
    let has_top_level_selector = selector_field_keys_present(args);

    let form_count = usize::from(has_top_level_coordinate)
        + usize::from(has_target)
        + usize::from(has_top_level_selector);

    if form_count == 0 {
        return Err("Must provide target or coordinate".to_string());
    }

    if form_count > 1 {
        return Err(
            "Must provide exactly one target form: target, coordinate, or selector fields"
                .to_string(),
        );
    }

    if has_top_level_coordinate {
        return parse_coordinate(args.get("coordinate")).map(ActionTarget::Coordinate);
    }

    if let Some(target) = args.get("target") {
        if target.get("coordinate").is_some() {
            if selector_field_keys_present(target) {
                return Err(
                    "Target must be either a coordinate or a selector, not both".to_string()
                );
            }

            return parse_coordinate(target.get("coordinate")).map(ActionTarget::Coordinate);
        }

        let selector = parse_selector_from_args_strict(target)?;
        return if has_selector_values(&selector) {
            Ok(ActionTarget::Selector(selector))
        } else {
            Err("Target selector must include at least one selector field".to_string())
        };
    }

    let selector = parse_selector_from_args_strict(args)?;
    if has_selector_values(&selector) {
        Ok(ActionTarget::Selector(selector))
    } else {
        Err("Selector must include at least one selector field".to_string())
    }
}

pub fn parse_optional_action_target(
    args: &serde_json::Value,
) -> Result<Option<ActionTarget>, String> {
    let has_any_target_form = args.get("target").is_some()
        || args.get("coordinate").is_some()
        || selector_field_keys_present(args);

    if has_any_target_form {
        parse_action_target(args).map(Some)
    } else {
        Ok(None)
    }
}

fn parse_coordinate(value: Option<&serde_json::Value>) -> Result<Coordinate, String> {
    let coord = value.ok_or("Coordinate is required")?;
    let x = coord
        .get("x")
        .and_then(|v| v.as_f64())
        .ok_or("Coordinate must include numeric x")?;
    let y = coord
        .get("y")
        .and_then(|v| v.as_f64())
        .ok_or("Coordinate must include numeric y")?;

    Ok(Coordinate { x, y })
}

pub fn selector_field_keys_present(value: &serde_json::Value) -> bool {
    value.get("app").is_some()
        || value.get("window").is_some()
        || value.get("role").is_some()
        || value.get("name").is_some()
        || value.get("text").is_some()
        || value.get("id").is_some()
        || value.get("state").is_some()
}

fn has_selector_values(selector: &Selector) -> bool {
    selector.app.is_some()
        || selector.window.is_some()
        || selector.element.role.is_some()
        || selector.element.name.is_some()
        || selector.element.text.is_some()
        || selector.element.id.is_some()
        || selector.element.state.is_some()
}

pub fn validate_query_selector(selector: &Selector) -> Result<(), String> {
    if !has_selector_values(selector) {
        return Err("Selector must include at least one selector field".to_string());
    }

    Ok(())
}

pub fn validate_action_selector(selector: &Selector) -> Result<(), String> {
    if selector.app.is_none() {
        return Err("Selector must include 'app' field to specify which application".to_string());
    }

    if selector.element.role.is_none()
        && selector.element.name.is_none()
        && selector.element.id.is_none()
        && selector.element.text.is_none()
    {
        return Err(
            "Selector must include at least one element identifier (role, name, id, or text)"
                .to_string(),
        );
    }

    Ok(())
}

pub fn parse_mouse_button(s: &str) -> MouseButton {
    match s {
        "right" => MouseButton::Right,
        "middle" => MouseButton::Middle,
        _ => MouseButton::Left,
    }
}

pub fn parse_mouse_button_strict(s: &str) -> Result<MouseButton, String> {
    match s {
        "left" => Ok(MouseButton::Left),
        "right" => Ok(MouseButton::Right),
        "middle" => Ok(MouseButton::Middle),
        _ => Err(format!("Unknown mouse button: {}", s)),
    }
}

pub fn parse_scroll_direction(s: &str) -> ScrollDirection {
    match s {
        "up" => ScrollDirection::Up,
        "down" => ScrollDirection::Down,
        "left" => ScrollDirection::Left,
        "right" => ScrollDirection::Right,
        _ => ScrollDirection::Down,
    }
}

pub fn parse_scroll_direction_strict(s: &str) -> Result<ScrollDirection, String> {
    match s {
        "up" => Ok(ScrollDirection::Up),
        "down" => Ok(ScrollDirection::Down),
        "left" => Ok(ScrollDirection::Left),
        "right" => Ok(ScrollDirection::Right),
        _ => Err(format!("Unknown scroll direction: {}", s)),
    }
}

pub fn parse_step_target(value: &serde_json::Value) -> Option<StepTarget> {
    if let (Some(x), Some(y)) = (
        value.get("x").and_then(|v| v.as_f64()),
        value.get("y").and_then(|v| v.as_f64()),
    ) {
        return Some(StepTarget::Coordinate(Coordinate { x, y }));
    }

    if let Some(coord) = value.get("coordinate") {
        if let (Some(x), Some(y)) = (
            coord.get("x").and_then(|v| v.as_f64()),
            coord.get("y").and_then(|v| v.as_f64()),
        ) {
            return Some(StepTarget::Coordinate(Coordinate { x, y }));
        }
    }

    if value.is_object() {
        if let Ok(selector) = serde_json::from_value::<Selector>(value.clone()) {
            return Some(StepTarget::Selector(selector));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_tools_count() {
        let tools = all_tools();
        assert_eq!(tools.len(), 20);
    }

    #[test]
    fn test_tool_names() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"sootie_context"));
        assert!(names.contains(&"sootie_find"));
        assert!(names.contains(&"sootie_inspect"));
        assert!(names.contains(&"sootie_wait"));
        assert!(names.contains(&"sootie_screenshot"));
        assert!(names.contains(&"sootie_click"));
        assert!(names.contains(&"sootie_type"));
        assert!(names.contains(&"sootie_press"));
        assert!(names.contains(&"sootie_hotkey"));
        assert!(names.contains(&"sootie_scroll"));
        assert!(names.contains(&"sootie_hover"));
        assert!(names.contains(&"sootie_drag"));
        assert!(names.contains(&"sootie_focus"));
        assert!(names.contains(&"sootie_window"));
        assert!(names.contains(&"sootie_recipes"));
        assert!(names.contains(&"sootie_run"));
        assert!(names.contains(&"sootie_recipe_save"));
        assert!(names.contains(&"sootie_recipe_delete"));
    }

    #[test]
    fn test_action_tools_publish_canonical_target_schema() {
        let click = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_click")
            .unwrap();

        let required = click.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|value| value == "target"));
        assert!(click.input_schema["properties"].get("coordinate").is_none());
    }

    #[test]
    fn test_selector_and_numeric_schemas_are_typed() {
        let tools = all_tools();
        let find = tools
            .iter()
            .find(|tool| tool.name == "sootie_find")
            .unwrap();
        let wait = tools
            .iter()
            .find(|tool| tool.name == "sootie_wait")
            .unwrap();
        let find_apps = tools
            .iter()
            .find(|tool| tool.name == "sootie_find_apps")
            .unwrap();
        let scroll = tools
            .iter()
            .find(|tool| tool.name == "sootie_scroll")
            .unwrap();

        assert!(find.input_schema["properties"]["app"]["oneOf"].is_array());
        assert!(find.input_schema["properties"]["window"]["oneOf"].is_array());
        assert_eq!(
            wait.input_schema["properties"]["timeout"]["type"],
            "integer"
        );
        assert_eq!(
            find_apps.input_schema["properties"]["limit"]["type"],
            "integer"
        );
        assert_eq!(
            scroll.input_schema["properties"]["amount"]["type"],
            "integer"
        );
    }

    #[test]
    fn test_action_target_schema_requires_selector_keys_or_coordinate() {
        let click = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_click")
            .unwrap();

        let target_schema = &click.input_schema["properties"]["target"]["allOf"][0];
        assert!(target_schema["oneOf"].is_array());
        assert_eq!(target_schema["oneOf"][0]["required"][0], "app");
        assert!(target_schema["oneOf"][0]["anyOf"].is_array());
        assert_eq!(target_schema["oneOf"][1]["required"][0], "coordinate");
    }

    #[test]
    fn test_drag_schema_uses_canonical_target_shape() {
        let drag = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_drag")
            .unwrap();

        assert!(drag.input_schema["properties"]["from"]["allOf"].is_array());
        assert!(drag.input_schema["properties"]["to"]["allOf"].is_array());
    }

    #[test]
    fn test_parse_selector_from_args_string_app() {
        let args = serde_json::json!({
            "app": "Chrome",
            "window": "Gmail",
            "role": "button",
            "name": "Compose"
        });

        let sel = parse_selector_from_args(&args);
        assert_eq!(sel.app.unwrap().name, Some("Chrome".to_string()));
        assert_eq!(sel.window.unwrap().title, Some("Gmail".to_string()));
        assert_eq!(sel.element.role, Some("button".to_string()));
        assert_eq!(sel.element.name, Some("Compose".to_string()));
    }

    #[test]
    fn test_parse_selector_from_args_struct_app() {
        let args = serde_json::json!({
            "app": { "name": "Chrome", "is_frontmost": true },
            "role": "button"
        });

        let sel = parse_selector_from_args(&args);
        let app = sel.app.unwrap();
        assert_eq!(app.name, Some("Chrome".to_string()));
        assert_eq!(app.is_frontmost, Some(true));
    }

    #[test]
    fn test_parse_selector_from_args_state() {
        let args = serde_json::json!({
            "name": "Compose",
            "state": { "visible": true, "focused": false }
        });

        let sel = parse_selector_from_args(&args);
        let state = sel.element.state.unwrap();
        assert_eq!(state.visible, Some(true));
        assert_eq!(state.focused, Some(false));
    }

    #[test]
    fn test_parse_action_target_coordinate() {
        let args = serde_json::json!({
            "coordinate": { "x": 100.0, "y": 200.0 }
        });

        let target = parse_action_target(&args).unwrap();
        match target {
            ActionTarget::Coordinate(c) => {
                assert_eq!(c.x, 100.0);
                assert_eq!(c.y, 200.0);
            }
            _ => panic!("expected coordinate"),
        }
    }

    #[test]
    fn test_parse_action_target_selector() {
        let args = serde_json::json!({
            "app": "Chrome",
            "role": "button",
            "name": "Submit"
        });

        let target = parse_action_target(&args).unwrap();
        match target {
            ActionTarget::Selector(s) => {
                assert_eq!(s.app.unwrap().name, Some("Chrome".to_string()));
                assert_eq!(s.element.role, Some("button".to_string()));
            }
            _ => panic!("expected selector"),
        }
    }

    #[test]
    fn test_parse_action_target_nested_target_selector() {
        let args = serde_json::json!({
            "target": {
                "app": "Chrome",
                "window": "Gmail",
                "role": "button",
                "name": "Compose"
            }
        });

        let target = parse_action_target(&args).unwrap();
        match target {
            ActionTarget::Selector(s) => {
                assert_eq!(s.app.unwrap().name.as_deref(), Some("Chrome"));
                assert_eq!(s.window.unwrap().title.as_deref(), Some("Gmail"));
                assert_eq!(s.element.role.as_deref(), Some("button"));
                assert_eq!(s.element.name.as_deref(), Some("Compose"));
            }
            _ => panic!("expected selector"),
        }
    }

    #[test]
    fn test_parse_mouse_button() {
        assert_eq!(parse_mouse_button("left"), MouseButton::Left);
        assert_eq!(parse_mouse_button("right"), MouseButton::Right);
        assert_eq!(parse_mouse_button("middle"), MouseButton::Middle);
        assert_eq!(parse_mouse_button("other"), MouseButton::Left);
    }

    #[test]
    fn test_parse_scroll_direction() {
        assert_eq!(parse_scroll_direction("up"), ScrollDirection::Up);
        assert_eq!(parse_scroll_direction("down"), ScrollDirection::Down);
        assert_eq!(parse_scroll_direction("left"), ScrollDirection::Left);
        assert_eq!(parse_scroll_direction("right"), ScrollDirection::Right);
    }

    #[test]
    fn test_parse_step_target_coordinate() {
        let val = serde_json::json!({
            "coordinate": { "x": 50, "y": 75 }
        });
        let target = parse_step_target(&val).unwrap();
        match target {
            StepTarget::Coordinate(c) => {
                assert_eq!(c.x, 50.0);
                assert_eq!(c.y, 75.0);
            }
            _ => panic!("expected coordinate"),
        }
    }

    #[test]
    fn test_parse_step_target_selector() {
        let val = serde_json::json!({
            "role": "button",
            "name": "Submit"
        });
        let target = parse_step_target(&val).unwrap();
        match target {
            StepTarget::Selector(s) => {
                assert_eq!(s.element.role, Some("button".to_string()));
            }
            _ => panic!("expected selector"),
        }
    }
}
