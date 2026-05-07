use sootie_core::action::{ActionTarget, MouseButton, ScrollDirection};
use sootie_core::recipe::StepTarget;
use sootie_core::selector::{AppSelector, Coordinate, Selector, WindowSelector, WindowState};

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
                "app": {
                    "description": "App selector - string or object with name/bundle_id/is_frontmost"
                },
                "window": {
                    "description": "Window selector - string or object with title/id/index/focused"
                },
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
                "app": {},
                "window": {},
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
                "app": {},
                "window": {},
                "role": { "type": "string" },
                "name": { "type": "string" },
                "state": {
                    "type": "object",
                    "description": "State to wait for (e.g. {visible: true})"
                },
                "timeout": {
                    "type": "number",
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
                "app": {},
                "window": {},
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
                    "type": "number",
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
                    "type": "number",
                    "description": "Maximum number of results to return",
                    "default": 10
                }
            },
            "required": ["pattern"]
        }),
    }
}

fn action_click() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_click".to_string(),
        description: "Click a resolved target or coordinates".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Selector object to click"
                },
                "coordinate": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" }
                    }
                },
                "button": {
                    "type": "string",
                    "enum": ["left", "right", "middle"],
                    "default": "left"
                },
                "count": {
                    "type": "number",
                    "default": 1
                }
            }
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
                    "type": "object",
                    "description": "Selector defining where to type. Must include app and at least one element identifier.",
                    "properties": {
                        "app": { "type": "string", "description": "Application name" },
                        "role": { "type": "string", "description": "UI element role (e.g. textfield, button)" },
                        "name": { "type": "string", "description": "Element accessible name" },
                        "id": { "type": "string", "description": "Element identifier" },
                        "text": { "type": "string", "description": "Element text content" }
                    },
                    "required": ["app"]
                },
                "clear_first": {
                    "type": "boolean",
                    "default": false,
                    "description": "Clear existing text before typing"
                }
            },
            "required": ["text", "target"]
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
                    "description": "Selector object to scroll"
                },
                "coordinate": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" }
                    }
                },
                "direction": {
                    "type": "string",
                    "enum": ["up", "down", "left", "right"]
                },
                "amount": {
                    "type": "number",
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
                    "description": "Selector object to hover"
                },
                "coordinate": {
                    "type": "object",
                    "properties": {
                        "x": { "type": "number" },
                        "y": { "type": "number" }
                    }
                }
            }
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
                    "description": "Source selector or coordinate"
                },
                "to": {
                    "description": "Destination selector or coordinate"
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

pub fn parse_action_target(args: &serde_json::Value) -> Option<ActionTarget> {
    if let Some(coord) = args.get("coordinate") {
        if let (Some(x), Some(y)) = (
            coord.get("x").and_then(|v| v.as_f64()),
            coord.get("y").and_then(|v| v.as_f64()),
        ) {
            return Some(ActionTarget::Coordinate(Coordinate { x, y }));
        }
    }

    if let Some(target) = args.get("target") {
        if let Some(coord) = target.get("coordinate") {
            if let (Some(x), Some(y)) = (
                coord.get("x").and_then(|v| v.as_f64()),
                coord.get("y").and_then(|v| v.as_f64()),
            ) {
                return Some(ActionTarget::Coordinate(Coordinate { x, y }));
            }
        }

        if target.is_object() {
            let selector = parse_selector_from_args(target);
            if selector.app.is_some()
                || selector.window.is_some()
                || selector.element.role.is_some()
                || selector.element.name.is_some()
                || selector.element.text.is_some()
                || selector.element.id.is_some()
                || selector.element.state.is_some()
            {
                return Some(ActionTarget::Selector(selector));
            }
        }
    }

    if args.get("app").is_some()
        || args.get("window").is_some()
        || args.get("role").is_some()
        || args.get("name").is_some()
        || args.get("id").is_some()
        || args.get("state").is_some()
    {
        let selector = parse_selector_from_args(args);
        return Some(ActionTarget::Selector(selector));
    }

    None
}

pub fn validate_selector(selector: &Selector) -> Result<(), String> {
    if selector.app.is_none() {
        return Err("Selector must include 'app' field to specify which application".to_string());
    }
    
    if selector.element.role.is_none()
        && selector.element.name.is_none()
        && selector.element.id.is_none()
        && selector.element.text.is_none()
    {
        return Err("Selector must include at least one element identifier (role, name, id, or text)".to_string());
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

pub fn parse_scroll_direction(s: &str) -> ScrollDirection {
    match s {
        "up" => ScrollDirection::Up,
        "down" => ScrollDirection::Down,
        "left" => ScrollDirection::Left,
        "right" => ScrollDirection::Right,
        _ => ScrollDirection::Down,
    }
}

pub fn parse_step_target(value: &serde_json::Value) -> Option<StepTarget> {
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
