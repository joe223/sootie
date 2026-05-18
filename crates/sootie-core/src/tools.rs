use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    pub annotations: ToolAnnotations,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolAnnotations {
    #[serde(rename = "readOnlyHint")]
    pub read_only_hint: bool,
    #[serde(rename = "destructiveHint")]
    pub destructive_hint: bool,
    #[serde(rename = "idempotentHint")]
    pub idempotent_hint: bool,
    #[serde(rename = "openWorldHint")]
    pub open_world_hint: bool,
}

pub const TOOL_NAMES: &[&str] = &[
    "sootie_context",
    "sootie_state",
    "sootie_find",
    "sootie_read",
    "sootie_inspect",
    "sootie_element_at",
    "sootie_screenshot",
    "sootie_click",
    "sootie_type",
    "sootie_press",
    "sootie_hotkey",
    "sootie_scroll",
    "sootie_hover",
    "sootie_long_press",
    "sootie_drag",
    "sootie_focus",
    "sootie_window",
    "sootie_wait",
    "sootie_recipes",
    "sootie_run",
    "sootie_recipe_show",
    "sootie_recipe_save",
    "sootie_recipe_delete",
    "sootie_parse_screen",
    "sootie_ground",
    "sootie_annotate",
    "sootie_learn_start",
    "sootie_learn_stop",
    "sootie_learn_status",
];

pub fn tool_definitions() -> Vec<ToolDefinition> {
    TOOL_NAMES
        .iter()
        .map(|name| tool_definition(name))
        .collect()
}

fn tool_definition(name: &str) -> ToolDefinition {
    match name {
        "sootie_context" => tool(
            name,
            "Get orientation: app, window, URL, focused element, and interactive elements.",
            optional_app_props(&[]),
            &[],
        ),
        "sootie_state" => tool(
            name,
            "List running apps and windows.",
            optional_app_props(&[]),
            &[],
        ),
        "sootie_find" => tool(
            name,
            "Find elements by query, role, DOM id/class, identifier, or app.",
            find_props(),
            &[],
        ),
        "sootie_read" => tool(
            name,
            "Read visible text from an app or matching element.",
            read_props(),
            &[],
        ),
        "sootie_inspect" => tool(
            name,
            "Inspect one matching element.",
            inspect_props(),
            &["query"],
        ),
        "sootie_element_at" => tool(
            name,
            "Describe the element at a screen coordinate.",
            props(&[
                ("x", "number", "X coordinate."),
                ("y", "number", "Y coordinate."),
            ]),
            &["x", "y"],
        ),
        "sootie_screenshot" => tool(
            name,
            "Capture a screenshot for visual debugging.",
            optional_app_props(&[(
                "full_resolution",
                "boolean",
                "Request native-resolution capture when supported.",
            )]),
            &[],
        ),
        "sootie_annotate" => tool(
            name,
            "Return a labeled text index of interactive elements.",
            optional_app_props(&[
                ("roles", "array", "Roles to include."),
                ("max_labels", "integer", "Maximum labels."),
            ]),
            &[],
        ),
        "sootie_ground" => tool(
            name,
            "Ground a described UI target to ranked candidates, falling back to vision coordinates when platform and CDP lookup miss.",
            ground_props(),
            &["description"],
        ),
        "sootie_parse_screen" => tool(
            name,
            "Detect visible interactive elements from platform context.",
            optional_app_props(&[(
                "full_resolution",
                "boolean",
                "Request native-resolution capture when supported.",
            )]),
            &[],
        ),
        "sootie_click" => tool(
            name,
            "Click by element query or coordinates.",
            action_props(&[
                ("button", "string", "left/right/middle."),
                ("count", "integer", "Click count."),
            ]),
            &[],
        ),
        "sootie_hover" => tool(
            name,
            "Move cursor to an element or coordinate.",
            action_props(&[]),
            &[],
        ),
        "sootie_long_press" => tool(
            name,
            "Press and hold on an element or coordinate.",
            action_props(&[
                ("duration", "number", "Hold duration seconds."),
                ("button", "string", "left/right/middle."),
            ]),
            &[],
        ),
        "sootie_drag" => tool(
            name,
            "Drag from a point or element to a destination.",
            drag_props(),
            &["to_x", "to_y"],
        ),
        "sootie_type" => tool(
            name,
            "Type text into current focus or target field.",
            type_props(),
            &["text"],
        ),
        "sootie_press" => tool(
            name,
            "Press one key.",
            optional_app_props(&[
                ("key", "string", "Key name."),
                ("modifiers", "array", "Modifier keys."),
            ]),
            &["key"],
        ),
        "sootie_hotkey" => tool(
            name,
            "Press a key combination.",
            optional_app_props(&[("keys", "array", "Keys like [cmd,l].")]),
            &["keys"],
        ),
        "sootie_scroll" => tool(
            name,
            "Scroll in a direction.",
            scroll_props(),
            &["direction"],
        ),
        "sootie_focus" => tool(
            name,
            "Focus an app or window.",
            app_destination_props(&[("window", "string", "Window title substring.")]),
            &["app"],
        ),
        "sootie_window" => tool(
            name,
            "Window management: list/focus/minimize/maximize/restore/close/move/resize.",
            window_props(),
            &["action", "app"],
        ),
        "sootie_wait" => tool(
            name,
            "Wait for title/url/element conditions instead of fixed sleeps.",
            wait_props(),
            &["condition"],
        ),
        "sootie_recipes" => tool(name, "List installed recipes.", json!({}), &[]),
        "sootie_run" => tool(
            name,
            "Run a saved recipe with params.",
            props(&[
                ("recipe", "string", "Recipe name."),
                ("params", "object", "Parameter values."),
            ]),
            &["recipe"],
        ),
        "sootie_recipe_show" => tool(
            name,
            "Show a saved recipe.",
            props(&[("name", "string", "Recipe name.")]),
            &["name"],
        ),
        "sootie_recipe_save" => tool(
            name,
            "Save a recipe JSON object or JSON string.",
            recipe_save_props(),
            &["recipe_json"],
        ),
        "sootie_recipe_delete" => tool(
            name,
            "Delete a saved recipe.",
            props(&[("name", "string", "Recipe name.")]),
            &["name"],
        ),
        "sootie_learn_start" => tool(
            name,
            "Start learning mode and record successful Sootie actions in this session.",
            props(&[(
                "task_description",
                "string",
                "Task label for this recording.",
            )]),
            &[],
        ),
        "sootie_learn_stop" => tool(
            name,
            "Stop learning mode and return recorded Sootie actions.",
            json!({}),
            &[],
        ),
        "sootie_learn_status" => tool(name, "Report learning mode status.", json!({}), &[]),
        _ => unreachable!(),
    }
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDefinition {
    let mut schema = json!({
        "type": "object",
        "properties": properties,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        input_schema: schema,
        annotations: annotations_for(name),
    }
}

fn annotations_for(name: &str) -> ToolAnnotations {
    match name {
        "sootie_context"
        | "sootie_state"
        | "sootie_find"
        | "sootie_read"
        | "sootie_inspect"
        | "sootie_element_at"
        | "sootie_screenshot"
        | "sootie_annotate"
        | "sootie_ground"
        | "sootie_parse_screen"
        | "sootie_wait"
        | "sootie_recipes"
        | "sootie_recipe_show"
        | "sootie_learn_status" => ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        },
        "sootie_hover" | "sootie_focus" | "sootie_learn_start" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: true,
        },
        "sootie_recipe_save" | "sootie_learn_stop" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        },
        "sootie_recipe_delete" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: true,
            open_world_hint: false,
        },
        _ => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
            open_world_hint: true,
        },
    }
}

fn props(entries: &[(&str, &str, &str)]) -> Value {
    let mut map = serde_json::Map::new();
    for (name, ty, description) in entries {
        map.insert((*name).to_string(), typed_schema(ty, description));
    }
    Value::Object(map)
}

fn typed_schema(ty: &str, description: &str) -> Value {
    if ty == "array" {
        json!({ "type": "array", "items": { "type": "string" }, "description": description })
    } else {
        json!({ "type": ty, "description": description })
    }
}

fn app_destination_props(extra: &[(&str, &str, &str)]) -> Value {
    let mut map = props(&[("app", "string", "App name.")])
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (name, ty, description) in extra {
        map.insert((*name).to_string(), typed_schema(ty, description));
    }
    Value::Object(map)
}

fn optional_app_props(extra: &[(&str, &str, &str)]) -> Value {
    app_destination_props(extra)
}

fn window_props() -> Value {
    app_destination_props(&[
        ("action", "string", "Window action."),
        ("window", "string", "Window title."),
        ("x", "number", "X."),
        ("y", "number", "Y."),
        ("width", "number", "Width."),
        ("height", "number", "Height."),
    ])
}

fn find_props() -> Value {
    props(&[
        ("query", "string", "Text/name search."),
        ("role", "string", "Role filter."),
        ("dom_id", "string", "DOM id."),
        ("dom_class", "string", "DOM class."),
        ("identifier", "string", "Native identifier."),
        ("app", "string", "App name."),
        ("depth", "integer", "Depth hint."),
    ])
}

fn read_props() -> Value {
    props(&[
        ("app", "string", "Optional app."),
        ("query", "string", "Optional text filter."),
        ("depth", "integer", "Traversal depth hint."),
    ])
}

fn inspect_props() -> Value {
    props(&[
        ("query", "string", "Element to inspect."),
        ("role", "string", "Role filter."),
        ("dom_id", "string", "DOM id."),
        ("app", "string", "App name."),
    ])
}

fn action_props(extra: &[(&str, &str, &str)]) -> Value {
    let mut map = props(&[
        ("query", "string", "Element query."),
        ("role", "string", "Role filter."),
        ("dom_id", "string", "DOM id."),
        ("app", "string", "App name."),
        ("x", "number", "X coordinate."),
        ("y", "number", "Y coordinate."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    for (name, ty, description) in extra {
        map.insert(
            (*name).to_string(),
            json!({ "type": ty, "description": description }),
        );
    }
    Value::Object(map)
}

fn type_props() -> Value {
    props(&[
        ("text", "string", "Text to type."),
        ("into", "string", "Target field query."),
        ("dom_id", "string", "Target DOM id."),
        ("app", "string", "Optional app."),
        ("clear", "boolean", "Clear first."),
    ])
}

fn scroll_props() -> Value {
    props(&[
        ("direction", "string", "up/down/left/right."),
        ("amount", "integer", "Scroll amount."),
        ("app", "string", "Optional app."),
        ("x", "number", "X coordinate."),
        ("y", "number", "Y coordinate."),
    ])
}

fn wait_props() -> Value {
    props(&[
        (
            "condition",
            "string",
            "urlContains/titleContains/elementExists/elementGone/urlChanged/titleChanged.",
        ),
        ("value", "string", "Match value."),
        ("timeout", "number", "Seconds."),
        ("interval", "number", "Seconds."),
        ("app", "string", "Optional app."),
    ])
}

fn ground_props() -> Value {
    let mut map = props(&[
        ("description", "string", "Visual target description."),
        ("app", "string", "Optional app."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    map.insert(
        "crop_box".to_string(),
        json!({
            "type": "array",
            "items": { "type": "number" },
            "description": "Optional [x1,y1,x2,y2] crop region in screen coordinates."
        }),
    );
    Value::Object(map)
}

fn drag_props() -> Value {
    props(&[
        ("from_x", "number", "Start X."),
        ("from_y", "number", "Start Y."),
        ("to_x", "number", "End X."),
        ("to_y", "number", "End Y."),
        ("query", "string", "Drag source query."),
        ("role", "string", "Role filter."),
        ("dom_id", "string", "DOM id."),
        ("app", "string", "App name."),
        ("duration", "number", "Duration seconds."),
        ("hold_duration", "number", "Initial hold seconds."),
    ])
}

fn recipe_save_props() -> Value {
    props(&[("recipe_json", "string", "Complete recipe JSON string.")])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn property_names(tool: &ToolDefinition) -> Vec<String> {
        let mut names = tool.input_schema["properties"]
            .as_object()
            .expect("properties object")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        names.sort();
        names
    }

    fn required_names(tool: &ToolDefinition) -> Vec<String> {
        tool.input_schema
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().expect("required name").to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    fn assert_contract(tool: &ToolDefinition, properties: &[&str], required: &[&str]) {
        let mut expected_properties = properties
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        expected_properties.sort();
        assert_eq!(property_names(tool), expected_properties, "{}", tool.name);
        assert_eq!(
            required_names(tool),
            required
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
            "{}",
            tool.name
        );
    }

    fn assert_property_type(tool: &ToolDefinition, name: &str, ty: &str) {
        assert_eq!(
            tool.input_schema["properties"][name]["type"], ty,
            "{}.{name}",
            tool.name
        );
    }

    fn assert_array_item_type(tool: &ToolDefinition, name: &str, item_ty: &str) {
        assert_property_type(tool, name, "array");
        assert_eq!(
            tool.input_schema["properties"][name]["items"]["type"], item_ty,
            "{}.{name}[]",
            tool.name
        );
    }

    fn tool_by_name<'a>(tools: &'a [ToolDefinition], name: &str) -> &'a ToolDefinition {
        tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("tool exists")
    }

    #[test]
    fn exposes_sootie_tool_surface() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 29);
        assert!(tools.iter().any(|tool| tool.name == "sootie_context"));
        assert!(tools.iter().any(|tool| tool.name == "sootie_learn_status"));
        assert!(!tools.iter().any(|tool| tool.name == "sootie_launch"));
    }

    #[test]
    fn tool_contract_is_exact() {
        let tools = tool_definitions();
        assert_eq!(TOOL_NAMES.len(), 29);
        assert_eq!(
            tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            TOOL_NAMES
        );

        macro_rules! app_scope {
            ($($name:literal),* $(,)?) => {
                &["app", $($name),*][..]
            };
        }

        let expected = [
            ("sootie_context", app_scope![], &[][..]),
            ("sootie_state", app_scope![], &[]),
            (
                "sootie_find",
                app_scope![
                    "query",
                    "role",
                    "dom_id",
                    "dom_class",
                    "identifier",
                    "depth",
                ],
                &[],
            ),
            ("sootie_read", app_scope!["query", "depth"], &[]),
            (
                "sootie_inspect",
                app_scope!["query", "role", "dom_id"],
                &["query"],
            ),
            ("sootie_element_at", &["x", "y"], &["x", "y"]),
            ("sootie_screenshot", app_scope!["full_resolution"], &[]),
            (
                "sootie_click",
                app_scope!["query", "role", "dom_id", "x", "y", "button", "count",],
                &[],
            ),
            (
                "sootie_type",
                app_scope!["text", "into", "dom_id", "clear"],
                &["text"],
            ),
            ("sootie_press", app_scope!["key", "modifiers"], &["key"]),
            ("sootie_hotkey", app_scope!["keys"], &["keys"]),
            (
                "sootie_scroll",
                app_scope!["direction", "amount", "x", "y"],
                &["direction"],
            ),
            (
                "sootie_hover",
                app_scope!["query", "role", "dom_id", "x", "y"],
                &[],
            ),
            (
                "sootie_long_press",
                app_scope!["query", "role", "dom_id", "x", "y", "duration", "button",],
                &[],
            ),
            (
                "sootie_drag",
                app_scope![
                    "from_x",
                    "from_y",
                    "to_x",
                    "to_y",
                    "query",
                    "role",
                    "dom_id",
                    "duration",
                    "hold_duration",
                ],
                &["to_x", "to_y"],
            ),
            ("sootie_focus", app_scope!["window"], &["app"]),
            (
                "sootie_window",
                app_scope!["action", "window", "x", "y", "width", "height"],
                &["action", "app"],
            ),
            (
                "sootie_wait",
                app_scope!["condition", "value", "timeout", "interval"],
                &["condition"],
            ),
            ("sootie_recipes", &[], &[]),
            ("sootie_run", &["recipe", "params"], &["recipe"]),
            ("sootie_recipe_show", &["name"], &["name"]),
            ("sootie_recipe_save", &["recipe_json"], &["recipe_json"]),
            ("sootie_recipe_delete", &["name"], &["name"]),
            ("sootie_parse_screen", app_scope!["full_resolution"], &[]),
            (
                "sootie_ground",
                app_scope!["description", "crop_box"],
                &["description"],
            ),
            ("sootie_annotate", app_scope!["roles", "max_labels"], &[]),
            ("sootie_learn_start", &["task_description"], &[]),
            ("sootie_learn_stop", &[], &[]),
            ("sootie_learn_status", &[], &[]),
        ];

        assert_eq!(
            expected
                .iter()
                .map(|(name, _, _)| *name)
                .collect::<Vec<_>>(),
            TOOL_NAMES
        );
        for (name, properties, required) in expected {
            let tool = tools
                .iter()
                .find(|tool| tool.name == name)
                .expect("tool exists");
            assert_contract(tool, properties, required);
        }
    }

    #[test]
    fn required_fields_match_public_compatibility_contract() {
        let tools = tool_definitions();
        let expected = [
            ("sootie_context", &[][..]),
            ("sootie_state", &[]),
            ("sootie_find", &[]),
            ("sootie_read", &[]),
            ("sootie_inspect", &["query"][..]),
            ("sootie_element_at", &["x", "y"]),
            ("sootie_screenshot", &[]),
            ("sootie_click", &[]),
            ("sootie_type", &["text"]),
            ("sootie_press", &["key"]),
            ("sootie_hotkey", &["keys"]),
            ("sootie_scroll", &["direction"]),
            ("sootie_hover", &[]),
            ("sootie_long_press", &[]),
            ("sootie_drag", &["to_x", "to_y"]),
            ("sootie_focus", &["app"]),
            ("sootie_window", &["action", "app"]),
            ("sootie_wait", &["condition"]),
            ("sootie_recipes", &[]),
            ("sootie_run", &["recipe"]),
            ("sootie_recipe_show", &["name"]),
            ("sootie_recipe_save", &["recipe_json"]),
            ("sootie_recipe_delete", &["name"]),
            ("sootie_parse_screen", &[]),
            ("sootie_ground", &["description"]),
            ("sootie_annotate", &[]),
            ("sootie_learn_start", &[]),
            ("sootie_learn_stop", &[]),
            ("sootie_learn_status", &[]),
        ];

        assert_eq!(
            expected.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            TOOL_NAMES
        );
        for (name, required) in expected {
            assert_eq!(
                required_names(tool_by_name(&tools, name)),
                required,
                "{name}"
            );
        }
    }

    #[test]
    fn property_types_match_public_compatibility_contract() {
        let tools = tool_definitions();
        let expected = [
            ("sootie_context", &[("app", "string")][..], &[][..]),
            ("sootie_state", &[("app", "string")][..], &[][..]),
            (
                "sootie_find",
                &[
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("dom_class", "string"),
                    ("identifier", "string"),
                    ("app", "string"),
                    ("depth", "integer"),
                ][..],
                &[][..],
            ),
            (
                "sootie_read",
                &[("app", "string"), ("query", "string"), ("depth", "integer")],
                &[],
            ),
            (
                "sootie_inspect",
                &[
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                ],
                &[],
            ),
            (
                "sootie_element_at",
                &[("x", "number"), ("y", "number")],
                &[],
            ),
            (
                "sootie_screenshot",
                &[("app", "string"), ("full_resolution", "boolean")],
                &[],
            ),
            (
                "sootie_click",
                &[
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                    ("x", "number"),
                    ("y", "number"),
                    ("button", "string"),
                    ("count", "integer"),
                ],
                &[],
            ),
            (
                "sootie_type",
                &[
                    ("text", "string"),
                    ("into", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                    ("clear", "boolean"),
                ],
                &[],
            ),
            (
                "sootie_press",
                &[("key", "string"), ("app", "string")],
                &[("modifiers", "string")],
            ),
            ("sootie_hotkey", &[("app", "string")], &[("keys", "string")]),
            (
                "sootie_scroll",
                &[
                    ("direction", "string"),
                    ("amount", "integer"),
                    ("app", "string"),
                    ("x", "number"),
                    ("y", "number"),
                ],
                &[],
            ),
            (
                "sootie_hover",
                &[
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                    ("x", "number"),
                    ("y", "number"),
                ],
                &[],
            ),
            (
                "sootie_long_press",
                &[
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                    ("x", "number"),
                    ("y", "number"),
                    ("duration", "number"),
                    ("button", "string"),
                ],
                &[],
            ),
            (
                "sootie_drag",
                &[
                    ("from_x", "number"),
                    ("from_y", "number"),
                    ("to_x", "number"),
                    ("to_y", "number"),
                    ("query", "string"),
                    ("role", "string"),
                    ("dom_id", "string"),
                    ("app", "string"),
                    ("duration", "number"),
                    ("hold_duration", "number"),
                ],
                &[],
            ),
            (
                "sootie_focus",
                &[("app", "string"), ("window", "string")],
                &[],
            ),
            (
                "sootie_window",
                &[
                    ("action", "string"),
                    ("app", "string"),
                    ("window", "string"),
                    ("x", "number"),
                    ("y", "number"),
                    ("width", "number"),
                    ("height", "number"),
                ],
                &[],
            ),
            (
                "sootie_wait",
                &[
                    ("condition", "string"),
                    ("value", "string"),
                    ("timeout", "number"),
                    ("interval", "number"),
                    ("app", "string"),
                ],
                &[],
            ),
            ("sootie_recipes", &[], &[]),
            (
                "sootie_run",
                &[("recipe", "string"), ("params", "object")],
                &[],
            ),
            ("sootie_recipe_show", &[("name", "string")], &[]),
            ("sootie_recipe_save", &[("recipe_json", "string")], &[]),
            ("sootie_recipe_delete", &[("name", "string")], &[]),
            (
                "sootie_parse_screen",
                &[("app", "string"), ("full_resolution", "boolean")],
                &[],
            ),
            (
                "sootie_ground",
                &[("description", "string"), ("app", "string")],
                &[("crop_box", "number")],
            ),
            (
                "sootie_annotate",
                &[("app", "string"), ("max_labels", "integer")],
                &[("roles", "string")],
            ),
            ("sootie_learn_start", &[("task_description", "string")], &[]),
            ("sootie_learn_stop", &[], &[]),
            ("sootie_learn_status", &[], &[]),
        ];

        for (name, scalar_properties, array_properties) in expected {
            let tool = tool_by_name(&tools, name);
            for (property, ty) in scalar_properties {
                assert_property_type(tool, property, ty);
            }
            for (property, item_ty) in array_properties {
                assert_array_item_type(tool, property, item_ty);
            }
        }
    }

    #[test]
    fn ground_crop_box_is_numeric() {
        let tools = tool_definitions();
        let ground = tools
            .iter()
            .find(|tool| tool.name == "sootie_ground")
            .expect("ground tool exists");
        assert_eq!(
            ground.input_schema["properties"]["crop_box"]["items"]["type"],
            "number"
        );
    }

    #[test]
    fn advertised_shapes_do_not_expand_public_compatibility_parameters() {
        let tools = tool_definitions();
        let forbidden = [
            "target",
            "from_target",
            "to_target",
            "el_description",
            "platform_app_id",
            "to_platform_app_id",
            "bundle_id",
            "to_bundle_id",
            "to_app",
            "clear_first",
            "duration_ms",
            "hold_duration_ms",
            "timeout_ms",
            "interval_ms",
            "bounds",
            "max_candidates",
            "fullResolution",
            "domId",
        ];
        for tool in &tools {
            for name in forbidden {
                assert!(
                    tool.input_schema["properties"][name].is_null(),
                    "{} unexpectedly advertises {name}",
                    tool.name
                );
            }
            if tool.name != "sootie_ground" {
                assert!(
                    tool.input_schema["properties"]["description"].is_null(),
                    "{} unexpectedly advertises description",
                    tool.name
                );
            }
            if !tool.input_schema["properties"]["app"].is_null() {
                assert_eq!(
                    tool.input_schema["properties"]["app"]["type"], "string",
                    "{} app parameter must match public compatibility string shape",
                    tool.name
                );
                assert!(tool.input_schema["properties"]["app"]["oneOf"].is_null());
            }
        }

        let press = tool_by_name(&tools, "sootie_press");
        assert_eq!(
            press.input_schema["properties"]["modifiers"]["items"]["type"],
            "string"
        );

        let hotkey = tool_by_name(&tools, "sootie_hotkey");
        assert_eq!(
            hotkey.input_schema["properties"]["keys"]["items"]["type"],
            "string"
        );

        let annotate = tool_by_name(&tools, "sootie_annotate");
        assert_eq!(
            annotate.input_schema["properties"]["roles"]["items"]["type"],
            "string"
        );

        let recipe_save = tool_by_name(&tools, "sootie_recipe_save");
        assert!(recipe_save.input_schema["properties"]["recipe"].is_null());
        assert_eq!(
            recipe_save.input_schema["properties"]["recipe_json"]["type"],
            "string"
        );
        assert_eq!(recipe_save.input_schema["required"][0], "recipe_json");
    }

    #[test]
    fn advertised_annotations_separate_read_only_and_mutating_tools() {
        let tools = tool_definitions();
        let status = tool_by_name(&tools, "sootie_learn_status");
        assert!(status.annotations.read_only_hint);
        assert!(!status.annotations.destructive_hint);
        assert!(status.annotations.idempotent_hint);
        assert!(!status.annotations.open_world_hint);

        let click = tool_by_name(&tools, "sootie_click");
        assert!(!click.annotations.read_only_hint);
        assert!(click.annotations.destructive_hint);
        assert!(!click.annotations.idempotent_hint);
        assert!(click.annotations.open_world_hint);

        let recipe_save = tool_by_name(&tools, "sootie_recipe_save");
        assert!(!recipe_save.annotations.read_only_hint);
        assert!(!recipe_save.annotations.destructive_hint);
        assert!(!recipe_save.annotations.open_world_hint);
    }
}
