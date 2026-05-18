use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{json, Value};

use crate::types::{Point, SootieError, SootieResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recipe {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub app: Option<String>,
    #[serde(default, deserialize_with = "deserialize_recipe_param_map")]
    pub params: BTreeMap<String, RecipeParam>,
    #[serde(default)]
    pub preconditions: Option<RecipePreconditions>,
    #[serde(default)]
    pub steps: Vec<RecipeStep>,
    #[serde(default, rename = "on_failure")]
    pub on_failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecipeParam {
    #[serde(default, rename = "type")]
    pub param_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecipePreconditions {
    #[serde(default, rename = "app_running")]
    pub app_running: Option<String>,
    #[serde(default, rename = "url_contains")]
    pub url_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeStep {
    #[serde(default)]
    pub id: Option<u32>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub app: Option<Value>,
    #[serde(default)]
    pub args: Value,
    #[serde(default, deserialize_with = "deserialize_value_map")]
    pub params: BTreeMap<String, Value>,
    #[serde(default)]
    pub target: Option<RecipeTarget>,
    #[serde(default, rename = "to_target")]
    pub to_target: Option<RecipeTarget>,
    #[serde(default)]
    pub text: Option<Value>,
    #[serde(default)]
    pub key: Option<Value>,
    #[serde(default)]
    pub keys: Option<Value>,
    #[serde(default)]
    pub direction: Option<Value>,
    #[serde(default)]
    pub amount: Option<Value>,
    #[serde(default)]
    pub button: Option<Value>,
    #[serde(default)]
    pub count: Option<Value>,
    #[serde(default)]
    pub timeout: Option<Value>,
    #[serde(default, rename = "clear_first")]
    pub clear_first: Option<Value>,
    #[serde(default, rename = "wait_after")]
    pub wait_after: Option<RecipeWaitCondition>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default, rename = "on_failure")]
    pub on_failure: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecipeTarget {
    #[serde(default)]
    pub app: Option<Value>,
    #[serde(default)]
    pub coordinate: Option<Point>,
    #[serde(default, rename = "window_coordinate")]
    pub window_coordinate: Option<Point>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub identifier: Option<String>,
    #[serde(default, rename = "dom_id")]
    pub dom_id: Option<String>,
    #[serde(default, rename = "dom_class")]
    pub dom_class: Option<String>,
    #[serde(default)]
    pub criteria: Vec<RecipeCriterion>,
    #[serde(default, rename = "computedNameContains")]
    pub computed_name_contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeCriterion {
    pub attribute: String,
    pub value: String,
    #[serde(default, rename = "matchType")]
    pub match_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeWaitCondition {
    pub condition: String,
    #[serde(default)]
    pub target: Option<RecipeTarget>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub timeout: Option<f64>,
    #[serde(default)]
    pub timeout_ms: Option<f64>,
    #[serde(default)]
    pub interval: Option<f64>,
    #[serde(default)]
    pub interval_ms: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct RecipeStore {
    root: PathBuf,
}

impl Default for RecipeStore {
    fn default() -> Self {
        let root = dirs_next::data_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("sootie")
            .join("recipes");
        Self { root }
    }
}

impl RecipeStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn list(&self) -> SootieResult<Vec<Recipe>> {
        if !self.root.exists() {
            return Ok(vec![]);
        }
        let mut recipes: Vec<Recipe> = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let text = fs::read_to_string(path)?;
            recipes.push(serde_json::from_str(&text)?);
        }
        recipes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(recipes)
    }

    pub fn get(&self, name: &str) -> SootieResult<Recipe> {
        let path = self.path_for(name)?;
        if !path.exists() {
            return Err(SootieError::NotFound(format!("recipe '{name}'")));
        }
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    pub fn save(&self, recipe: &Recipe) -> SootieResult<PathBuf> {
        validate_recipe(recipe)?;
        fs::create_dir_all(&self.root)?;
        let path = self.path_for(&recipe.name)?;
        fs::write(&path, serde_json::to_vec_pretty(recipe)?)?;
        Ok(path)
    }

    pub fn delete(&self, name: &str) -> SootieResult<bool> {
        let path = self.path_for(name)?;
        if path.exists() {
            fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn path_for(&self, name: &str) -> SootieResult<PathBuf> {
        if name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err(SootieError::InvalidArguments(
                "recipe name must not contain path separators".to_string(),
            ));
        }
        Ok(self.root.join(format!("{name}.json")))
    }
}

pub fn parse_recipe(value: &Value) -> SootieResult<Recipe> {
    if let Some(text) = value.as_str() {
        Ok(serde_json::from_str(text)?)
    } else {
        Ok(serde_json::from_value(value.clone())?)
    }
}

fn deserialize_recipe_param_map<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, RecipeParam>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_map_or_empty(deserializer)
}

fn deserialize_value_map<'de, D>(deserializer: D) -> Result<BTreeMap<String, Value>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_map_or_empty(deserializer)
}

fn deserialize_map_or_empty<'de, D, T>(deserializer: D) -> Result<BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: for<'a> Deserialize<'a>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(BTreeMap::new()),
        Some(Value::Array(items)) if items.is_empty() => Ok(BTreeMap::new()),
        Some(Value::Object(map)) => serde_json::from_value(Value::Object(map))
            .map_err(|error| serde::de::Error::custom(error.to_string())),
        Some(other) => Err(serde::de::Error::custom(format!(
            "expected object, null, or empty array for params, got {other}"
        ))),
    }
}

fn legacy_app_name(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(name) => Some(name.clone()),
        Value::Object(map) => map
            .get("name")
            .or_else(|| map.get("app"))
            .or_else(|| map.get("app_id"))
            .or_else(|| map.get("platform_app_id"))
            .or_else(|| map.get("bundle_id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

pub fn substitute_params(value: &Value, params: &BTreeMap<String, Value>) -> Value {
    match value {
        Value::String(text) => {
            let mut rendered = text.clone();
            for (key, value) in params {
                let replacement = value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string());
                rendered = rendered.replace(&format!("{{{{{key}}}}}"), &replacement);
            }
            Value::String(rendered)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| substitute_params(item, params))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), substitute_params(value, params)))
                .collect(),
        ),
        other => other.clone(),
    }
}

pub fn recipe_step_tool_call(
    step: &RecipeStep,
    recipe_app: Option<&str>,
) -> SootieResult<(String, Value)> {
    if let Some(tool) = &step.tool {
        return Ok((tool.clone(), step.args.clone()));
    }

    let action = step.action.as_deref().ok_or_else(|| {
        SootieError::InvalidArguments("recipe step requires tool or action".into())
    })?;
    let mut args = serde_json::Map::new();
    let step_app = legacy_app_name(step.app.as_ref());
    insert_app(
        &mut args,
        step_app.as_deref().or(recipe_app),
        &step.params,
        step.target.as_ref(),
    );

    match action {
        "screenshot" => Ok(("sootie_screenshot".to_string(), Value::Object(args))),
        "click" => {
            insert_target_args(&mut args, step.target.as_ref());
            insert_string_alias(&mut args, &step.params, "query", &["query", "target"]);
            insert_string(&mut args, &step.params, "role");
            insert_string(&mut args, &step.params, "dom_id");
            insert_string_with_fallback(&mut args, &step.params, "button", step.button.as_ref());
            insert_number(&mut args, &step.params, "x");
            insert_number(&mut args, &step.params, "y");
            insert_u32_with_fallback(&mut args, &step.params, "count", step.count.as_ref());
            Ok(("sootie_click".to_string(), Value::Object(args)))
        }
        "hover" => {
            insert_target_args(&mut args, step.target.as_ref());
            insert_string_alias(&mut args, &step.params, "query", &["query", "target"]);
            insert_string(&mut args, &step.params, "role");
            insert_string(&mut args, &step.params, "dom_id");
            insert_number(&mut args, &step.params, "x");
            insert_number(&mut args, &step.params, "y");
            Ok(("sootie_hover".to_string(), Value::Object(args)))
        }
        "long_press" => {
            insert_target_args(&mut args, step.target.as_ref());
            insert_string_alias(&mut args, &step.params, "query", &["query", "target"]);
            insert_string(&mut args, &step.params, "role");
            insert_string(&mut args, &step.params, "dom_id");
            insert_string_with_fallback(&mut args, &step.params, "button", step.button.as_ref());
            insert_number(&mut args, &step.params, "x");
            insert_number(&mut args, &step.params, "y");
            insert_number(&mut args, &step.params, "duration");
            Ok(("sootie_long_press".to_string(), Value::Object(args)))
        }
        "drag" => {
            insert_from_target_args(&mut args, step.target.as_ref());
            insert_to_target_args(&mut args, step.to_target.as_ref());
            insert_string_alias(&mut args, &step.params, "query", &["query", "target"]);
            insert_string(&mut args, &step.params, "role");
            insert_string(&mut args, &step.params, "dom_id");
            insert_number(&mut args, &step.params, "from_x");
            insert_number(&mut args, &step.params, "from_y");
            insert_number(&mut args, &step.params, "to_x");
            insert_number(&mut args, &step.params, "to_y");
            insert_number(&mut args, &step.params, "duration");
            insert_number(&mut args, &step.params, "hold_duration");
            Ok(("sootie_drag".to_string(), Value::Object(args)))
        }
        "type" | "paste_text" => {
            insert_target_into_args(&mut args, step.target.as_ref());
            insert_string_alias(&mut args, &step.params, "into", &["into", "target"]);
            insert_string(&mut args, &step.params, "dom_id");
            insert_string_with_fallback(&mut args, &step.params, "text", step.text.as_ref());
            insert_bool(&mut args, &step.params, "clear");
            insert_bool_with_fallback(&mut args, &step.params, "clear", step.clear_first.as_ref());
            Ok(("sootie_type".to_string(), Value::Object(args)))
        }
        "press" => {
            insert_string_with_fallback(&mut args, &step.params, "key", step.key.as_ref());
            insert_string_array(&mut args, &step.params, "modifiers");
            Ok(("sootie_press".to_string(), Value::Object(args)))
        }
        "hotkey" => {
            insert_string_array_with_fallback(&mut args, &step.params, "keys", step.keys.as_ref());
            Ok(("sootie_hotkey".to_string(), Value::Object(args)))
        }
        "scroll" => {
            args.entry("direction".to_string())
                .or_insert_with(|| json!("down"));
            insert_string_with_fallback(
                &mut args,
                &step.params,
                "direction",
                step.direction.as_ref(),
            );
            insert_i32_with_fallback(&mut args, &step.params, "amount", step.amount.as_ref());
            insert_number(&mut args, &step.params, "x");
            insert_number(&mut args, &step.params, "y");
            Ok(("sootie_scroll".to_string(), Value::Object(args)))
        }
        "focus" => {
            insert_string(&mut args, &step.params, "window");
            if let Some(app) = string_value(step.params.get("app"))
                .or_else(|| {
                    step.params
                        .get("to_app")
                        .and_then(|app| legacy_app_name(Some(app)))
                })
                .or_else(|| recipe_app.map(str::to_string))
            {
                args.insert("app".to_string(), json!(app));
            }
            Ok(("sootie_focus".to_string(), Value::Object(args)))
        }
        "window" => {
            insert_string(&mut args, &step.params, "action");
            insert_string(&mut args, &step.params, "window");
            insert_number(&mut args, &step.params, "x");
            insert_number(&mut args, &step.params, "y");
            insert_number(&mut args, &step.params, "width");
            insert_number(&mut args, &step.params, "height");
            Ok(("sootie_window".to_string(), Value::Object(args)))
        }
        "wait" => {
            if !has_wait_condition(&step.params) && step.target.is_none() {
                return Ok((
                    "__delay".to_string(),
                    json!({ "seconds": legacy_timeout_seconds(&step.params, step.timeout.as_ref()) }),
                ));
            }
            insert_wait_params(
                &mut args,
                &step.params,
                step.target.as_ref(),
                step.timeout.as_ref(),
            );
            Ok(("sootie_wait".to_string(), Value::Object(args)))
        }
        other => Err(SootieError::InvalidArguments(format!(
            "unknown recipe action '{other}'"
        ))),
    }
}

pub fn recipe_wait_tool_call(
    wait: &RecipeWaitCondition,
    recipe_app: Option<&str>,
) -> SootieResult<(String, Value)> {
    if wait.condition == "delay" {
        return Ok((
            "__delay".to_string(),
            json!({ "seconds": wait_timeout_seconds(wait).unwrap_or(0.5).max(0.0) }),
        ));
    }

    let mut args = serde_json::Map::new();
    if let Some(app) = recipe_app {
        args.insert("app".to_string(), json!(app));
    }
    args.insert("condition".to_string(), json!(wait.condition));
    if let Some(value) = &wait.value {
        args.insert("value".to_string(), json!(value));
    } else if let Some(target) = &wait.target {
        if let Some(query) = &target.computed_name_contains {
            args.insert("value".to_string(), json!(query));
        }
    }
    if let Some(timeout) = wait_timeout_seconds(wait) {
        args.insert("timeout".to_string(), json!(timeout));
    }
    if let Some(interval) = wait_interval_seconds(wait) {
        args.insert("interval".to_string(), json!(interval));
    }
    Ok(("sootie_wait".to_string(), Value::Object(args)))
}

fn validate_recipe(recipe: &Recipe) -> SootieResult<()> {
    if recipe.name.trim().is_empty() {
        return Err(SootieError::InvalidArguments(
            "recipe.name is required".to_string(),
        ));
    }
    if recipe.schema_version == 0 {
        return Err(SootieError::InvalidArguments(
            "recipe.schema_version must be positive".to_string(),
        ));
    }
    Ok(())
}

fn insert_app(
    args: &mut serde_json::Map<String, Value>,
    recipe_app: Option<&str>,
    params: &BTreeMap<String, Value>,
    target: Option<&RecipeTarget>,
) {
    if let Some(app) = string_value(params.get("app"))
        .or_else(|| {
            params
                .get("to_app")
                .and_then(|app| legacy_app_name(Some(app)))
        })
        .or_else(|| recipe_app.map(str::to_string))
    {
        args.insert("app".to_string(), json!(app));
    } else if let Some(app) = target.and_then(|target| legacy_app_name(target.app.as_ref())) {
        args.insert("app".to_string(), json!(app));
    }
}

fn insert_target_args(args: &mut serde_json::Map<String, Value>, target: Option<&RecipeTarget>) {
    let Some(target) = target else {
        return;
    };
    insert_target_app(args, target);
    insert_target_coordinate(args, target, "x", "y");
    if let Some(query) = &target.computed_name_contains {
        args.insert("query".to_string(), json!(query));
    } else if let Some(query) = target.name.as_ref().or(target.text.as_ref()) {
        args.insert("query".to_string(), json!(query));
    }
    insert_criteria_args(args, target);
    insert_legacy_selector_args(args, target);
}

fn insert_target_into_args(
    args: &mut serde_json::Map<String, Value>,
    target: Option<&RecipeTarget>,
) {
    let Some(target) = target else {
        return;
    };
    insert_target_app(args, target);
    if let Some(query) = &target.computed_name_contains {
        args.insert("into".to_string(), json!(query));
    } else if let Some(query) = target.name.as_ref().or(target.text.as_ref()) {
        args.insert("into".to_string(), json!(query));
    }
    insert_criteria_args(args, target);
    insert_legacy_selector_args(args, target);
}

fn insert_from_target_args(
    args: &mut serde_json::Map<String, Value>,
    target: Option<&RecipeTarget>,
) {
    let Some(target) = target else {
        return;
    };
    insert_target_app(args, target);
    if insert_target_coordinate(args, target, "from_x", "from_y") {
        return;
    }
    if let Some(query) = &target.computed_name_contains {
        args.insert("query".to_string(), json!(query));
    } else if let Some(query) = target.name.as_ref().or(target.text.as_ref()) {
        args.insert("query".to_string(), json!(query));
    }
    insert_criteria_args(args, target);
    insert_legacy_selector_args(args, target);
}

fn insert_to_target_args(args: &mut serde_json::Map<String, Value>, target: Option<&RecipeTarget>) {
    let Some(target) = target else {
        return;
    };
    if insert_target_coordinate(args, target, "to_x", "to_y") {
        return;
    }
    if let Some(target_value) = target_value(target) {
        args.insert("to_target".to_string(), target_value);
    }
}

fn insert_target_app(args: &mut serde_json::Map<String, Value>, target: &RecipeTarget) {
    if args.contains_key("app") {
        return;
    }
    if let Some(app) = legacy_app_name(target.app.as_ref()) {
        args.insert("app".to_string(), json!(app));
    }
}

fn insert_target_coordinate(
    args: &mut serde_json::Map<String, Value>,
    target: &RecipeTarget,
    x_key: &str,
    y_key: &str,
) -> bool {
    let point = target
        .coordinate
        .as_ref()
        .or(target.window_coordinate.as_ref());
    if let Some(point) = point {
        args.insert(x_key.to_string(), json!(point.x));
        args.insert(y_key.to_string(), json!(point.y));
        true
    } else {
        false
    }
}

fn target_value(target: &RecipeTarget) -> Option<Value> {
    let mut value = serde_json::Map::new();
    if let Some(app) = target
        .app
        .as_ref()
        .and_then(|app| legacy_app_name(Some(app)))
    {
        value.insert("app".to_string(), json!(app));
    }
    let mut selector = serde_json::Map::new();
    if let Some(query) = target
        .computed_name_contains
        .as_ref()
        .or(target.name.as_ref())
        .or(target.text.as_ref())
    {
        selector.insert("query".to_string(), json!(query));
    }
    if let Some(role) = &target.role {
        selector.insert("role".to_string(), json!(role));
    }
    if let Some(dom_id) = target.dom_id.as_ref().or(target.id.as_ref()) {
        selector.insert("dom_id".to_string(), json!(dom_id));
    }
    if let Some(dom_class) = &target.dom_class {
        selector.insert("dom_class".to_string(), json!(dom_class));
    }
    if let Some(identifier) = &target.identifier {
        selector.insert("identifier".to_string(), json!(identifier));
    }
    if !selector.is_empty() {
        value.insert("selector".to_string(), Value::Object(selector));
    }
    (!value.is_empty()).then_some(Value::Object(value))
}

fn insert_criteria_args(args: &mut serde_json::Map<String, Value>, target: &RecipeTarget) {
    for criterion in &target.criteria {
        match criterion.attribute.as_str() {
            "AXRole" => {
                args.insert("role".to_string(), json!(criterion.value));
            }
            "AXDOMIdentifier" => {
                args.insert("dom_id".to_string(), json!(criterion.value));
            }
            "AXDOMClassList" => {
                args.insert("dom_class".to_string(), json!(criterion.value));
            }
            "AXIdentifier" => {
                args.insert("identifier".to_string(), json!(criterion.value));
            }
            _ => {}
        }
    }
}

fn insert_legacy_selector_args(args: &mut serde_json::Map<String, Value>, target: &RecipeTarget) {
    if let Some(role) = &target.role {
        args.insert("role".to_string(), json!(role));
    }
    if let Some(dom_id) = target.dom_id.as_ref().or(target.id.as_ref()) {
        args.insert("dom_id".to_string(), json!(dom_id));
    }
    if let Some(dom_class) = &target.dom_class {
        args.insert("dom_class".to_string(), json!(dom_class));
    }
    if let Some(identifier) = &target.identifier {
        args.insert("identifier".to_string(), json!(identifier));
    }
}

fn insert_wait_params(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    target: Option<&RecipeTarget>,
    legacy_timeout: Option<&Value>,
) {
    insert_string(args, params, "condition");
    insert_string(args, params, "value");
    insert_seconds_alias(args, params, "timeout", "timeout_ms");
    insert_seconds_alias(args, params, "interval", "interval_ms");
    if !args.contains_key("condition") {
        args.insert("condition".to_string(), json!("elementExists"));
    }
    if !args.contains_key("timeout") {
        args.insert(
            "timeout".to_string(),
            json!(legacy_timeout_seconds(params, legacy_timeout)),
        );
    }
    if !args.contains_key("value") {
        if let Some(target) = target.and_then(|target| target.computed_name_contains.as_ref()) {
            args.insert("value".to_string(), json!(target));
        } else if let Some(target) =
            target.and_then(|target| target.name.as_ref().or(target.text.as_ref()))
        {
            args.insert("value".to_string(), json!(target));
        }
    }
    if let Some(target) = target.and_then(target_value) {
        args.insert("target".to_string(), target);
    }
}

fn insert_string(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
) {
    if let Some(value) = string_value(params.get(key)) {
        args.insert(key.to_string(), json!(value));
    }
}

fn insert_string_with_fallback(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
    fallback: Option<&Value>,
) {
    if let Some(value) =
        string_value(params.get(key)).or_else(|| fallback.and_then(string_value_from_value))
    {
        args.insert(key.to_string(), json!(value));
    }
}

fn insert_string_alias(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    output_key: &str,
    input_keys: &[&str],
) {
    for key in input_keys {
        if let Some(value) = string_value(params.get(*key)) {
            args.insert(output_key.to_string(), json!(value));
            return;
        }
    }
}

fn insert_number(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
) {
    if let Some(value) = f64_value(params.get(key)) {
        args.insert(key.to_string(), json!(value));
    }
}

fn insert_seconds_alias(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    seconds_key: &str,
    millis_key: &str,
) {
    if let Some(value) = f64_value(params.get(seconds_key)) {
        args.insert(seconds_key.to_string(), json!(value));
    } else if let Some(value) = f64_value(params.get(millis_key)) {
        args.insert(seconds_key.to_string(), json!(value / 1000.0));
    }
}

fn insert_i32_with_fallback(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
    fallback: Option<&Value>,
) {
    if let Some(value) =
        i64_value(params.get(key)).or_else(|| fallback.and_then(i64_value_from_value))
    {
        args.insert(key.to_string(), json!(value as i32));
    }
}

fn insert_u32_with_fallback(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
    fallback: Option<&Value>,
) {
    if let Some(value) =
        i64_value(params.get(key)).or_else(|| fallback.and_then(i64_value_from_value))
    {
        args.insert(key.to_string(), json!(value.max(0) as u32));
    }
}

fn insert_bool(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
) {
    if let Some(value) = bool_value(params.get(key)) {
        args.insert(key.to_string(), json!(value));
    }
}

fn insert_bool_with_fallback(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
    fallback: Option<&Value>,
) {
    if let Some(value) =
        bool_value(params.get(key)).or_else(|| fallback.and_then(bool_value_from_value))
    {
        args.insert(key.to_string(), json!(value));
    }
}

fn insert_string_array(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
) {
    if let Some(value) = params.get(key) {
        let items = match value {
            Value::Array(items) => items
                .iter()
                .filter_map(string_value_from_value)
                .collect::<Vec<_>>(),
            _ => string_value_from_value(value)
                .map(|text| {
                    text.split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        };
        args.insert(key.to_string(), json!(items));
    }
}

fn insert_string_array_with_fallback(
    args: &mut serde_json::Map<String, Value>,
    params: &BTreeMap<String, Value>,
    key: &str,
    fallback: Option<&Value>,
) {
    if params.get(key).is_some() {
        insert_string_array(args, params, key);
        return;
    }
    if let Some(items) = fallback.and_then(string_array_from_value) {
        args.insert(key.to_string(), json!(items));
    }
}

fn has_wait_condition(params: &BTreeMap<String, Value>) -> bool {
    params.get("condition").and_then(Value::as_str).is_some()
}

fn legacy_timeout_seconds(params: &BTreeMap<String, Value>, fallback: Option<&Value>) -> f64 {
    match f64_value(params.get("timeout")).or_else(|| fallback.and_then(f64_value_from_value)) {
        Some(timeout) if timeout > 10.0 => timeout / 1000.0,
        Some(timeout) => timeout.max(0.0),
        None => f64_value(params.get("timeout_ms"))
            .map(|timeout_ms| (timeout_ms / 1000.0).max(0.0))
            .unwrap_or(0.5),
    }
}

fn wait_timeout_seconds(wait: &RecipeWaitCondition) -> Option<f64> {
    wait.timeout
        .or_else(|| wait.timeout_ms.map(|timeout_ms| timeout_ms / 1000.0))
}

fn wait_interval_seconds(wait: &RecipeWaitCondition) -> Option<f64> {
    wait.interval
        .or_else(|| wait.interval_ms.map(|interval_ms| interval_ms / 1000.0))
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(string_value_from_value)
}

fn string_value_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        _ => None,
    }
}

fn f64_value(value: Option<&Value>) -> Option<f64> {
    value.and_then(f64_value_from_value)
}

fn f64_value_from_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }
}

fn i64_value(value: Option<&Value>) -> Option<i64> {
    value.and_then(i64_value_from_value)
}

fn i64_value_from_value(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

fn bool_value(value: Option<&Value>) -> Option<bool> {
    value.and_then(bool_value_from_value)
}

fn bool_value_from_value(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        Value::String(text) => text.parse::<bool>().ok(),
        _ => None,
    }
}

fn string_array_from_value(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Array(items) => Some(items.iter().filter_map(string_value_from_value).collect()),
        _ => string_value_from_value(value).map(|text| {
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitutes_nested_params() {
        let mut params = BTreeMap::new();
        params.insert("name".to_string(), json!("Ada"));
        let value = json!({"text": "hello {{name}}", "arr": ["{{name}}"]});
        assert_eq!(
            substitute_params(&value, &params),
            json!({"text":"hello Ada","arr":["Ada"]})
        );
    }

    #[test]
    fn store_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = RecipeStore::new(dir.path().to_path_buf());
        let recipe = Recipe {
            schema_version: 1,
            name: "hello".to_string(),
            description: None,
            app: None,
            params: BTreeMap::new(),
            preconditions: None,
            steps: vec![],
            on_failure: None,
        };
        store.save(&recipe).unwrap();
        assert_eq!(store.get("hello").unwrap().name, "hello");
        assert_eq!(store.list().unwrap().len(), 1);
        assert!(store.delete("hello").unwrap());
    }

    #[test]
    fn parses_v2_action_recipe() {
        let recipe = parse_recipe(&json!({
            "schema_version": 2,
            "name": "finder-create-folder",
            "app": "Finder",
            "params": {
                "folder_name": {
                    "type": "string",
                    "required": true
                }
            },
            "steps": [
                {
                    "id": 1,
                    "action": "hotkey",
                    "params": { "keys": "cmd,shift,n" }
                },
                {
                    "id": 2,
                    "action": "type",
                    "target": {
                        "computedNameContains": "Name",
                        "criteria": [
                            { "attribute": "AXRole", "value": "AXTextField" }
                        ]
                    },
                    "params": { "text": "{{folder_name}}" }
                }
            ]
        }))
        .unwrap();

        assert_eq!(
            recipe.params["folder_name"].param_type.as_deref(),
            Some("string")
        );
        let (tool, args) = recipe_step_tool_call(&recipe.steps[0], recipe.app.as_deref()).unwrap();
        assert_eq!(tool, "sootie_hotkey");
        assert_eq!(args["app"], "Finder");
        assert_eq!(args["keys"], json!(["cmd", "shift", "n"]));

        let (tool, args) = recipe_step_tool_call(&recipe.steps[1], recipe.app.as_deref()).unwrap();
        assert_eq!(tool, "sootie_type");
        assert_eq!(args["into"], "Name");
        assert_eq!(args["role"], "AXTextField");
        assert_eq!(args["text"], "{{folder_name}}");
    }

    #[test]
    fn parses_legacy_empty_param_shapes() {
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "legacy-empty-params",
            "params": [],
            "steps": [
                {
                    "action": "wait",
                    "params": null
                }
            ]
        }))
        .unwrap();

        assert!(recipe.params.is_empty());
        assert!(recipe.steps[0].params.is_empty());
    }

    #[test]
    fn launch_recipe_action_is_not_part_of_public_contract() {
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "legacy-launch",
            "steps": [
                {
                    "action": "launch",
                    "app": {
                        "name": "Safari"
                    },
                    "params": null
                }
            ]
        }))
        .unwrap();

        let error = recipe_step_tool_call(&recipe.steps[0], recipe.app.as_deref()).unwrap_err();
        assert!(error.to_string().contains("unknown recipe action 'launch'"));
    }

    #[test]
    fn maps_compatible_app_alias_shapes() {
        let recipe = parse_recipe(&json!({
            "schema_version": 4,
            "name": "compatible-app-shapes",
            "steps": [
                {
                    "action": "focus",
                    "params": {
                        "to_app": {
                            "platform_app_id": "Finder"
                        }
                    }
                }
            ]
        }))
        .unwrap();

        let (tool, args) = recipe_step_tool_call(&recipe.steps[0], None).unwrap();
        assert_eq!(tool, "sootie_focus");
        assert_eq!(args["app"], "Finder");
    }

    #[test]
    fn maps_legacy_recorded_step_fields_to_tool_args() {
        let recipe = parse_recipe(&json!({
            "schema_version": 3,
            "name": "legacy-recording",
            "steps": [
                {
                    "action": "type",
                    "target": {
                        "app": { "name": "Chrome" },
                        "role": "textfield",
                        "name": "Address"
                    },
                    "text": "https://example.com",
                    "params": null
                },
                {
                    "action": "press",
                    "key": "Enter",
                    "params": null
                },
                {
                    "action": "click",
                    "target": { "coordinate": { "x": 40.0, "y": 90.0 } },
                    "button": "left",
                    "count": 2,
                    "params": null
                },
                {
                    "action": "drag",
                    "target": { "coordinate": { "x": 10.0, "y": 20.0 } },
                    "to_target": { "coordinate": { "x": 30.0, "y": 40.0 } },
                    "params": null
                },
                {
                    "action": "paste_text",
                    "text": "clipboard payload",
                    "params": null
                },
                {
                    "action": "screenshot",
                    "target": { "app": { "name": "Chrome" } },
                    "params": null
                },
                {
                    "action": "wait",
                    "target": {
                        "app": { "name": "Chrome" },
                        "role": "document"
                    },
                    "timeout": 1000,
                    "params": null
                },
                {
                    "action": "wait",
                    "timeout": 250,
                    "params": null
                }
            ]
        }))
        .unwrap();

        let (tool, args) = recipe_step_tool_call(&recipe.steps[0], None).unwrap();
        assert_eq!(tool, "sootie_type");
        assert_eq!(args["app"], "Chrome");
        assert_eq!(args["role"], "textfield");
        assert_eq!(args["into"], "Address");
        assert_eq!(args["text"], "https://example.com");

        let (tool, args) = recipe_step_tool_call(&recipe.steps[1], None).unwrap();
        assert_eq!(tool, "sootie_press");
        assert_eq!(args["key"], "Enter");

        let (tool, args) = recipe_step_tool_call(&recipe.steps[2], None).unwrap();
        assert_eq!(tool, "sootie_click");
        assert_eq!(args["x"], 40.0);
        assert_eq!(args["y"], 90.0);
        assert_eq!(args["button"], "left");
        assert_eq!(args["count"], 2);

        let (tool, args) = recipe_step_tool_call(&recipe.steps[3], None).unwrap();
        assert_eq!(tool, "sootie_drag");
        assert_eq!(args["from_x"], 10.0);
        assert_eq!(args["from_y"], 20.0);
        assert_eq!(args["to_x"], 30.0);
        assert_eq!(args["to_y"], 40.0);

        let (tool, args) = recipe_step_tool_call(&recipe.steps[4], None).unwrap();
        assert_eq!(tool, "sootie_type");
        assert_eq!(args["text"], "clipboard payload");

        let (tool, args) = recipe_step_tool_call(&recipe.steps[5], None).unwrap();
        assert_eq!(tool, "sootie_screenshot");
        assert_eq!(args["app"], "Chrome");

        let (tool, args) = recipe_step_tool_call(&recipe.steps[6], None).unwrap();
        assert_eq!(tool, "sootie_wait");
        assert_eq!(args["condition"], "elementExists");
        assert_eq!(args["target"]["app"], "Chrome");
        assert_eq!(args["target"]["selector"]["role"], "document");
        assert_eq!(args["timeout"], 1.0);

        let (tool, args) = recipe_step_tool_call(&recipe.steps[7], None).unwrap();
        assert_eq!(tool, "__delay");
        assert_eq!(args["seconds"], 0.25);
    }

    #[test]
    fn maps_drag_hold_duration_from_recipe() {
        let recipe = parse_recipe(&json!({
            "schema_version": 2,
            "name": "drag-item",
            "app": "Files",
            "steps": [{
                "action": "drag",
                "target": { "computedNameContains": "Report.pdf" },
                "params": {
                    "to_x": 500,
                    "to_y": 600,
                    "duration": "0.75",
                    "hold_duration": "0.3"
                }
            }]
        }))
        .unwrap();

        let (tool, args) = recipe_step_tool_call(&recipe.steps[0], recipe.app.as_deref()).unwrap();
        assert_eq!(tool, "sootie_drag");
        assert_eq!(args["app"], "Files");
        assert_eq!(args["query"], "Report.pdf");
        assert_eq!(args["to_x"], 500.0);
        assert_eq!(args["to_y"], 600.0);
        assert_eq!(args["duration"], 0.75);
        assert_eq!(args["hold_duration"], 0.3);
    }

    #[test]
    fn maps_millisecond_wait_aliases_from_recipe() {
        let recipe = parse_recipe(&json!({
            "schema_version": 2,
            "name": "wait-ms",
            "app": "Browser",
            "steps": [{
                "action": "wait",
                "target": { "computedNameContains": "Ready" },
                "params": {
                    "timeout_ms": 750,
                    "interval_ms": 125
                },
                "wait_after": {
                    "condition": "urlContains",
                    "value": "done",
                    "timeout_ms": 500,
                    "interval_ms": 100
                }
            }]
        }))
        .unwrap();

        let (tool, args) = recipe_step_tool_call(&recipe.steps[0], recipe.app.as_deref()).unwrap();
        assert_eq!(tool, "sootie_wait");
        assert_eq!(args["app"], "Browser");
        assert_eq!(args["value"], "Ready");
        assert_eq!(args["timeout"], 0.75);
        assert_eq!(args["interval"], 0.125);

        let (tool, args) = recipe_wait_tool_call(
            recipe.steps[0].wait_after.as_ref().unwrap(),
            recipe.app.as_deref(),
        )
        .unwrap();
        assert_eq!(tool, "sootie_wait");
        assert_eq!(args["app"], "Browser");
        assert_eq!(args["value"], "done");
        assert_eq!(args["timeout"], 0.5);
        assert_eq!(args["interval"], 0.1);
    }
}
