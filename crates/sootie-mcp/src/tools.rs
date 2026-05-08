use sootie_core::action::{MouseButton, ScrollDirection};
use sootie_core::recipe::StepTarget;
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
                "target": target_schema()
            },
            "required": ["target"]
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
                "target": target_schema()
            },
            "required": ["target"]
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
                "target": target_schema(),
                "state": {
                    "type": "object",
                    "properties": {
                        "visible": { "type": "boolean" },
                        "focused": { "type": "boolean" }
                    }
                },
                "timeout": {
                    "type": "integer",
                    "default": 5000,
                    "description": "Timeout in milliseconds"
                }
            },
        "required": ["target"]
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
                "scope": scope_schema(),
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

fn app_selector_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            { "type": "string" },
            {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "name": { "type": "string", "description": "Human app name, e.g. Safari" },
                    "bundle_id": { "type": "string", "description": "OS package identifier" },
                    "is_frontmost": { "type": "boolean", "description": "Require app to be frontmost" }
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
                "additionalProperties": false,
                "properties": {
                    "title": { "type": "string", "description": "Window title substring or exact match policy" },
                    "id": { "type": "string", "description": "OS/browser window identifier" },
                    "index": { "type": "integer", "description": "Window stack index" },
                    "focused": { "type": "boolean", "description": "Require focused window" }
                }
            }
        ]
    })
}

fn target_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "app": app_selector_schema(),
            "window": window_selector_schema(),
            "selector": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "role": { "type": "string", "description": "Normalized element role" },
                    "name": { "type": "string", "description": "Accessible/computed name" },
                    "id": { "type": "string", "description": "Backend-specific element identifier" },
                    "text": { "type": "string", "description": "Visible text content" }
                },
                "required": [],
                "anyOf": [
                    { "required": ["role"] },
                    { "required": ["name"] },
                    { "required": ["id"] },
                    { "required": ["text"] }
                ]
            }
        },
        "required": ["selector"]
    })
}

fn scope_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "app": app_selector_schema(),
            "window": window_selector_schema()
        },
        "required": []
    })
}

fn action_click() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_click".to_string(),
        description: "Click a resolved target".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical target object",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
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
        description: "Type text into the top-ranked element resolved from target.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to type"
                },
                "target": {
                    "description": "Canonical target object",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
                },
                "clear_first": {
                    "type": "boolean",
                    "default": false,
                    "description": "Clear existing text before typing"
                }
            },
            "required": ["target", "text"]
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
                    "description": "Canonical target object",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
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
            "required": ["target", "direction"]
        }),
    }
}

fn action_hover() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_hover".to_string(),
        description: "Hover over the top-ranked element resolved from target".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical target object",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
                }
            },
            "required": ["target"]
        }),
    }
}

fn action_drag() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_drag".to_string(),
        description: "Drag between two resolved targets".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "from_target": {
                    "description": "Source target",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
                },
                "to_target": {
                    "description": "Destination target",
                    "type": "object",
                    "properties": target_schema()["properties"].clone(),
                    "required": target_schema()["required"].clone()
                }
            },
            "required": ["from_target", "to_target"]
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
        assert!(click.input_schema["properties"]["target"]["properties"]["selector"].is_object());
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

        assert!(find.input_schema["properties"]["target"]["properties"]["app"]["oneOf"].is_array());
        assert!(
            find.input_schema["properties"]["target"]["properties"]["window"]["oneOf"].is_array()
        );
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
    fn test_action_target_schema_requires_nested_selector() {
        let click = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_click")
            .unwrap();

        let target_schema = &click.input_schema["properties"]["target"];
        assert_eq!(target_schema["required"][0], "selector");
        assert!(target_schema.get("oneOf").is_none());
    }

    #[test]
    fn test_drag_schema_uses_from_and_to_target_shapes() {
        let drag = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_drag")
            .unwrap();

        assert!(
            drag.input_schema["properties"]["from_target"]["properties"]["selector"].is_object()
        );
        assert!(drag.input_schema["properties"]["to_target"]["properties"]["selector"].is_object());
        assert_eq!(drag.input_schema["required"][0], "from_target");
        assert_eq!(drag.input_schema["required"][1], "to_target");
    }

    #[test]
    fn test_find_and_inspect_schema_wrap_nested_target_at_root() {
        let tools = all_tools();
        let find = tools
            .iter()
            .find(|tool| tool.name == "sootie_find")
            .unwrap();
        let inspect = tools
            .iter()
            .find(|tool| tool.name == "sootie_inspect")
            .unwrap();

        for tool in [find, inspect] {
            assert_eq!(tool.input_schema["required"][0], "target");
            assert!(
                tool.input_schema["properties"]["target"]["properties"]["selector"].is_object()
            );
        }
    }

    #[test]
    fn test_screenshot_schema_uses_scope_not_target() {
        let screenshot = all_tools()
            .into_iter()
            .find(|tool| tool.name == "sootie_screenshot")
            .unwrap();

        assert!(screenshot.input_schema["properties"]["scope"]["properties"]["app"].is_object());
        assert!(screenshot.input_schema["properties"]
            .get("target")
            .is_none());
    }

    #[test]
    fn test_scroll_and_type_require_target() {
        let tools = all_tools();
        let scroll = tools
            .iter()
            .find(|tool| tool.name == "sootie_scroll")
            .unwrap();
        let r#type = tools
            .iter()
            .find(|tool| tool.name == "sootie_type")
            .unwrap();

        assert!(scroll.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "target"));
        assert!(r#type.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "target"));
        assert!(r#type.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "text"));
    }

    #[test]
    fn test_target_schema_round_trips_into_target_type() {
        let value = serde_json::json!({
            "app": { "name": "Chrome", "is_frontmost": true },
            "window": { "title": "Inbox", "focused": true },
            "selector": { "role": "button", "name": "Compose" }
        });

        let target: sootie_core::selector::Target = serde_json::from_value(value).unwrap();
        assert_eq!(target.app.unwrap().name.as_deref(), Some("Chrome"));
        assert_eq!(target.window.unwrap().title.as_deref(), Some("Inbox"));
        assert_eq!(target.selector.role.as_deref(), Some("button"));
    }

    #[test]
    fn test_scope_schema_round_trips_into_scope_type() {
        let value = serde_json::json!({
            "app": { "bundle_id": "com.apple.Safari" },
            "window": { "id": "win_42" }
        });

        let scope: sootie_core::selector::Scope = serde_json::from_value(value).unwrap();
        assert_eq!(
            scope.app.unwrap().bundle_id.as_deref(),
            Some("com.apple.Safari")
        );
        assert_eq!(scope.window.unwrap().id.as_deref(), Some("win_42"));
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
