use serde_json::{json, Value};

pub(crate) fn learned_recipe_from_actions(
    task_description: &str,
    actions: &[Value],
) -> Option<Value> {
    let mut steps = Vec::new();
    for action in actions {
        if let Some(step) = learned_clipboard_step(steps.len() + 1, action) {
            steps.push(step);
        }
        if let Some(step) = learned_recipe_step(steps.len() + 1, action) {
            steps.push(step);
        }
    }
    if steps.is_empty() {
        return None;
    }
    let mut recipe = serde_json::Map::new();
    recipe.insert("schema_version".to_string(), json!(4));
    recipe.insert(
        "name".to_string(),
        json!(learned_recipe_name(task_description)),
    );
    if !task_description.trim().is_empty() {
        recipe.insert("description".to_string(), json!(task_description));
    }
    let apps = learned_apps(actions);
    if apps.len() == 1 {
        recipe.insert("app".to_string(), json!(apps[0]));
    }
    recipe.insert("steps".to_string(), Value::Array(steps));
    Some(Value::Object(recipe))
}

fn learned_recipe_name(task_description: &str) -> String {
    let name = stable_identifier_text(task_description);
    if name.is_empty() {
        "learned-recipe".to_string()
    } else {
        name
    }
}

fn learned_recipe_step(id: usize, action: &Value) -> Option<Value> {
    let action_type = action.get("action_type")?.as_str()?;
    let mut step = serde_json::Map::new();
    step.insert("id".to_string(), json!(id));
    match action_type {
        "click" => {
            step.insert("action".to_string(), json!("click"));
            if let Some(target) = learned_recipe_target(action, "", "target", true) {
                step.insert("target".to_string(), target);
            }
            insert_non_default_string(&mut step, action, "button", "left");
            insert_non_default_u64(&mut step, action, "count", 1);
        }
        "hover" => {
            step.insert("action".to_string(), json!("hover"));
            if let Some(target) = learned_recipe_target(action, "", "target", true) {
                step.insert("target".to_string(), target);
            }
        }
        "longPress" => {
            step.insert("action".to_string(), json!("long_press"));
            if let Some(target) = learned_recipe_target(action, "", "target", true) {
                step.insert("target".to_string(), target);
            }
            if let Some(duration) = action.get("duration").and_then(Value::as_f64) {
                step.insert("params".to_string(), json!({ "duration": duration }));
            }
            insert_non_default_string(&mut step, action, "button", "left");
        }
        "drag" => {
            step.insert("action".to_string(), json!("drag"));
            if let Some(target) = learned_recipe_target(action, "from_", "from_target", true) {
                step.insert("target".to_string(), target);
            }
            if let Some(target) = learned_recipe_target(action, "to_", "to_target", true) {
                step.insert("to_target".to_string(), target);
            }
            let mut params = serde_json::Map::new();
            if let Some(duration) = action.get("duration").and_then(Value::as_f64) {
                params.insert("duration".to_string(), json!(duration));
            }
            if let Some(hold_duration) = action.get("hold_duration").and_then(Value::as_f64) {
                params.insert("hold_duration".to_string(), json!(hold_duration));
            }
            if !params.is_empty() {
                step.insert("params".to_string(), Value::Object(params));
            }
        }
        "typeText" => {
            step.insert("action".to_string(), json!("type"));
            step.insert(
                "text".to_string(),
                action.get("text").cloned().unwrap_or(Value::Null),
            );
            if let Some(target) = learned_recipe_target(action, "", "target", false) {
                step.insert("target".to_string(), target);
            }
        }
        "keyPress" => {
            step.insert("action".to_string(), json!("press"));
            step.insert(
                "key".to_string(),
                action.get("key_name").cloned().unwrap_or(Value::Null),
            );
            if let Some(modifiers) = non_empty_array(action.get("modifiers")) {
                step.insert(
                    "params".to_string(),
                    json!({ "modifiers": modifiers.clone() }),
                );
            }
        }
        "hotkey" => {
            step.insert("action".to_string(), json!("hotkey"));
            let keys = action
                .get("modifiers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .chain(action.get("key_name").cloned())
                .collect::<Vec<_>>();
            step.insert("keys".to_string(), Value::Array(keys));
        }
        "scroll" => {
            step.insert("action".to_string(), json!("scroll"));
            let (direction, amount) = scroll_recipe_direction_amount(action);
            step.insert("direction".to_string(), json!(direction));
            step.insert("amount".to_string(), json!(amount));
        }
        "appSwitch" => {
            step.insert("action".to_string(), json!("focus"));
            let mut params = serde_json::Map::new();
            if let Some(app) = action.get("to_app").and_then(Value::as_str) {
                params.insert("app".to_string(), json!(app));
            }
            if !params.is_empty() {
                step.insert("params".to_string(), Value::Object(params));
            }
        }
        "window" => {
            step.insert("action".to_string(), json!("window"));
            let mut params = serde_json::Map::new();
            for key in ["command", "window", "x", "y", "width", "height"] {
                if let Some(value) = action.get(key).filter(|value| !value.is_null()) {
                    let recipe_key = if key == "command" { "action" } else { key };
                    params.insert(recipe_key.to_string(), value.clone());
                }
            }
            if !params.is_empty() {
                step.insert("params".to_string(), Value::Object(params));
            }
        }
        _ => return None,
    }
    Some(Value::Object(step))
}

fn learned_clipboard_step(id: usize, action: &Value) -> Option<Value> {
    if !learned_action_is_paste_hotkey(action) {
        return None;
    }
    let text = action
        .get("clipboard_text")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())?;
    Some(json!({
        "id": id,
        "action": "set_clipboard",
        "text": text,
    }))
}

fn learned_action_is_paste_hotkey(action: &Value) -> bool {
    if action.get("action_type").and_then(Value::as_str) != Some("hotkey") {
        return false;
    }
    let key = action
        .get("key_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !key.eq_ignore_ascii_case("v") {
        return false;
    }
    action
        .get("modifiers")
        .and_then(Value::as_array)
        .is_some_and(|modifiers| {
            modifiers.iter().any(|modifier| {
                modifier.as_str().is_some_and(|value| {
                    matches!(
                        value.to_ascii_lowercase().as_str(),
                        "cmd" | "meta" | "command" | "ctrl" | "control"
                    )
                })
            })
        })
}

fn learned_recipe_target(
    action: &Value,
    coordinate_prefix: &str,
    target_key: &str,
    include_coordinates: bool,
) -> Option<Value> {
    let mut target = serde_json::Map::new();
    copy_non_empty_string(&mut target, action, "app", "app");
    copy_non_empty_string(&mut target, action, "window", "window");
    if let Some(raw_target) = action.get(target_key) {
        copy_target_selector_string(&mut target, raw_target, "query", "name");
        copy_target_selector_string(&mut target, raw_target, "name", "name");
        copy_target_selector_string(&mut target, raw_target, "text", "text");
        copy_target_selector_string(&mut target, raw_target, "role", "role");
        copy_target_selector_string(&mut target, raw_target, "dom_id", "dom_id");
        copy_target_selector_string(&mut target, raw_target, "id", "dom_id");
        copy_target_selector_string(&mut target, raw_target, "dom_class", "dom_class");
        copy_target_selector_string(&mut target, raw_target, "identifier", "identifier");
    }
    if let Some(query) = action.get("query").and_then(Value::as_str) {
        if !query.trim().is_empty() && !target.contains_key("name") {
            target.insert("name".to_string(), json!(query));
        }
    }
    if include_coordinates {
        if let Some(point) = action.get(format!("{coordinate_prefix}window_normalized_coordinate"))
        {
            target.insert("window_normalized_coordinate".to_string(), point.clone());
        } else if let Some(point) = action.get(format!("{coordinate_prefix}window_coordinate")) {
            target.insert("window_coordinate".to_string(), point.clone());
        } else if let Some(point) = action.get(format!("{coordinate_prefix}screen_coordinate")) {
            target.insert("coordinate".to_string(), point.clone());
        }
    }
    (!target.is_empty()).then_some(Value::Object(target))
}

fn copy_non_empty_string(
    target: &mut serde_json::Map<String, Value>,
    source: &Value,
    source_key: &str,
    target_key: &str,
) {
    if let Some(value) = source.get(source_key).and_then(Value::as_str) {
        if !value.trim().is_empty() {
            target.insert(target_key.to_string(), json!(value));
        }
    }
}

fn copy_target_selector_string(
    target: &mut serde_json::Map<String, Value>,
    source: &Value,
    source_key: &str,
    target_key: &str,
) {
    let value = source
        .get("selector")
        .and_then(|selector| selector.get(source_key))
        .or_else(|| source.get(source_key))
        .and_then(Value::as_str);
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        target.insert(target_key.to_string(), json!(value));
    }
}

fn insert_non_default_string(
    target: &mut serde_json::Map<String, Value>,
    source: &Value,
    key: &str,
    default: &str,
) {
    if let Some(value) = source.get(key).and_then(Value::as_str) {
        if !value.eq_ignore_ascii_case(default) {
            target.insert(key.to_string(), json!(value));
        }
    }
}

fn insert_non_default_u64(
    target: &mut serde_json::Map<String, Value>,
    source: &Value,
    key: &str,
    default: u64,
) {
    if let Some(value) = source.get(key).and_then(Value::as_u64) {
        if value != default {
            target.insert(key.to_string(), json!(value));
        }
    }
}

fn non_empty_array(value: Option<&Value>) -> Option<&Vec<Value>> {
    value
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
}

fn scroll_recipe_direction_amount(action: &Value) -> (&'static str, i64) {
    let delta_x = action.get("delta_x").and_then(Value::as_i64).unwrap_or(0);
    let delta_y = action.get("delta_y").and_then(Value::as_i64).unwrap_or(0);
    if delta_x.abs() > delta_y.abs() {
        if delta_x < 0 {
            ("left", delta_x.abs())
        } else {
            ("right", delta_x.abs())
        }
    } else if delta_y > 0 {
        ("up", delta_y.abs())
    } else {
        ("down", delta_y.abs().max(1))
    }
}

fn learned_apps(actions: &[Value]) -> Vec<String> {
    let mut apps = Vec::<String>::new();
    for action in actions {
        for key in ["app", "to_app"] {
            if let Some(app) = action.get(key).and_then(Value::as_str) {
                if !app.is_empty() && !apps.iter().any(|existing| existing == app) {
                    apps.push(app.to_string());
                }
            }
        }
    }
    apps
}

fn stable_identifier_text(value: &str) -> String {
    let mut identifier = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while identifier.contains("--") {
        identifier = identifier.replace("--", "-");
    }
    identifier.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learned_paste_hotkey_emits_clipboard_asset_before_paste() {
        let recipe = learned_recipe_from_actions(
            "paste svg",
            &[json!({
                "action_type": "hotkey",
                "key_name": "v",
                "modifiers": ["cmd"],
                "clipboard_text": "<svg/>"
            })],
        )
        .expect("learned recipe");

        assert_eq!(recipe["steps"][0]["action"], "set_clipboard");
        assert_eq!(recipe["steps"][0]["text"], "<svg/>");
        assert_eq!(recipe["steps"][1]["action"], "hotkey");
        assert_eq!(recipe["steps"][1]["keys"], json!(["cmd", "v"]));
    }
}
