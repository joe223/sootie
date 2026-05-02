use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogConfig {
    pub level: LogLevel,
    pub file_path: Option<String>,
    pub log_tool_calls: bool,
    pub log_perception: bool,
    pub log_actions: bool,
    pub log_cascade: bool,
    pub log_recipes: bool,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: LogLevel::Info,
            file_path: None,
            log_tool_calls: true,
            log_perception: true,
            log_actions: true,
            log_cascade: true,
            log_recipes: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallLog {
    pub tool_name: String,
    pub request_id: Option<serde_json::Value>,
    pub arguments: serde_json::Value,
    pub success: bool,
    pub error_message: Option<String>,
    pub duration_ms: u64,
    pub backend_used: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerceptionLog {
    pub operation: String,
    pub selector: Option<serde_json::Value>,
    pub success: bool,
    pub result_summary: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionLog {
    pub action_type: String,
    pub target: Option<String>,
    pub coordinate: Option<(f64, f64)>,
    pub success: bool,
    pub backend_used: Option<String>,
    pub error_message: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CascadeLog {
    pub step: String,
    pub backend: String,
    pub success: bool,
    pub fallback_triggered: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeLog {
    pub operation: String,
    pub recipe_name: String,
    pub step_count: Option<usize>,
    pub params_provided: Option<Vec<String>>,
    pub success: bool,
    pub error_message: Option<String>,
}

pub struct SootieLogger {
    config: LogConfig,
}

impl SootieLogger {
    pub fn new(config: LogConfig) -> Self {
        Self { config }
    }

    pub fn log_tool_call(&self, log: &ToolCallLog) {
        if !self.config.log_tool_calls {
            return;
        }

        if log.success {
            info!(
                tool = %log.tool_name,
                request_id = ?log.request_id,
                duration_ms = log.duration_ms,
                backend = ?log.backend_used,
                "Tool call completed"
            );
        } else {
            error!(
                tool = %log.tool_name,
                request_id = ?log.request_id,
                duration_ms = log.duration_ms,
                error = ?log.error_message,
                "Tool call failed"
            );
        }
    }

    pub fn log_perception(&self, log: &PerceptionLog) {
        if !self.config.log_perception {
            return;
        }

        if log.success {
            debug!(
                operation = %log.operation,
                selector = ?log.selector,
                result = %log.result_summary,
                duration_ms = log.duration_ms,
                "Perception operation completed"
            );
        } else {
            warn!(
                operation = %log.operation,
                selector = ?log.selector,
                duration_ms = log.duration_ms,
                "Perception operation failed"
            );
        }
    }

    pub fn log_action(&self, log: &ActionLog) {
        if !self.config.log_actions {
            return;
        }

        if log.success {
            info!(
                action = %log.action_type,
                target = ?log.target,
                coordinate = ?log.coordinate,
                backend = ?log.backend_used,
                duration_ms = log.duration_ms,
                "Action completed"
            );
        } else {
            error!(
                action = %log.action_type,
                target = ?log.target,
                coordinate = ?log.coordinate,
                error = ?log.error_message,
                duration_ms = log.duration_ms,
                "Action failed"
            );
        }
    }

    pub fn log_cascade(&self, log: &CascadeLog) {
        if !self.config.log_cascade {
            return;
        }

        if log.fallback_triggered {
            warn!(
                step = %log.step,
                backend = %log.backend,
                success = log.success,
                duration_ms = log.duration_ms,
                "Cascade fallback triggered"
            );
        } else {
            debug!(
                step = %log.step,
                backend = %log.backend,
                success = log.success,
                duration_ms = log.duration_ms,
                "Cascade step completed"
            );
        }
    }

    pub fn log_recipe(&self, log: &RecipeLog) {
        if !self.config.log_recipes {
            return;
        }

        if log.success {
            info!(
                operation = %log.operation,
                recipe = %log.recipe_name,
                steps = ?log.step_count,
                params = ?log.params_provided,
                "Recipe operation completed"
            );
        } else {
            error!(
                operation = %log.operation,
                recipe = %log.recipe_name,
                error = ?log.error_message,
                "Recipe operation failed"
            );
        }
    }

    pub fn log_mcp_request(&self, method: &str, id: &Option<serde_json::Value>) {
        info!(
            method = %method,
            request_id = ?id,
            "MCP request received"
        );
    }

    pub fn log_mcp_response(&self, method: &str, success: bool, duration: Duration) {
        if success {
            debug!(
                method = %method,
                duration_ms = duration.as_millis() as u64,
                "MCP response sent"
            );
        } else {
            error!(
                method = %method,
                duration_ms = duration.as_millis() as u64,
                "MCP error response sent"
            );
        }
    }

    pub fn log_session_start(&self, platform: &str, version: &str) {
        info!(
            platform = %platform,
            version = %version,
            "Sootie session started"
        );
    }

    pub fn log_session_end(&self) {
        info!("Sootie session ended");
    }

    pub fn log_permission_check(&self, platform: &str, granted: bool) {
        if granted {
            info!(platform = %platform, "Permissions verified");
        } else {
            warn!(platform = %platform, "Permissions not granted - some features may be limited");
        }
    }

    pub fn log_platform_init(&self, platform: &str, success: bool) {
        if success {
            info!(platform = %platform, "Platform provider initialized");
        } else {
            error!(platform = %platform, "Failed to initialize platform provider");
        }
    }
}

pub fn create_duration_ms(start: std::time::Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_config_default() {
        let config = LogConfig::default();
        assert_eq!(config.level, LogLevel::Info);
        assert!(config.file_path.is_none());
        assert!(config.log_tool_calls);
        assert!(config.log_perception);
        assert!(config.log_actions);
        assert!(config.log_cascade);
        assert!(config.log_recipes);
    }

    #[test]
    fn test_log_config_serialize() {
        let config = LogConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: LogConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, deserialized);
    }

#[test]
    fn test_tool_call_log_serialize() {
        let log = ToolCallLog {
            tool_name: "sootie_click".to_string(),
            request_id: Some(serde_json::Value::Number(1.into())),
            arguments: serde_json::json!({"button": "left"}),
            success: true,
            error_message: None,
            duration_ms: 50,
            backend_used: Some("cgevent".to_string()),
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: ToolCallLog = serde_json::from_str(&json).unwrap();
        assert_eq!(log.tool_name, deserialized.tool_name);
        assert_eq!(log.success, deserialized.success);
        assert_eq!(log.duration_ms, deserialized.duration_ms);
    }
    
    #[test]
    fn test_perception_log_serialize() {
        let log = PerceptionLog {
            operation: "find".to_string(),
            selector: Some(serde_json::json!({"role": "button"})),
            success: true,
            result_summary: "Found 3 elements".to_string(),
            duration_ms: 25,
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: PerceptionLog = serde_json::from_str(&json).unwrap();
        assert_eq!(log.operation, deserialized.operation);
        assert_eq!(log.success, deserialized.success);
    }
    
    #[test]
    fn test_action_log_serialize() {
        let log = ActionLog {
            action_type: "click".to_string(),
            target: Some("button".to_string()),
            coordinate: Some((100.0, 200.0)),
            success: true,
            backend_used: Some("cgevent".to_string()),
            error_message: None,
            duration_ms: 10,
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: ActionLog = serde_json::from_str(&json).unwrap();
        assert_eq!(log.action_type, deserialized.action_type);
        assert_eq!(log.coordinate, deserialized.coordinate);
    }
    
    #[test]
    fn test_cascade_log_serialize() {
        let log = CascadeLog {
            step: "click".to_string(),
            backend: "accessibility".to_string(),
            success: true,
            fallback_triggered: false,
            duration_ms: 15,
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: CascadeLog = serde_json::from_str(&json).unwrap();
        assert_eq!(log.step, deserialized.step);
        assert_eq!(log.fallback_triggered, deserialized.fallback_triggered);
    }
    
    #[test]
    fn test_recipe_log_serialize() {
        let log = RecipeLog {
            operation: "run".to_string(),
            recipe_name: "gmail-compose".to_string(),
            step_count: Some(5),
            params_provided: Some(vec!["recipient".to_string()]),
            success: true,
            error_message: None,
        };

        let json = serde_json::to_string(&log).unwrap();
        let deserialized: RecipeLog = serde_json::from_str(&json).unwrap();
        assert_eq!(log.recipe_name, deserialized.recipe_name);
        assert_eq!(log.step_count, deserialized.step_count);
    }
    
    #[test]
    fn test_log_level_serialize() {
        assert_eq!(serde_json::to_string(&LogLevel::Debug).unwrap(), "\"debug\"");
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(serde_json::to_string(&LogLevel::Error).unwrap(), "\"error\"");
        assert_eq!(serde_json::to_string(&LogLevel::Trace).unwrap(), "\"trace\"");
    }
    
    #[test]
    fn test_log_level_deserialize() {
        let debug: LogLevel = serde_json::from_str("\"debug\"").unwrap();
        assert_eq!(debug, LogLevel::Debug);
        
        let info: LogLevel = serde_json::from_str("\"info\"").unwrap();
        assert_eq!(info, LogLevel::Info);
    }
    
#[test]
    fn test_sootie_logger_creation() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        assert!(logger.config.log_tool_calls);
    }
    
    #[test]
    fn test_sootie_logger_disabled_logs() {
        let config = LogConfig {
            level: LogLevel::Info,
            file_path: None,
            log_tool_calls: false,
            log_perception: false,
            log_actions: false,
            log_cascade: false,
            log_recipes: false,
        };
        let logger = SootieLogger::new(config);
        
        let tool_log = ToolCallLog {
            tool_name: "test".to_string(),
            request_id: None,
            arguments: serde_json::json!({}),
            success: true,
            error_message: None,
            duration_ms: 0,
            backend_used: None,
        };
        
        logger.log_tool_call(&tool_log);
    }
    
    #[test]
    fn test_tool_call_log_error_path() {
        let log = ToolCallLog {
            tool_name: "sootie_click".to_string(),
            request_id: Some(serde_json::Value::Number(1.into())),
            arguments: serde_json::json!({"button": "left"}),
            success: false,
            error_message: Some("Permission denied".to_string()),
            duration_ms: 50,
            backend_used: None,
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("Permission denied"));
        assert!(json.contains("false"));
    }
    
    #[test]
    fn test_perception_log_error_path() {
        let log = PerceptionLog {
            operation: "find".to_string(),
            selector: None,
            success: false,
            result_summary: "".to_string(),
            duration_ms: 10,
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("false"));
    }
    
    #[test]
    fn test_action_log_error_path() {
        let log = ActionLog {
            action_type: "click".to_string(),
            target: None,
            coordinate: None,
            success: false,
            backend_used: None,
            error_message: Some("Failed to click".to_string()),
            duration_ms: 5,
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("Failed to click"));
    }
    
    #[test]
    fn test_cascade_log_fallback_path() {
        let log = CascadeLog {
            step: "click".to_string(),
            backend: "vision".to_string(),
            success: true,
            fallback_triggered: true,
            duration_ms: 150,
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("true"));
        assert!(json.contains("vision"));
    }
    
    #[test]
    fn test_recipe_log_error_path() {
        let log = RecipeLog {
            operation: "run".to_string(),
            recipe_name: "test".to_string(),
            step_count: None,
            params_provided: None,
            success: false,
            error_message: Some("Missing parameter".to_string()),
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("Missing parameter"));
    }
    
    #[test]
    fn test_create_duration_ms() {
        let start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let ms = create_duration_ms(start);
        assert!(ms >= 10);
    }
    
    #[test]
    fn test_log_level_serialize_all() {
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&LogLevel::Debug).unwrap(), "\"debug\"");
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(serde_json::to_string(&LogLevel::Error).unwrap(), "\"error\"");
    }
    
    #[test]
    fn test_sootie_logger_log_session() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_session_start("macos", "0.1.0");
    }

    #[test]
    fn test_log_perception_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = PerceptionLog {
            operation: "find".to_string(),
            selector: Some(serde_json::json!({"role": "button"})),
            success: true,
            result_summary: "Found 3 elements".to_string(),
            duration_ms: 25,
        };
        logger.log_perception(&log);
    }

    #[test]
    fn test_log_action_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = ActionLog {
            action_type: "click".to_string(),
            target: Some("Submit button".to_string()),
            coordinate: Some((100.0, 200.0)),
            success: true,
            backend_used: Some("cgevent".to_string()),
            error_message: None,
            duration_ms: 10,
        };
        logger.log_action(&log);
    }

    #[test]
    fn test_log_cascade_no_fallback() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = CascadeLog {
            step: "click".to_string(),
            backend: "accessibility".to_string(),
            success: true,
            fallback_triggered: false,
            duration_ms: 15,
        };
        logger.log_cascade(&log);
    }

    #[test]
    fn test_log_recipe_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = RecipeLog {
            operation: "run".to_string(),
            recipe_name: "gmail-compose".to_string(),
            step_count: Some(5),
            params_provided: Some(vec!["to".to_string(), "subject".to_string()]),
            success: true,
            error_message: None,
        };
        logger.log_recipe(&log);
    }

    #[test]
    fn test_log_mcp_request() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_mcp_request("tools/list", &Some(serde_json::Value::Number(1.into())));
    }

    #[test]
    fn test_log_mcp_response_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let duration = std::time::Duration::from_millis(50);
        logger.log_mcp_response("tools/list", true, duration);
    }

    #[test]
    fn test_log_mcp_response_error() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let duration = std::time::Duration::from_millis(10);
        logger.log_mcp_response("tools/call", false, duration);
    }

    #[test]
    fn test_log_session_end() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_session_end();
    }

    #[test]
    fn test_log_permission_check_granted() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_permission_check("macos", true);
    }

    #[test]
    fn test_log_permission_check_not_granted() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_permission_check("macos", false);
    }

    #[test]
    fn test_log_platform_init_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_platform_init("macos", true);
    }

    #[test]
    fn test_log_platform_init_failure() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        logger.log_platform_init("macos", false);
    }

    #[test]
    fn test_log_tool_call_success() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = ToolCallLog {
            tool_name: "sootie_click".to_string(),
            request_id: Some(serde_json::Value::Number(1.into())),
            arguments: serde_json::json!({"x": 100, "y": 200}),
            success: true,
            error_message: None,
            duration_ms: 15,
            backend_used: Some("cgevent".to_string()),
        };
        logger.log_tool_call(&log);
    }

    #[test]
    fn test_log_tool_call_failure() {
        let config = LogConfig::default();
        let logger = SootieLogger::new(config);
        let log = ToolCallLog {
            tool_name: "sootie_find".to_string(),
            request_id: Some(serde_json::Value::Number(2.into())),
            arguments: serde_json::json!({"role": "button"}),
            success: false,
            error_message: Some("Element not found".to_string()),
            duration_ms: 30,
            backend_used: None,
        };
        logger.log_tool_call(&log);
    }
}
