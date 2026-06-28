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
    "context",
    "state",
    "find",
    "read",
    "inspect",
    "element_at",
    "screenshot",
    "click",
    "type",
    "press",
    "hotkey",
    "scroll",
    "hover",
    "long_press",
    "drag",
    "focus",
    "window",
    "wait",
    "recipes",
    "run",
    "recipe_show",
    "recipe_save",
    "recipe_delete",
    "parse_screen",
    "ground",
    "annotate",
    "browser_launch",
    "browser_connect",
    "browser_pages",
    "browser_select_page",
    "browser_open",
    "browser_observe",
    "browser_viewport",
    "browser_find",
    "browser_click",
    "browser_type",
    "browser_press",
    "browser_scroll",
    "browser_wait",
    "browser_extract",
    "browser_screenshot",
    "browser_back",
    "browser_forward",
    "browser_reload",
    "browser_close_page",
    "browser_shutdown",
    "browser_network",
    "browser_console",
    "browser_storage",
    "browser_cookies",
    "browser_downloads",
    "browser_upload",
    "browser_pdf",
    "cdp_send",
    "cdp_subscribe",
    "learn_start",
    "learn_stop",
    "learn_status",
];

pub fn canonical_tool_name(name: &str) -> &str {
    name.strip_prefix("sootie_").unwrap_or(name)
}

pub fn legacy_tool_name(name: &str) -> String {
    if name.starts_with("sootie_") {
        name.to_string()
    } else {
        format!("sootie_{name}")
    }
}

pub fn tool_definitions() -> Vec<ToolDefinition> {
    TOOL_NAMES
        .iter()
        .map(|name| tool_definition(name))
        .collect()
}

fn tool_definition(name: &str) -> ToolDefinition {
    let public_name = canonical_tool_name(name);
    let legacy_name = legacy_tool_name(public_name);
    let mut definition = legacy_tool_definition(&legacy_name);
    definition.name = public_name.to_string();
    definition.annotations = annotations_for(public_name);
    definition
}

fn legacy_tool_definition(name: &str) -> ToolDefinition {
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
            optional_app_props(&[
                ("window", "string", "Window title substring."),
                (
                    "full_resolution",
                    "boolean",
                    "Request native-resolution capture when supported.",
                ),
            ]),
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
            optional_app_props(&[
                ("window", "string", "Window title substring."),
                (
                    "full_resolution",
                    "boolean",
                    "Request native-resolution capture when supported.",
                ),
            ]),
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
        "sootie_browser_connect" => tool(
            name,
            "Connect to a browser CDP endpoint.",
            browser_connect_props(),
            &[],
        ),
        "sootie_browser_launch" => tool(
            name,
            "Launch a browser with a managed CDP endpoint and connect to it. Prefer headless for browser-only work unless the user needs a visible window.",
            browser_launch_props(),
            &[],
        ),
        "sootie_browser_pages" => tool(
            name,
            "List browser pages and tabs from the connected CDP endpoint.",
            browser_session_props(&[("include_inactive", "boolean", "Include inactive pages.")]),
            &[],
        ),
        "sootie_browser_select_page" => tool(
            name,
            "Select the browser page used by subsequent browser-native actions.",
            browser_session_props(&[("page_id", "string", "Browser page id.")]),
            &["page_id"],
        ),
        "sootie_browser_open" => tool(
            name,
            "Open a URL in a new or selected browser page.",
            browser_open_props(),
            &["url"],
        ),
        "sootie_browser_observe" => tool(
            name,
            "Observe browser page state, visible text, and interactive elements.",
            browser_observe_props(),
            &[],
        ),
        "sootie_browser_viewport" => tool(
            name,
            "Read or set the browser page viewport size through CDP. Prefer this over raw cdp_send for responsive layout checks.",
            browser_viewport_props(),
            &[],
        ),
        "sootie_browser_find" => tool(
            name,
            "Find browser elements by ref, selector, role/name/text, DOM id/class, or query.",
            browser_find_props(),
            &[],
        ),
        "sootie_browser_click" => tool(
            name,
            "Click a browser element using CDP without desktop fallback.",
            browser_action_props(&[
                ("button", "string", "left/right/middle."),
                ("count", "integer", "Click count."),
                ("wait_after", "string", "none/load/networkidle/stable."),
            ]),
            &[],
        ),
        "sootie_browser_type" => tool(
            name,
            "Type text into a browser element using CDP.",
            browser_type_props(),
            &["text"],
        ),
        "sootie_browser_press" => tool(
            name,
            "Press a browser key using CDP.",
            browser_session_props(&[
                ("page_id", "string", "Browser page id."),
                ("key", "string", "Key name."),
                ("modifiers", "array", "Modifier keys."),
            ]),
            &["key"],
        ),
        "sootie_browser_scroll" => tool(
            name,
            "Scroll the browser page or a browser element using CDP.",
            browser_scroll_props(),
            &[],
        ),
        "sootie_browser_wait" => tool(
            name,
            "Wait for browser lifecycle, URL/title/text, or element conditions.",
            browser_wait_props(),
            &["condition"],
        ),
        "sootie_browser_extract" => tool(
            name,
            "Extract browser page content as text, markdown, HTML, or JSON.",
            browser_extract_props(),
            &[],
        ),
        "sootie_browser_screenshot" => tool(
            name,
            "Capture a browser page screenshot through CDP.",
            browser_session_props(&[
                ("page_id", "string", "Browser page id."),
                ("full_page", "boolean", "Capture beyond the current viewport when supported."),
                ("format", "string", "png/jpeg."),
            ]),
            &[],
        ),
        "sootie_browser_back" => tool(
            name,
            "Navigate the selected browser page back.",
            browser_history_props(),
            &[],
        ),
        "sootie_browser_forward" => tool(
            name,
            "Navigate the selected browser page forward.",
            browser_history_props(),
            &[],
        ),
        "sootie_browser_reload" => tool(
            name,
            "Reload the selected browser page.",
            browser_history_props(),
            &[],
        ),
        "sootie_browser_close_page" => tool(
            name,
            "Close a browser page by page id.",
            browser_session_props(&[("page_id", "string", "Browser page id.")]),
            &[],
        ),
        "sootie_browser_shutdown" => tool(
            name,
            "Shut down a browser process launched by sootie_browser_launch.",
            browser_shutdown_props(),
            &[],
        ),
        "sootie_browser_network" => tool(
            name,
            "Inspect browser network entries or a guarded response body.",
            browser_network_props(),
            &[],
        ),
        "sootie_browser_console" => tool(
            name,
            "Read browser console entries captured from the page.",
            browser_console_props(),
            &[],
        ),
        "sootie_browser_storage" => tool(
            name,
            "List, read, or mutate localStorage/sessionStorage with policy checks.",
            browser_storage_props(),
            &["area", "action"],
        ),
        "sootie_browser_cookies" => tool(
            name,
            "List or mutate browser cookies with policy checks.",
            browser_cookies_props(),
            &["action"],
        ),
        "sootie_browser_downloads" => tool(
            name,
            "Configure browser download behavior with explicit unsafe opt-in.",
            browser_downloads_props(),
            &["action"],
        ),
        "sootie_browser_upload" => tool(
            name,
            "Set files on a browser file input through CDP with explicit unsafe opt-in.",
            browser_upload_props(),
            &["file_paths"],
        ),
        "sootie_browser_pdf" => tool(
            name,
            "Render the selected browser page to PDF through CDP.",
            browser_pdf_props(),
            &[],
        ),
        "sootie_cdp_send" => tool(
            name,
            "Send a raw CDP command through the guarded browser escape hatch.",
            cdp_send_props(),
            &["method"],
        ),
        "sootie_cdp_subscribe" => tool(
            name,
            "Collect a bounded batch of CDP events through the guarded escape hatch.",
            cdp_subscribe_props(),
            &["domain"],
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
    match canonical_tool_name(name) {
        "context"
        | "state"
        | "find"
        | "read"
        | "inspect"
        | "element_at"
        | "screenshot"
        | "annotate"
        | "ground"
        | "parse_screen"
        | "browser_connect"
        | "browser_pages"
        | "browser_select_page"
        | "browser_observe"
        | "browser_find"
        | "browser_extract"
        | "browser_screenshot"
        | "browser_network"
        | "browser_console"
        | "browser_pdf"
        | "wait"
        | "recipes"
        | "recipe_show"
        | "learn_status" => ToolAnnotations {
            read_only_hint: true,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: false,
        },
        "hover" | "focus" | "learn_start" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: true,
        },
        "recipe_save" | "learn_stop" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: false,
            open_world_hint: false,
        },
        "recipe_delete" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: true,
            open_world_hint: false,
        },
        "browser_viewport" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: false,
            idempotent_hint: true,
            open_world_hint: true,
        },
        "browser_open" | "browser_click" | "browser_type" | "browser_press" | "browser_scroll"
        | "browser_wait" | "browser_back" | "browser_forward" | "browser_reload" => {
            ToolAnnotations {
                read_only_hint: false,
                destructive_hint: false,
                idempotent_hint: false,
                open_world_hint: true,
            }
        }
        "browser_close_page" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: true,
            open_world_hint: true,
        },
        "browser_storage" | "browser_cookies" | "browser_downloads" | "browser_upload"
        | "cdp_send" | "cdp_subscribe" => ToolAnnotations {
            read_only_hint: false,
            destructive_hint: true,
            idempotent_hint: false,
            open_world_hint: true,
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
        ("max_results", "integer", "Maximum results."),
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

fn browser_connect_props() -> Value {
    props(&[
        ("port", "integer", "CDP HTTP port."),
        ("ws_url", "string", "Direct page WebSocket URL."),
        ("browser", "string", "chrome/edge/chromium/auto."),
        ("profile", "string", "Browser profile hint."),
        (
            "timeout_ms",
            "integer",
            "Connection wait timeout in milliseconds.",
        ),
    ])
}

fn browser_launch_props() -> Value {
    props(&[
        ("browser", "string", "chrome/edge/chromium/auto."),
        (
            "profile",
            "string",
            "Browser profile hint, for example incognito.",
        ),
        (
            "mode",
            "string",
            "headless/headless-incognito/normal/incognito. Defaults to headless.",
        ),
        (
            "headless",
            "boolean",
            "Run without opening a visible browser window. Defaults to true unless mode is normal or incognito.",
        ),
        (
            "port",
            "integer",
            "CDP HTTP port. Uses a free local port by default.",
        ),
        ("url", "string", "Optional initial URL."),
        (
            "user_data_dir",
            "string",
            "Optional browser user data directory.",
        ),
        (
            "timeout_ms",
            "integer",
            "Launch and connection timeout in milliseconds.",
        ),
    ])
}

fn browser_shutdown_props() -> Value {
    props(&[
        (
            "browser_id",
            "string",
            "Browser connection id returned by launch.",
        ),
        (
            "launch_id",
            "string",
            "Managed launch id returned by launch.",
        ),
        ("port", "integer", "CDP HTTP port for the launched browser."),
        (
            "timeout_ms",
            "integer",
            "Shutdown wait timeout in milliseconds.",
        ),
    ])
}

fn browser_session_props(extra: &[(&str, &str, &str)]) -> Value {
    let mut map = props(&[
        ("browser_id", "string", "Browser connection id."),
        ("port", "integer", "CDP HTTP port."),
        ("ws_url", "string", "Direct page WebSocket URL."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    for (name, ty, description) in extra {
        map.insert((*name).to_string(), typed_schema(ty, description));
    }
    Value::Object(map)
}

fn browser_page_props(extra: &[(&str, &str, &str)]) -> Value {
    let mut map = browser_session_props(&[("page_id", "string", "Browser page id.")])
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (name, ty, description) in extra {
        map.insert((*name).to_string(), typed_schema(ty, description));
    }
    Value::Object(map)
}

fn browser_open_props() -> Value {
    browser_page_props(&[
        ("url", "string", "URL to open."),
        ("new_page", "boolean", "Open in a new page."),
        (
            "wait_until",
            "string",
            "load/domcontentloaded/networkidle/none.",
        ),
        ("timeout_ms", "integer", "Timeout in milliseconds."),
    ])
}

fn browser_observe_props() -> Value {
    let mut map = browser_page_props(&[
        ("mode", "string", "snapshot/text/screenshot/hybrid."),
        ("max_elements", "integer", "Maximum elements."),
        (
            "max_text_chars",
            "integer",
            "Maximum visible text characters.",
        ),
        (
            "viewport_only",
            "boolean",
            "Only include viewport elements.",
        ),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    map.insert(
        "include".to_string(),
        json!({
            "type": "object",
            "description": "Optional include flags for elements, text, screenshot, frames, network, and console."
        }),
    );
    Value::Object(map)
}

fn browser_viewport_props() -> Value {
    browser_page_props(&[
        ("width", "integer", "Viewport width in CSS pixels."),
        ("height", "integer", "Viewport height in CSS pixels."),
        (
            "device_scale_factor",
            "number",
            "Device scale factor for the emulated viewport.",
        ),
        ("mobile", "boolean", "Whether to emulate a mobile viewport."),
        (
            "screen_width",
            "integer",
            "Optional screen width in CSS pixels.",
        ),
        (
            "screen_height",
            "integer",
            "Optional screen height in CSS pixels.",
        ),
        ("timeout_ms", "integer", "Timeout in milliseconds."),
    ])
}

fn browser_target_props() -> Value {
    props(&[
        ("ref", "string", "Element ref from observe/find."),
        ("selector", "string", "CSS selector."),
        ("dom_id", "string", "DOM id."),
        ("dom_class", "string", "DOM class."),
        ("role", "string", "Role filter."),
        ("name", "string", "Accessible name filter."),
        ("text", "string", "Visible text filter."),
        ("query", "string", "Text/name search."),
        ("x", "number", "Viewport X coordinate."),
        ("y", "number", "Viewport Y coordinate."),
    ])
}

fn browser_find_props() -> Value {
    let mut map = browser_page_props(&[
        ("visible_only", "boolean", "Only visible elements."),
        ("max_results", "integer", "Maximum results."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    if let Some(target) = browser_target_props().as_object() {
        map.extend(target.clone());
    }
    Value::Object(map)
}

fn browser_action_props(extra: &[(&str, &str, &str)]) -> Value {
    let mut map = browser_find_props()
        .as_object()
        .cloned()
        .unwrap_or_default();
    for (name, ty, description) in extra {
        map.insert((*name).to_string(), typed_schema(ty, description));
    }
    Value::Object(map)
}

fn browser_scroll_props() -> Value {
    let mut map = browser_action_props(&[("direction", "string", "up/down/left/right.")])
        .as_object()
        .cloned()
        .unwrap_or_default();
    map.insert(
        "amount".to_string(),
        json!({
            "anyOf": [
                { "type": "string", "enum": ["small", "medium", "large"] },
                { "type": "integer", "minimum": 1 }
            ],
            "description": "small/medium/large or a positive integer."
        }),
    );
    Value::Object(map)
}

fn browser_type_props() -> Value {
    let mut map = browser_action_props(&[
        ("into", "string", "Visible label, placeholder, or query."),
        ("focused", "boolean", "Type into active element."),
        ("text", "string", "Text to type."),
        ("clear", "boolean", "Clear first."),
        ("submit", "boolean", "Submit after typing."),
        ("delay_ms", "integer", "Typing delay hint."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    map.remove("max_results");
    Value::Object(map)
}

fn browser_wait_props() -> Value {
    browser_action_props(&[
        ("condition", "string", "Browser wait condition."),
        ("value", "string", "Condition value."),
        ("timeout_ms", "integer", "Timeout in milliseconds."),
        (
            "interval_ms",
            "integer",
            "Polling interval in milliseconds.",
        ),
    ])
}

fn browser_extract_props() -> Value {
    let mut map = browser_page_props(&[
        ("format", "string", "text/markdown/html/json."),
        ("instruction", "string", "Extraction instruction hint."),
        ("max_chars", "integer", "Maximum output characters."),
        ("selector", "string", "CSS selector target."),
        ("ref", "string", "Element ref target."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    map.insert(
        "target".to_string(),
        json!({
            "type": "object",
            "description": "Optional extraction target with ref, selector, or page=true."
        }),
    );
    Value::Object(map)
}

fn browser_history_props() -> Value {
    browser_page_props(&[("timeout_ms", "integer", "Timeout in milliseconds.")])
}

fn browser_network_props() -> Value {
    browser_page_props(&[
        (
            "since_ms",
            "integer",
            "Only include recent entries when available.",
        ),
        (
            "include_body",
            "boolean",
            "Include response body when a request id is provided.",
        ),
        (
            "request_id",
            "string",
            "CDP request id for response-body lookup.",
        ),
        ("url_contains", "string", "Filter by URL substring."),
        ("resource_type", "string", "Filter by resource type."),
        ("max_entries", "integer", "Maximum entries."),
        ("unsafe", "boolean", "Required for response body access."),
    ])
}

fn browser_console_props() -> Value {
    browser_page_props(&[
        ("level", "string", "log/info/warning/error/debug."),
        (
            "since_ms",
            "integer",
            "Only include recent entries when available.",
        ),
        ("max_entries", "integer", "Maximum entries."),
    ])
}

fn browser_storage_props() -> Value {
    browser_page_props(&[
        ("area", "string", "localStorage/sessionStorage."),
        ("origin", "string", "Origin hint."),
        ("action", "string", "list/get/set/remove/clear."),
        ("key", "string", "Storage key."),
        ("value", "string", "Storage value."),
        ("unsafe", "boolean", "Required for every storage action."),
    ])
}

fn browser_cookies_props() -> Value {
    browser_page_props(&[
        ("action", "string", "list/get/set/remove/clear."),
        ("name", "string", "Cookie name."),
        ("value", "string", "Cookie value."),
        ("url", "string", "Cookie URL."),
        ("domain", "string", "Cookie domain."),
        ("path", "string", "Cookie path."),
        ("expires", "number", "Cookie expiration timestamp."),
        ("http_only", "boolean", "Set HttpOnly."),
        ("secure", "boolean", "Set Secure."),
        ("same_site", "string", "Strict/Lax/None."),
        ("unsafe", "boolean", "Required for every cookie action."),
    ])
}

fn browser_downloads_props() -> Value {
    browser_page_props(&[
        ("action", "string", "deny/allow/allowAndName/default."),
        (
            "download_path",
            "string",
            "Directory for allowed downloads.",
        ),
        ("unsafe", "boolean", "Required to change download behavior."),
    ])
}

fn browser_upload_props() -> Value {
    let mut map = browser_action_props(&[
        ("file_paths", "array", "Absolute file paths to upload."),
        ("unsafe", "boolean", "Required to set file input paths."),
    ])
    .as_object()
    .cloned()
    .unwrap_or_default();
    map.remove("max_results");
    Value::Object(map)
}

fn browser_pdf_props() -> Value {
    browser_page_props(&[
        (
            "landscape",
            "boolean",
            "Print PDF in landscape orientation.",
        ),
        (
            "print_background",
            "boolean",
            "Include background graphics.",
        ),
        ("scale", "number", "PDF scale."),
        ("paper_width", "number", "Paper width in inches."),
        ("paper_height", "number", "Paper height in inches."),
    ])
}

fn cdp_send_props() -> Value {
    browser_page_props(&[
        (
            "domain",
            "string",
            "CDP domain; alternatively include it in method.",
        ),
        (
            "method",
            "string",
            "CDP method, for example Page.captureScreenshot.",
        ),
        ("params", "object", "CDP params."),
        ("timeout_ms", "integer", "Timeout in milliseconds."),
        ("unsafe", "boolean", "Required for raw CDP execution."),
    ])
}

fn cdp_subscribe_props() -> Value {
    browser_page_props(&[
        ("domain", "string", "CDP domain to enable and listen to."),
        ("event", "string", "Optional event name filter."),
        (
            "timeout_ms",
            "integer",
            "Collection window in milliseconds.",
        ),
        ("max_events", "integer", "Maximum events."),
        ("unsafe", "boolean", "Required for raw CDP subscription."),
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
        let name = canonical_tool_name(name);
        tools
            .iter()
            .find(|tool| tool.name == name)
            .expect("tool exists")
    }

    #[test]
    fn maps_legacy_tool_names_to_public_names() {
        assert_eq!(canonical_tool_name("sootie_browser_open"), "browser_open");
        assert_eq!(canonical_tool_name("browser_open"), "browser_open");
        assert_eq!(legacy_tool_name("browser_open"), "sootie_browser_open");
        assert_eq!(
            legacy_tool_name("sootie_browser_open"),
            "sootie_browser_open"
        );
    }

    #[test]
    fn exposes_sootie_tool_surface() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 58);
        assert!(tools.iter().any(|tool| tool.name == "context"));
        assert!(tools.iter().any(|tool| tool.name == "browser_observe"));
        for name in [
            "browser_launch",
            "browser_viewport",
            "browser_network",
            "browser_console",
            "browser_storage",
            "browser_cookies",
            "browser_downloads",
            "browser_upload",
            "browser_pdf",
            "browser_shutdown",
            "cdp_send",
            "cdp_subscribe",
        ] {
            assert!(tools.iter().any(|tool| tool.name == name), "{name}");
        }
        assert!(tools.iter().any(|tool| tool.name == "learn_status"));
        assert!(!tools.iter().any(|tool| tool.name.starts_with("sootie_")));
    }

    #[test]
    fn tool_contract_is_exact() {
        let tools = tool_definitions();
        assert_eq!(TOOL_NAMES.len(), 58);
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
        macro_rules! browser_session {
            ($($name:literal),* $(,)?) => {
                &["browser_id", "port", "ws_url", $($name),*][..]
            };
        }
        macro_rules! browser_page {
            ($($name:literal),* $(,)?) => {
                &["browser_id", "port", "ws_url", "page_id", $($name),*][..]
            };
        }
        macro_rules! browser_target {
            ($($name:literal),* $(,)?) => {
                &[
                    "browser_id", "port", "ws_url", "page_id",
                    "visible_only", "max_results",
                    "ref", "selector", "dom_id", "dom_class", "role", "name", "text", "query", "x", "y",
                    $($name),*
                ][..]
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
                    "max_results",
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
            (
                "sootie_screenshot",
                app_scope!["window", "full_resolution"],
                &[],
            ),
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
            (
                "sootie_parse_screen",
                app_scope!["window", "full_resolution"],
                &[],
            ),
            (
                "sootie_ground",
                app_scope!["description", "crop_box"],
                &["description"],
            ),
            ("sootie_annotate", app_scope!["roles", "max_labels"], &[]),
            (
                "sootie_browser_launch",
                &[
                    "browser",
                    "profile",
                    "mode",
                    "headless",
                    "port",
                    "url",
                    "user_data_dir",
                    "timeout_ms",
                ],
                &[],
            ),
            (
                "sootie_browser_connect",
                &["port", "ws_url", "browser", "profile", "timeout_ms"],
                &[],
            ),
            (
                "sootie_browser_pages",
                browser_session!["include_inactive"],
                &[],
            ),
            (
                "sootie_browser_select_page",
                browser_session!["page_id"],
                &["page_id"],
            ),
            (
                "sootie_browser_open",
                browser_page!["url", "new_page", "wait_until", "timeout_ms"],
                &["url"],
            ),
            (
                "sootie_browser_observe",
                browser_page![
                    "mode",
                    "max_elements",
                    "max_text_chars",
                    "viewport_only",
                    "include"
                ],
                &[],
            ),
            (
                "sootie_browser_viewport",
                browser_page![
                    "width",
                    "height",
                    "device_scale_factor",
                    "mobile",
                    "screen_width",
                    "screen_height",
                    "timeout_ms"
                ],
                &[],
            ),
            ("sootie_browser_find", browser_target![], &[]),
            (
                "sootie_browser_click",
                browser_target!["button", "count", "wait_after"],
                &[],
            ),
            (
                "sootie_browser_type",
                &[
                    "browser_id",
                    "port",
                    "ws_url",
                    "page_id",
                    "visible_only",
                    "ref",
                    "selector",
                    "dom_id",
                    "dom_class",
                    "role",
                    "name",
                    "text",
                    "query",
                    "x",
                    "y",
                    "into",
                    "focused",
                    "clear",
                    "submit",
                    "delay_ms",
                ],
                &["text"],
            ),
            (
                "sootie_browser_press",
                browser_session!["page_id", "key", "modifiers"],
                &["key"],
            ),
            (
                "sootie_browser_scroll",
                browser_target!["direction", "amount"],
                &[],
            ),
            (
                "sootie_browser_wait",
                browser_target!["condition", "value", "timeout_ms", "interval_ms"],
                &["condition"],
            ),
            (
                "sootie_browser_extract",
                browser_page![
                    "format",
                    "instruction",
                    "max_chars",
                    "selector",
                    "ref",
                    "target"
                ],
                &[],
            ),
            (
                "sootie_browser_screenshot",
                browser_session!["page_id", "full_page", "format"],
                &[],
            ),
            ("sootie_browser_back", browser_page!["timeout_ms"], &[]),
            ("sootie_browser_forward", browser_page!["timeout_ms"], &[]),
            ("sootie_browser_reload", browser_page!["timeout_ms"], &[]),
            (
                "sootie_browser_close_page",
                browser_session!["page_id"],
                &[],
            ),
            (
                "sootie_browser_shutdown",
                &["browser_id", "launch_id", "port", "timeout_ms"],
                &[],
            ),
            (
                "sootie_browser_network",
                browser_page![
                    "since_ms",
                    "include_body",
                    "request_id",
                    "url_contains",
                    "resource_type",
                    "max_entries",
                    "unsafe"
                ],
                &[],
            ),
            (
                "sootie_browser_console",
                browser_page!["level", "since_ms", "max_entries"],
                &[],
            ),
            (
                "sootie_browser_storage",
                browser_page!["area", "origin", "action", "key", "value", "unsafe"],
                &["area", "action"],
            ),
            (
                "sootie_browser_cookies",
                browser_page![
                    "action",
                    "name",
                    "value",
                    "url",
                    "domain",
                    "path",
                    "expires",
                    "http_only",
                    "secure",
                    "same_site",
                    "unsafe"
                ],
                &["action"],
            ),
            (
                "sootie_browser_downloads",
                browser_page!["action", "download_path", "unsafe"],
                &["action"],
            ),
            (
                "sootie_browser_upload",
                &[
                    "browser_id",
                    "port",
                    "ws_url",
                    "page_id",
                    "visible_only",
                    "ref",
                    "selector",
                    "dom_id",
                    "dom_class",
                    "role",
                    "name",
                    "text",
                    "query",
                    "x",
                    "y",
                    "file_paths",
                    "unsafe",
                ],
                &["file_paths"],
            ),
            (
                "sootie_browser_pdf",
                browser_page![
                    "landscape",
                    "print_background",
                    "scale",
                    "paper_width",
                    "paper_height"
                ],
                &[],
            ),
            (
                "sootie_cdp_send",
                browser_page!["domain", "method", "params", "timeout_ms", "unsafe"],
                &["method"],
            ),
            (
                "sootie_cdp_subscribe",
                browser_page!["domain", "event", "timeout_ms", "max_events", "unsafe"],
                &["domain"],
            ),
            ("sootie_learn_start", &["task_description"], &[]),
            ("sootie_learn_stop", &[], &[]),
            ("sootie_learn_status", &[], &[]),
        ];

        assert_eq!(
            expected
                .iter()
                .map(|(name, _, _)| canonical_tool_name(name))
                .collect::<Vec<_>>(),
            TOOL_NAMES
        );
        for (name, properties, required) in expected {
            let tool = tools
                .iter()
                .find(|tool| tool.name == canonical_tool_name(name))
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
            ("sootie_browser_launch", &[]),
            ("sootie_browser_connect", &[]),
            ("sootie_browser_pages", &[]),
            ("sootie_browser_select_page", &["page_id"]),
            ("sootie_browser_open", &["url"]),
            ("sootie_browser_observe", &[]),
            ("sootie_browser_viewport", &[]),
            ("sootie_browser_find", &[]),
            ("sootie_browser_click", &[]),
            ("sootie_browser_type", &["text"]),
            ("sootie_browser_press", &["key"]),
            ("sootie_browser_scroll", &[]),
            ("sootie_browser_wait", &["condition"]),
            ("sootie_browser_extract", &[]),
            ("sootie_browser_screenshot", &[]),
            ("sootie_browser_back", &[]),
            ("sootie_browser_forward", &[]),
            ("sootie_browser_reload", &[]),
            ("sootie_browser_close_page", &[]),
            ("sootie_browser_shutdown", &[]),
            ("sootie_browser_network", &[]),
            ("sootie_browser_console", &[]),
            ("sootie_browser_storage", &["area", "action"]),
            ("sootie_browser_cookies", &["action"]),
            ("sootie_browser_downloads", &["action"]),
            ("sootie_browser_upload", &["file_paths"]),
            ("sootie_browser_pdf", &[]),
            ("sootie_cdp_send", &["method"]),
            ("sootie_cdp_subscribe", &["domain"]),
            ("sootie_learn_start", &[]),
            ("sootie_learn_stop", &[]),
            ("sootie_learn_status", &[]),
        ];

        assert_eq!(
            expected
                .iter()
                .map(|(name, _)| canonical_tool_name(name))
                .collect::<Vec<_>>(),
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
                    ("max_results", "integer"),
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
                &[
                    ("app", "string"),
                    ("window", "string"),
                    ("full_resolution", "boolean"),
                ],
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
                &[
                    ("app", "string"),
                    ("window", "string"),
                    ("full_resolution", "boolean"),
                ],
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
            .find(|tool| tool.name == "ground")
            .expect("ground tool exists");
        assert_eq!(
            ground.input_schema["properties"]["crop_box"]["items"]["type"],
            "number"
        );
    }

    #[test]
    fn browser_scroll_amount_accepts_named_or_integer_schema() {
        let tools = tool_definitions();
        let scroll = tool_by_name(&tools, "sootie_browser_scroll");
        let amount = &scroll.input_schema["properties"]["amount"];
        assert_eq!(amount["anyOf"][0]["type"], "string");
        assert_eq!(amount["anyOf"][0]["enum"][0], "small");
        assert_eq!(amount["anyOf"][1]["type"], "integer");
        assert_eq!(amount["anyOf"][1]["minimum"], 1);
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
            if tool.name.starts_with("browser_") || tool.name.starts_with("cdp_") {
                continue;
            }
            for name in forbidden {
                assert!(
                    tool.input_schema["properties"][name].is_null(),
                    "{} unexpectedly advertises {name}",
                    tool.name
                );
            }
            if tool.name != "ground" {
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

        let browser_observe = tool_by_name(&tools, "sootie_browser_observe");
        assert!(browser_observe.annotations.read_only_hint);
        assert!(!browser_observe.annotations.destructive_hint);
        assert!(!browser_observe.annotations.open_world_hint);

        let browser_click = tool_by_name(&tools, "sootie_browser_click");
        assert!(!browser_click.annotations.read_only_hint);
        assert!(!browser_click.annotations.destructive_hint);
        assert!(browser_click.annotations.open_world_hint);

        let browser_close = tool_by_name(&tools, "sootie_browser_close_page");
        assert!(!browser_close.annotations.read_only_hint);
        assert!(browser_close.annotations.destructive_hint);
    }
}
