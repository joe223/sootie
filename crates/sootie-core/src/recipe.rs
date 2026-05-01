use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::selector::{Coordinate, Selector};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum StepTarget {
    Selector(Selector),
    Coordinate(Coordinate),
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
}

impl RecipeEngine {
    pub fn new() -> Self {
        Self {
            recipes: HashMap::new(),
        }
    }

    pub fn load(&mut self, recipe: Recipe) -> Result<(), RecipeError> {
        validate_recipe(&recipe)?;
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
        self.recipes
            .remove(name)
            .ok_or_else(|| RecipeError::NotFound(name.to_string()))
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
        let mut step = step.clone();

        if let Some(ref text) = step.text {
            step.text = Some(substitute_string(text, params));
        }

        step
    }
}

impl Default for RecipeEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn substitute_string(
    template: &str,
    params: &HashMap<String, serde_json::Value>,
) -> String {
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
        validate_step(step).map_err(|e| RecipeError::InvalidRecipe(format!("step {}: {}", i, e)))?;
    }

    Ok(())
}

fn validate_step(step: &RecipeStep) -> Result<(), String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_recipe(name: &str, steps: Vec<RecipeStep>) -> Recipe {
        Recipe {
            schema_version: CURRENT_SCHEMA_VERSION,
            name: name.to_string(),
            platforms: vec!["macos".to_string(), "windows".to_string(), "linux".to_string()],
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
        let mut engine = RecipeEngine::new();
        let recipe = make_recipe("my_recipe", vec![make_step("click")]);
        engine.load(recipe).unwrap();

        assert!(engine.get("my_recipe").is_some());
        assert!(engine.get("nonexistent").is_none());
    }

    #[test]
    fn test_recipe_engine_list() {
        let mut engine = RecipeEngine::new();
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
        let mut engine = RecipeEngine::new();
        engine
            .load(make_recipe("to_delete", vec![make_step("click")]))
            .unwrap();

        let deleted = engine.delete("to_delete").unwrap();
        assert_eq!(deleted.name, "to_delete");
        assert!(engine.get("to_delete").is_none());
    }

    #[test]
    fn test_recipe_engine_delete_not_found() {
        let mut engine = RecipeEngine::new();
        assert!(engine.delete("nonexistent").is_err());
    }

    #[test]
    fn test_resolve_params_with_provided() {
        let engine = RecipeEngine::new();
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
        let engine = RecipeEngine::new();
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
        let engine = RecipeEngine::new();
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
        let engine = RecipeEngine::new();
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
        assert_eq!(resolved.text, Some("Hello World, welcome to Sootie".to_string()));
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
                    target: Some(StepTarget::Selector(
                        serde_json::from_str(r#"{"app": "Chrome", "window": "Gmail", "name": "Compose", "role": "button"}"#).unwrap(),
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
                    target: Some(StepTarget::Selector(
                        serde_json::from_str(r#"{"role": "textfield", "name": "To"}"#).unwrap(),
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
                        "name": "Compose",
                        "role": "button",
                        "state": { "visible": true }
                    }
                },
                {
                    "action": "wait",
                    "target": {
                        "role": "textfield",
                        "name": "To"
                    },
                    "timeout": 5000
                },
                {
                    "action": "type",
                    "target": {
                        "role": "textfield",
                        "name": "To"
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
