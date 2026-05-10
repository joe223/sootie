use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::selector::{Coordinate, Target};

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Recipe {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub params: Vec<RecipeParam>,
    pub steps: Vec<RecipeStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecipeParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecipeStep {
    pub action: String,
    #[serde(default)]
    pub target: Option<StepTarget>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub keys: Option<Vec<String>>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub amount: Option<u32>,
    #[serde(default)]
    pub button: Option<String>,
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub to_target: Option<StepTarget>,
    #[serde(default)]
    pub params: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepTarget {
    Target(Target),
    Coordinate(Coordinate),
}

impl Serialize for StepTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            StepTarget::Target(target) => target.serialize(serializer),
            StepTarget::Coordinate(coordinate) => {
                #[derive(Serialize)]
                struct CoordinateTarget<'a> {
                    coordinate: &'a Coordinate,
                }

                CoordinateTarget { coordinate }.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for StepTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let Some(object) = value.as_object() else {
            return Err(serde::de::Error::custom("recipe target must be an object"));
        };

        if let Some(coordinate) = object.get("coordinate") {
            if object.len() != 1 {
                return Err(serde::de::Error::custom(
                    "coordinate recipe target cannot include selector fields",
                ));
            }
            return serde_json::from_value::<Coordinate>(coordinate.clone())
                .map(StepTarget::Coordinate)
                .map_err(serde::de::Error::custom);
        }

        serde_json::from_value::<Target>(value)
            .map(StepTarget::Target)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RecipeError {
    #[error("invalid recipe: {0}")]
    InvalidRecipe(String),

    #[error("missing required parameter: {0}")]
    MissingParam(String),

    #[error("step execution failed at step {step}: {error}")]
    StepFailed { step: usize, error: String },

    #[error("recipe not found: {0}")]
    NotFound(String),

    #[error("storage error: {0}")]
    StorageError(String),
}

pub struct RecipeEngine {
    recipes: HashMap<String, Recipe>,
    storage_dir: Option<PathBuf>,
}

impl RecipeEngine {
    pub fn new() -> Self {
        let mut engine = Self {
            recipes: HashMap::new(),
            storage_dir: default_recipe_storage_dir(),
        };
        engine.load_from_storage();
        engine
    }

    pub fn new_in_memory() -> Self {
        Self {
            recipes: HashMap::new(),
            storage_dir: None,
        }
    }

    pub fn new_with_storage_dir(storage_dir: PathBuf) -> Self {
        let mut engine = Self {
            recipes: HashMap::new(),
            storage_dir: Some(storage_dir),
        };
        engine.load_from_storage();
        engine
    }

    pub fn load(&mut self, recipe: Recipe) -> Result<(), RecipeError> {
        validate_recipe(&recipe)?;
        self.persist_recipe(&recipe)?;
        self.recipes.insert(recipe.name.clone(), recipe);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Recipe> {
        self.recipes.get(name)
    }

    pub fn list(&self) -> Vec<&Recipe> {
        self.recipes.values().collect()
    }

    pub fn delete(&mut self, name: &str) -> Result<Recipe, RecipeError> {
        let recipe = self
            .recipes
            .remove(name)
            .ok_or_else(|| RecipeError::NotFound(name.to_string()))?;
        self.delete_persisted_recipe(name)?;
        Ok(recipe)
    }

    pub fn resolve_params(
        &self,
        recipe: &Recipe,
        provided: &HashMap<String, serde_json::Value>,
    ) -> Result<HashMap<String, serde_json::Value>, RecipeError> {
        let mut resolved = HashMap::new();

        for param in &recipe.params {
            if let Some(value) = provided.get(&param.name) {
                resolved.insert(param.name.clone(), value.clone());
            } else if let Some(ref default) = param.default {
                resolved.insert(param.name.clone(), default.clone());
            } else if param.required {
                return Err(RecipeError::MissingParam(param.name.clone()));
            }
        }

        Ok(resolved)
    }

    pub fn substitute_step(
        &self,
        step: &RecipeStep,
        params: &HashMap<String, serde_json::Value>,
    ) -> RecipeStep {
        let Ok(value) = serde_json::to_value(step) else {
            return step.clone();
        };
        let substituted = substitute_value(value, params);
        serde_json::from_value(substituted).unwrap_or_else(|_| step.clone())
    }

    fn load_from_storage(&mut self) {
        let Some(storage_dir) = self.storage_dir.as_ref() else {
            return;
        };

        let Ok(entries) = fs::read_dir(storage_dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !is_recipe_file(&path) {
                continue;
            }

            let Ok(contents) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(recipe) = serde_json::from_str::<Recipe>(&contents) else {
                continue;
            };
            if validate_recipe(&recipe).is_ok() {
                self.recipes.insert(recipe.name.clone(), recipe);
            }
        }
    }

    fn persist_recipe(&self, recipe: &Recipe) -> Result<(), RecipeError> {
        let Some(storage_dir) = self.storage_dir.as_ref() else {
            return Ok(());
        };

        fs::create_dir_all(storage_dir).map_err(|e| {
            RecipeError::StorageError(format!("failed to create recipe dir: {}", e))
        })?;
        let path = recipe_path(storage_dir, &recipe.name);
        let json = serde_json::to_vec_pretty(recipe)
            .map_err(|e| RecipeError::StorageError(format!("failed to serialize recipe: {}", e)))?;
        fs::write(path, json).map_err(|e| {
            RecipeError::StorageError(format!("failed to write recipe file: {}", e))
        })?;
        Ok(())
    }

    fn delete_persisted_recipe(&self, name: &str) -> Result<(), RecipeError> {
        let Some(storage_dir) = self.storage_dir.as_ref() else {
            return Ok(());
        };

        let path = recipe_path(storage_dir, name);
        if path.exists() {
            fs::remove_file(path).map_err(|e| {
                RecipeError::StorageError(format!("failed to delete recipe file: {}", e))
            })?;
        }
        Ok(())
    }
}

impl Default for RecipeEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn substitute_string(template: &str, params: &HashMap<String, serde_json::Value>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        let placeholder = format!("${{{}}}", key);
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        result = result.replace(&placeholder, &replacement);
    }
    result
}

fn substitute_value(
    value: serde_json::Value,
    params: &HashMap<String, serde_json::Value>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => serde_json::Value::String(substitute_string(&s, params)),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(|item| substitute_value(item, params))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, substitute_value(value, params)))
                .collect(),
        ),
        other => other,
    }
}

fn default_recipe_storage_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("SOOTIE_RECIPE_DIR") {
        return Some(PathBuf::from(path));
    }

    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|path| path.join("sootie").join("recipes"));
    }

    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME").map(PathBuf::from).map(|path| {
            path.join("Library")
                .join("Application Support")
                .join("sootie")
                .join("recipes")
        });
    }

    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|path| path.join(".local").join("share"))
            })
            .map(|path| path.join("sootie").join("recipes"))
    }
}

fn recipe_path(storage_dir: &Path, recipe_name: &str) -> PathBuf {
    storage_dir.join(format!("{}.json", sanitize_recipe_name(recipe_name)))
}

fn sanitize_recipe_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            result.push(ch);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "_".to_string()
    } else {
        result
    }
}

fn is_recipe_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
}

pub fn validate_recipe(recipe: &Recipe) -> Result<(), RecipeError> {
    if recipe.name.is_empty() {
        return Err(RecipeError::InvalidRecipe(
            "recipe name cannot be empty".to_string(),
        ));
    }

    if recipe.steps.is_empty() {
        return Err(RecipeError::InvalidRecipe(
            "recipe must have at least one step".to_string(),
        ));
    }

    if recipe.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(RecipeError::InvalidRecipe(format!(
            "schema version {} is not supported (max: {})",
            recipe.schema_version, CURRENT_SCHEMA_VERSION
        )));
    }

    for (i, step) in recipe.steps.iter().enumerate() {
        validate_step(step)
            .map_err(|e| RecipeError::InvalidRecipe(format!("step {}: {}", i, e)))?;
    }

    Ok(())
}

fn validate_step(step: &RecipeStep) -> Result<(), String> {
    if let Some(target) = &step.target {
        validate_step_target(target)?;
    }
    if let Some(target) = &step.to_target {
        validate_step_target(target)?;
    }

    match step.action.as_str() {
        "click" => Ok(()),
        "type" => {
            if step.text.is_none() {
                Err("type action requires 'text' field".to_string())
            } else {
                Ok(())
            }
        }
        "press" => {
            if step.key.is_none() {
                Err("press action requires 'key' field".to_string())
            } else {
                Ok(())
            }
        }
        "hotkey" => {
            if step.keys.is_none() {
                Err("hotkey action requires 'keys' field".to_string())
            } else {
                Ok(())
            }
        }
        "scroll" => {
            if step.direction.is_none() {
                Err("scroll action requires 'direction' field".to_string())
            } else {
                Ok(())
            }
        }
        "wait" => Ok(()),
        "hover" => Ok(()),
        "drag" => Ok(()),
        "focus" => Ok(()),
        "screenshot" => Ok(()),
        other => Err(format!("unknown action: '{}'", other)),
    }
}

fn validate_step_target(target: &StepTarget) -> Result<(), String> {
    match target {
        StepTarget::Target(target) => target.validate().map_err(|e| e.to_string()),
        StepTarget::Coordinate(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recipe(name: &str, steps: Vec<RecipeStep>) -> Recipe {
        Recipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: name.to_string(),
            platforms: vec![
                "macos".to_string(),
                "windows".to_string(),
                "linux".to_string(),
            ],
            params: vec![],
            steps,
        }
    }

    fn make_step(action: &str) -> RecipeStep {
        RecipeStep {
            action: action.to_string(),
            target: None,
            text: None,
            key: None,
            keys: None,
            direction: None,
            amount: None,
            button: None,
            count: None,
            timeout: None,
            to_target: None,
            params: None,
        }
    }

    #[test]
    fn test_validate_valid_recipe() {
        let recipe = make_recipe("test", vec![make_step("click")]);
        assert!(validate_recipe(&recipe).is_ok());
    }

    #[test]
    fn test_validate_empty_name() {
        let recipe = make_recipe("", vec![make_step("click")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_no_steps() {
        let recipe = make_recipe("test", vec![]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_unknown_action() {
        let recipe = make_recipe("test", vec![make_step("fly")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_type_needs_text() {
        let recipe = make_recipe("test", vec![make_step("type")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_type_with_text() {
        let mut step = make_step("type");
        step.text = Some("hello".to_string());
        let recipe = make_recipe("test", vec![step]);
        assert!(validate_recipe(&recipe).is_ok());
    }

    #[test]
    fn test_validate_press_needs_key() {
        let recipe = make_recipe("test", vec![make_step("press")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_hotkey_needs_keys() {
        let recipe = make_recipe("test", vec![make_step("hotkey")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_scroll_needs_direction() {
        let recipe = make_recipe("test", vec![make_step("scroll")]);
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_validate_future_schema_version() {
        let mut recipe = make_recipe("test", vec![make_step("click")]);
        recipe.schema_version = 99;
        assert!(validate_recipe(&recipe).is_err());
    }

    #[test]
    fn test_recipe_engine_load_and_get() {
        let mut engine = RecipeEngine::new_in_memory();
        let recipe = make_recipe("my_recipe", vec![make_step("click")]);
        engine.load(recipe).unwrap();

        assert!(engine.get("my_recipe").is_some());
        assert!(engine.get("nonexistent").is_none());
    }

    #[test]
    fn test_recipe_engine_list() {
        let mut engine = RecipeEngine::new_in_memory();
        engine
            .load(make_recipe("recipe1", vec![make_step("click")]))
            .unwrap();
        engine
            .load(make_recipe("recipe2", vec![make_step("click")]))
            .unwrap();

        let list = engine.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_recipe_engine_delete() {
        let mut engine = RecipeEngine::new_in_memory();
        engine
            .load(make_recipe("to_delete", vec![make_step("click")]))
            .unwrap();

        let deleted = engine.delete("to_delete").unwrap();
        assert_eq!(deleted.name, "to_delete");
        assert!(engine.get("to_delete").is_none());
    }

    #[test]
    fn test_recipe_engine_delete_not_found() {
        let mut engine = RecipeEngine::new_in_memory();
        assert!(engine.delete("nonexistent").is_err());
    }

    #[test]
    fn test_resolve_params_with_provided() {
        let engine = RecipeEngine::new_in_memory();
        let recipe = Recipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: "test".to_string(),
            platforms: vec![],
            params: vec![RecipeParam {
                name: "to".to_string(),
                param_type: "string".to_string(),
                required: true,
                default: None,
            }],
            steps: vec![make_step("click")],
        };

        let mut provided = HashMap::new();
        provided.insert(
            "to".to_string(),
            serde_json::Value::String("user@example.com".to_string()),
        );

        let resolved = engine.resolve_params(&recipe, &provided).unwrap();
        assert_eq!(
            resolved.get("to").unwrap(),
            &serde_json::Value::String("user@example.com".to_string())
        );
    }

    #[test]
    fn test_resolve_params_missing_required() {
        let engine = RecipeEngine::new_in_memory();
        let recipe = Recipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: "test".to_string(),
            platforms: vec![],
            params: vec![RecipeParam {
                name: "to".to_string(),
                param_type: "string".to_string(),
                required: true,
                default: None,
            }],
            steps: vec![make_step("click")],
        };

        let provided = HashMap::new();
        assert!(engine.resolve_params(&recipe, &provided).is_err());
    }

    #[test]
    fn test_resolve_params_with_default() {
        let engine = RecipeEngine::new_in_memory();
        let recipe = Recipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: "test".to_string(),
            platforms: vec![],
            params: vec![RecipeParam {
                name: "body".to_string(),
                param_type: "string".to_string(),
                required: false,
                default: Some(serde_json::Value::String("default body".to_string())),
            }],
            steps: vec![make_step("click")],
        };

        let provided = HashMap::new();
        let resolved = engine.resolve_params(&recipe, &provided).unwrap();
        assert_eq!(
            resolved.get("body").unwrap(),
            &serde_json::Value::String("default body".to_string())
        );
    }

    #[test]
    fn test_substitute_step_text() {
        let engine = RecipeEngine::new_in_memory();
        let step = RecipeStep {
            action: "type".to_string(),
            target: None,
            text: Some("Hello ${name}, welcome to ${app}".to_string()),
            key: None,
            keys: None,
            direction: None,
            amount: None,
            button: None,
            count: None,
            timeout: None,
            to_target: None,
            params: None,
        };

        let mut params = HashMap::new();
        params.insert(
            "name".to_string(),
            serde_json::Value::String("World".to_string()),
        );
        params.insert(
            "app".to_string(),
            serde_json::Value::String("Sootie".to_string()),
        );

        let resolved = engine.substitute_step(&step, &params);
        assert_eq!(
            resolved.text,
            Some("Hello World, welcome to Sootie".to_string())
        );
    }

    #[test]
    fn test_substitute_step_target_fields() {
        let engine = RecipeEngine::new_in_memory();
        let step = RecipeStep {
            action: "click".to_string(),
            target: Some(StepTarget::Target(
                serde_json::from_value(serde_json::json!({
                    "app": "${app}",
                    "selector": { "name": "${button_name}" }
                }))
                .unwrap(),
            )),
            text: None,
            key: None,
            keys: None,
            direction: None,
            amount: None,
            button: None,
            count: None,
            timeout: None,
            to_target: None,
            params: None,
        };

        let mut params = HashMap::new();
        params.insert(
            "app".to_string(),
            serde_json::Value::String("Chrome".to_string()),
        );
        params.insert(
            "button_name".to_string(),
            serde_json::Value::String("Compose".to_string()),
        );

        let resolved = engine.substitute_step(&step, &params);
        match resolved.target.unwrap() {
            StepTarget::Target(target) => {
                assert_eq!(target.app.unwrap().name.as_deref(), Some("Chrome"));
                assert_eq!(target.selector.name.as_deref(), Some("Compose"));
            }
            _ => panic!("expected structured target"),
        }
    }

    #[test]
    fn test_recipe_engine_persists_to_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let recipe_path = recipe_path(temp_dir.path(), "persist-me");
        let mut engine = RecipeEngine::new_with_storage_dir(temp_dir.path().to_path_buf());
        let recipe = make_recipe("persist-me", vec![make_step("click")]);

        engine.load(recipe).unwrap();

        assert!(recipe_path.exists());
        let persisted = fs::read_to_string(recipe_path).unwrap();
        assert!(persisted.contains("\"name\": \"persist-me\""));
    }

    #[test]
    fn test_recipe_engine_loads_from_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let recipe = make_recipe("from-disk", vec![make_step("click")]);
        let path = recipe_path(temp_dir.path(), &recipe.name);
        fs::write(path, serde_json::to_vec_pretty(&recipe).unwrap()).unwrap();

        let engine = RecipeEngine::new_with_storage_dir(temp_dir.path().to_path_buf());

        assert!(engine.get("from-disk").is_some());
    }

    #[test]
    fn test_recipe_serialize_deserialize() {
        let recipe = Recipe {
            schema_version: 3,
            name: "gmail-send".to_string(),
            platforms: vec!["macos".to_string(), "windows".to_string(), "linux".to_string()],
            params: vec![
                RecipeParam {
                    name: "to".to_string(),
                    param_type: "string".to_string(),
                    required: true,
                    default: None,
                },
                RecipeParam {
                    name: "subject".to_string(),
                    param_type: "string".to_string(),
                    required: true,
                    default: None,
                },
            ],
            steps: vec![
                RecipeStep {
                    action: "click".to_string(),
                    target: Some(StepTarget::Target(
                        serde_json::from_str(r#"{"app": "Chrome", "window": "Gmail", "selector": {"name": "Compose", "role": "button"}}"#).unwrap(),
                    )),
                    text: None,
                    key: None,
                    keys: None,
                    direction: None,
                    amount: None,
                    button: None,
                    count: None,
                    timeout: None,
                    to_target: None,
                    params: None,
                },
                RecipeStep {
                    action: "type".to_string(),
                    target: Some(StepTarget::Target(
                        serde_json::from_str(r#"{"selector": {"role": "textfield", "name": "To"}}"#).unwrap(),
                    )),
                    text: Some("${to}".to_string()),
                    key: None,
                    keys: None,
                    direction: None,
                    amount: None,
                    button: None,
                    count: None,
                    timeout: None,
                    to_target: None,
                    params: None,
                },
            ],
        };

        let json = serde_json::to_string_pretty(&recipe).unwrap();
        let deserialized: Recipe = serde_json::from_str(&json).unwrap();
        assert_eq!(recipe, deserialized);
    }

    #[test]
    fn test_recipe_from_readme_json() {
        let json = r#"{
            "schema_version": 3,
            "name": "gmail-send",
            "platforms": ["macos", "windows", "linux"],
            "params": [
                { "name": "to", "type": "string", "required": true },
                { "name": "subject", "type": "string", "required": true },
                { "name": "body", "type": "string", "required": false }
            ],
            "steps": [
                {
                    "action": "click",
                    "target": {
                        "app": "Chrome",
                        "window": "Gmail",
                        "selector": {
                            "name": "Compose",
                            "role": "button"
                        }
                    }
                },
                {
                    "action": "wait",
                    "target": {
                        "selector": {
                            "role": "textfield",
                            "name": "To"
                        }
                    },
                    "timeout": 5000
                },
                {
                    "action": "type",
                    "target": {
                        "selector": {
                            "role": "textfield",
                            "name": "To"
                        }
                    },
                    "text": "${to}"
                }
            ]
        }"#;

        let recipe: Recipe = serde_json::from_str(json).unwrap();
        assert_eq!(recipe.name, "gmail-send");
        assert_eq!(recipe.steps.len(), 3);
        assert!(validate_recipe(&recipe).is_ok());
    }
}
