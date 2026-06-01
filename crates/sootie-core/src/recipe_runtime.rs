use serde_json::{json, Value};

use crate::types::{Bounds, SootieError, SootieResult};

#[derive(Debug, Clone, Copy)]
struct RecipePointKeys {
    x: &'static str,
    y: &'static str,
    space: &'static str,
}

pub(crate) fn recipe_primary_dispatch_args(args: &Value) -> Value {
    let Value::Object(mut map) = args.clone() else {
        return args.clone();
    };
    remove_recipe_fallback_keys(&mut map);
    Value::Object(map)
}

pub(crate) fn recipe_coordinate_fallback_args(args: &Value) -> Option<Value> {
    let Value::Object(mut map) = args.clone() else {
        return None;
    };
    let mut has_fallback = false;
    if promote_recipe_fallback_point(
        &mut map,
        "__fallback_x",
        "__fallback_y",
        "__fallback_coordinate_space",
        "x",
        "y",
        "coordinate_space",
    ) {
        strip_recipe_pointer_selector_fields(&mut map);
        has_fallback = true;
    }
    if promote_recipe_fallback_point(
        &mut map,
        "__fallback_from_x",
        "__fallback_from_y",
        "__fallback_from_coordinate_space",
        "from_x",
        "from_y",
        "from_coordinate_space",
    ) {
        strip_recipe_pointer_selector_fields(&mut map);
        map.remove("from_target");
        has_fallback = true;
    }
    if promote_recipe_fallback_point(
        &mut map,
        "__fallback_to_x",
        "__fallback_to_y",
        "__fallback_to_coordinate_space",
        "to_x",
        "to_y",
        "to_coordinate_space",
    ) {
        map.remove("to_target");
        has_fallback = true;
    }
    remove_recipe_fallback_keys(&mut map);
    has_fallback.then_some(Value::Object(map))
}

fn promote_recipe_fallback_point(
    map: &mut serde_json::Map<String, Value>,
    fallback_x_key: &str,
    fallback_y_key: &str,
    fallback_space_key: &str,
    x_key: &str,
    y_key: &str,
    space_key: &str,
) -> bool {
    let Some(x) = map.get(fallback_x_key).cloned() else {
        return false;
    };
    let Some(y) = map.get(fallback_y_key).cloned() else {
        return false;
    };
    map.insert(x_key.to_string(), x);
    map.insert(y_key.to_string(), y);
    if let Some(space) = map.get(fallback_space_key).cloned() {
        map.insert(space_key.to_string(), space);
    }
    let fallback_resolved_from_key = format!("{fallback_space_key}_resolved_from");
    if let Some(resolved_from) = map.get(&fallback_resolved_from_key).cloned() {
        map.insert(format!("{space_key}_resolved_from"), resolved_from);
    }
    true
}

fn strip_recipe_pointer_selector_fields(map: &mut serde_json::Map<String, Value>) {
    for key in [
        "query",
        "role",
        "dom_id",
        "dom_class",
        "identifier",
        "target",
    ] {
        map.remove(key);
    }
}

fn remove_recipe_fallback_keys(map: &mut serde_json::Map<String, Value>) {
    map.retain(|key, _| !key.starts_with("__fallback_"));
}

pub(crate) fn recipe_coordinate_fallback_reason(error: &str) -> Option<&str> {
    let lower = error.to_lowercase();
    if lower.contains("not found")
        || lower.contains("target")
        || lower.contains("did not resolve")
        || lower.contains("no accessible element")
    {
        Some(error)
    } else {
        None
    }
}

pub(crate) fn recipe_error_coordinate_fallback_reason(error: &SootieError) -> Option<String> {
    match error {
        SootieError::NotFound(_) => Some(error.to_string()),
        SootieError::InvalidArguments(message)
            if recipe_coordinate_fallback_reason(message).is_some() =>
        {
            Some(error.to_string())
        }
        _ => None,
    }
}

pub(crate) fn resolve_recipe_coordinate_spaces<F>(
    args: Value,
    app: Option<&str>,
    window: Option<&str>,
    mut current_window_bounds: F,
) -> SootieResult<Value>
where
    F: FnMut(&str, Option<&str>) -> SootieResult<Option<Bounds>>,
{
    let Value::Object(mut map) = args else {
        return Ok(args);
    };
    let mut frame = None;
    for keys in [
        RecipePointKeys {
            x: "x",
            y: "y",
            space: "coordinate_space",
        },
        RecipePointKeys {
            x: "from_x",
            y: "from_y",
            space: "from_coordinate_space",
        },
        RecipePointKeys {
            x: "to_x",
            y: "to_y",
            space: "to_coordinate_space",
        },
        RecipePointKeys {
            x: "__fallback_x",
            y: "__fallback_y",
            space: "__fallback_coordinate_space",
        },
        RecipePointKeys {
            x: "__fallback_from_x",
            y: "__fallback_from_y",
            space: "__fallback_from_coordinate_space",
        },
        RecipePointKeys {
            x: "__fallback_to_x",
            y: "__fallback_to_y",
            space: "__fallback_to_coordinate_space",
        },
    ] {
        resolve_recipe_point(
            &mut map,
            keys,
            app,
            window,
            &mut frame,
            &mut current_window_bounds,
        )?;
    }
    Ok(Value::Object(map))
}

fn resolve_recipe_point<F>(
    map: &mut serde_json::Map<String, Value>,
    keys: RecipePointKeys,
    app: Option<&str>,
    window: Option<&str>,
    frame: &mut Option<Bounds>,
    current_window_bounds: &mut F,
) -> SootieResult<()>
where
    F: FnMut(&str, Option<&str>) -> SootieResult<Option<Bounds>>,
{
    let Some(space) = map
        .get(keys.space)
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(());
    };
    let Some(x) = map.get(keys.x).and_then(f64_value) else {
        return Ok(());
    };
    let Some(y) = map.get(keys.y).and_then(f64_value) else {
        return Ok(());
    };
    if !matches!(
        space.as_str(),
        "window" | "window_relative" | "window_normalized" | "normalized"
    ) {
        return Ok(());
    }
    let app = app.ok_or_else(|| {
        SootieError::InvalidArguments(format!("{}={space} requires app", keys.space))
    })?;
    let bounds = match frame {
        Some(bounds) => bounds.clone(),
        None => {
            let bounds = current_window_bounds(app, window)?.ok_or_else(|| {
                SootieError::NotFound(format!(
                    "no window bounds available for recipe coordinate remapping in {app}"
                ))
            })?;
            *frame = Some(bounds.clone());
            bounds
        }
    };
    let (resolved_x, resolved_y) = match space.as_str() {
        "window" | "window_relative" => (bounds.x + x, bounds.y + y),
        "window_normalized" | "normalized" => {
            (bounds.x + x * bounds.width, bounds.y + y * bounds.height)
        }
        _ => unreachable!(),
    };
    map.insert(keys.x.to_string(), json!(resolved_x));
    map.insert(keys.y.to_string(), json!(resolved_y));
    map.insert(format!("{}_resolved_from", keys.space), json!(space));
    map.insert(keys.space.to_string(), json!("screen"));
    Ok(())
}

fn f64_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bounds() -> Bounds {
        Bounds {
            x: 100.0,
            y: 200.0,
            width: 400.0,
            height: 300.0,
        }
    }

    #[test]
    fn remaps_window_coordinate_spaces_once_per_recipe_step() {
        let args = json!({
            "app": "Safari",
            "window": "Excalidraw",
            "x": 0.25,
            "y": 0.5,
            "coordinate_space": "window_normalized",
            "__fallback_x": 10,
            "__fallback_y": 20,
            "__fallback_coordinate_space": "window"
        });
        let mut calls = Vec::<(String, Option<String>)>::new();

        let resolved = resolve_recipe_coordinate_spaces(
            args,
            Some("Safari"),
            Some("Excalidraw"),
            |app, window| {
                calls.push((app.to_string(), window.map(str::to_string)));
                Ok(Some(test_bounds()))
            },
        )
        .expect("coordinate remapping should succeed");

        assert_eq!(resolved["x"], json!(200.0));
        assert_eq!(resolved["y"], json!(350.0));
        assert_eq!(resolved["coordinate_space"], json!("screen"));
        assert_eq!(
            resolved["coordinate_space_resolved_from"],
            json!("window_normalized")
        );
        assert_eq!(resolved["__fallback_x"], json!(110.0));
        assert_eq!(resolved["__fallback_y"], json!(220.0));
        assert_eq!(resolved["__fallback_coordinate_space"], json!("screen"));
        assert_eq!(
            resolved["__fallback_coordinate_space_resolved_from"],
            json!("window")
        );
        assert_eq!(
            calls,
            vec![("Safari".to_string(), Some("Excalidraw".to_string()))]
        );
    }

    #[test]
    fn window_coordinate_space_requires_app_scope() {
        let error = resolve_recipe_coordinate_spaces(
            json!({
                "x": 10,
                "y": 20,
                "coordinate_space": "window"
            }),
            None,
            None,
            |_app, _window| Ok(Some(test_bounds())),
        )
        .expect_err("window-relative coordinates require app scope");

        assert!(error
            .to_string()
            .contains("coordinate_space=window requires app"));
    }

    #[test]
    fn missing_current_window_bounds_is_target_resolution_failure() {
        let error = resolve_recipe_coordinate_spaces(
            json!({
                "x": 10,
                "y": 20,
                "coordinate_space": "window"
            }),
            Some("Safari"),
            Some("Missing"),
            |_app, _window| Ok(None),
        )
        .expect_err("missing window bounds should fail before coordinate dispatch");

        assert!(matches!(error, SootieError::NotFound(_)));
        assert!(error
            .to_string()
            .contains("no window bounds available for recipe coordinate remapping"));
    }
}
