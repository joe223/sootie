use sootie_core::action::{MouseButton, ScrollDirection};
use sootie_core::recipe::StepTarget;

use crate::types::ToolDefinition;

pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        system_capabilities(),
        system_last_report(),
        perception_context(),
        perception_find_apps(),
        perception_find(),
        perception_find_element(),
        action_click(),
        action_type(),
        action_press(),
        action_hotkey(),
        action_scroll(),
        action_drag(),
        perception_save_screenshot(),
        app_launch(),
        window_focus(),
        window_op(),
        workflow_recipes(),
        workflow_run(),
        workflow_recipe_save(),
        workflow_recipe_delete(),
    ]
}

fn system_last_report() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_last_report".to_string(),
        description: "Return the most recent tool execution report for debugging and recovery. Reports include sanitized arguments, success status, elapsed time, error code, and error details.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn system_capabilities() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_capabilities".to_string(),
        description: "Describe this Sootie server's platform support, degraded capabilities, and the recommended agent workflow. Call this once before planning desktop automation on a new host.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {}
        }),
    }
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
fn perception_find_element() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_find_element".to_string(),
        description: "Find UI elements from a short element description. Returns element positions and metadata."
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "el_description": {
                    "type": "string",
                    "description": "Short element description for locating the target element"
                },
                "window": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "app": {
                            "type": "string",
                            "description": "Optional app name to focus and scope before finding"
                        },
                        "windowId": {
                            "type": "string",
                            "description": "Optional window identifier to focus and scope before finding"
                        }
                    }
                }
            },
            "required": ["el_description"]
        }),
    }
}

fn perception_find() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_find".to_string(),
        description: "Find UI elements with the canonical structured target object. Prefer this over natural-language element descriptions when you have app, window, role, name, text, or id fields.".to_string(),
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

fn description_window_scope_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "app": {
                "type": "string",
                "description": "Optional app name to focus and scope before finding"
            },
            "windowId": {
                "type": "string",
                "description": "Optional window identifier to focus and scope before finding"
            }
        }
    })
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

fn coordinate_target_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "coordinate": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "x": { "type": "number", "description": "Absolute screen X coordinate" },
                    "y": { "type": "number", "description": "Absolute screen Y coordinate" }
                },
                "required": ["x", "y"]
            }
        },
        "required": ["coordinate"]
    })
}

fn action_target_schema() -> serde_json::Value {
    serde_json::json!({
        "oneOf": [
            target_schema(),
            coordinate_target_schema()
        ]
    })
}

fn action_click() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_click".to_string(),
        description: "Click a canonical structured target. Resolves the target first, reports the selected backend and coordinates, then performs the click.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Canonical action target: structured element target or coordinate fallback",
                    "allOf": [action_target_schema()]
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
        description: "Type text. If target is provided, resolves the structured element target or coordinate first; otherwise types into the currently focused element.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Optional canonical action target to type into",
                    "allOf": [action_target_schema()]
                },
                "text": {
                    "type": "string",
                    "description": "Text to type"
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
        description: "Press a key. If target is provided, resolves and clicks the structured element target or coordinate first, then presses the key.".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "target": {
                    "description": "Optional canonical action target to focus before pressing",
                    "allOf": [action_target_schema()]
                },
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

fn action_drag() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_drag".to_string(),
        description: "Drag between two resolved targets".to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "from_target": {
                    "description": "Source canonical action target",
                    "allOf": [action_target_schema()]
                },
                "to_target": {
                    "description": "Destination canonical action target",
                    "allOf": [action_target_schema()]
                }
            },
            "required": ["from_target", "to_target"]
        }),
    }
}

fn perception_save_screenshot() -> ToolDefinition {
    ToolDefinition {
        name: "sootie_save_screenshot".to_string(),
        description: "Capture the current scoped window or screen and save it as a PNG file"
            .to_string(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute destination path for the screenshot PNG"
                },
                "window": description_window_scope_schema()
            },
            "required": ["path"]
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
    serde_json::from_value::<StepTarget>(value.clone()).ok()
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
        assert!(names.contains(&"sootie_capabilities"));
        assert!(names.contains(&"sootie_last_report"));
        assert!(names.contains(&"sootie_context"));
        assert!(names.contains(&"sootie_find_apps"));
        assert!(names.contains(&"sootie_find"));
        assert!(names.contains(&"sootie_find_element"));
        assert!(names.contains(&"sootie_click"));
        assert!(names.contains(&"sootie_type"));
        assert!(names.contains(&"sootie_press"));
        assert!(names.contains(&"sootie_hotkey"));
        assert!(names.contains(&"sootie_scroll"));
        assert!(names.contains(&"sootie_drag"));
        assert!(names.contains(&"sootie_save_screenshot"));
        assert!(names.contains(&"sootie_focus"));
        assert!(names.contains(&"sootie_window"));
        assert!(names.contains(&"sootie_recipes"));
        assert!(names.contains(&"sootie_run"));
        assert!(names.contains(&"sootie_recipe_save"));
        assert!(names.contains(&"sootie_recipe_delete"));
    }

    #[test]
    fn test_description_based_tools_require_el_description() {
        let tools = all_tools();

        let tool = tools
            .iter()
            .find(|tool| tool.name == "sootie_find_element")
            .unwrap();
        assert!(tool.input_schema["properties"]["el_description"].is_object());
        assert!(tool.input_schema["properties"]["window"].is_object());
        assert!(tool.input_schema["properties"].get("name").is_none());
        assert!(tool.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "el_description"));
    }

    #[test]
    fn test_selector_and_numeric_schemas_are_typed() {
        let tools = all_tools();
        let find_apps = tools
            .iter()
            .find(|tool| tool.name == "sootie_find_apps")
            .unwrap();
        let scroll = tools
            .iter()
            .find(|tool| tool.name == "sootie_scroll")
            .unwrap();

        assert!(find_apps.input_schema["properties"]["pattern"]["type"] == "string");
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
    fn test_scroll_requires_target_and_type_accepts_optional_target() {
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
            .any(|value| value == "text"));
        assert!(r#type.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value != "field"));
        assert!(r#type.input_schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .all(|value| value != "target"));
        assert!(
            r#type.input_schema["properties"]["target"]["allOf"][0]["oneOf"][0]["properties"]
                ["selector"]
                .is_object()
        );
        assert!(r#type.input_schema["properties"].get("field").is_none());
        assert!(r#type.input_schema["properties"].get("window").is_none());
    }

    #[test]
    fn test_action_tools_use_canonical_targets() {
        let tools = all_tools();
        let click = tools
            .iter()
            .find(|tool| tool.name == "sootie_click")
            .unwrap();
        let press = tools
            .iter()
            .find(|tool| tool.name == "sootie_press")
            .unwrap();
        let drag = tools
            .iter()
            .find(|tool| tool.name == "sootie_drag")
            .unwrap();

        assert!(
            click.input_schema["properties"]["target"]["allOf"][0]["oneOf"][0]["properties"]
                ["selector"]
                .is_object()
        );
        assert!(
            click.input_schema["properties"]["target"]["allOf"][0]["oneOf"][1]["properties"]
                ["coordinate"]
                .is_object()
        );
        assert!(
            press.input_schema["properties"]["target"]["allOf"][0]["oneOf"][0]["properties"]
                ["selector"]
                .is_object()
        );
        assert!(
            drag.input_schema["properties"]["from_target"]["allOf"][0]["oneOf"][1]["properties"]
                ["coordinate"]
                .is_object()
        );
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
            "selector": {
                "role": "button",
                "name": "Submit"
            }
        });
        let target = parse_step_target(&val).unwrap();
        match target {
            StepTarget::Target(target) => {
                assert_eq!(target.selector.role, Some("button".to_string()));
            }
            _ => panic!("expected structured target"),
        }
    }
}
