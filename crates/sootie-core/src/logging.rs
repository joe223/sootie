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
    pub sanitize_logs: bool,
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
            sanitize_logs: true,
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

    pub fn config(&self) -> &LogConfig {
        &self.config
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

/// Default sensitive field names (lowercase for matching)
const DEFAULT_SENSITIVE_FIELDS: &[&str] = &[
    "password",
    "pwd",
    "pass",
    "api_key",
    "apikey",
    "key",
    "token",
    "secret",
    "credential",
    "auth",
    "private",
    "authorization",
    "bearer",
];

/// Sensitive field patterns from environment variable
fn get_sensitive_fields_from_env() -> Vec<String> {
    std::env::var("SOOTIE_SENSITIVE_FIELDS")
        .ok()
        .and_then(|val| serde_json::from_str::<Vec<String>>(&val).ok())
        .unwrap_or_default()
}

/// Combined list of sensitive field patterns
fn get_sensitive_fields() -> Vec<String> {
    let defaults: Vec<String> = DEFAULT_SENSITIVE_FIELDS
        .iter()
        .map(|s| s.to_string())
        .collect();
    let env_override = get_sensitive_fields_from_env();

    if env_override.is_empty() {
        defaults
    } else {
        env_override
    }
}

/// Check if field name is sensitive (case-insensitive)
fn is_sensitive_field(field_name: &str) -> bool {
    let field_lower = field_name.to_lowercase();
    get_sensitive_fields()
        .iter()
        .any(|s| field_lower.contains(&s.to_lowercase()))
}

/// Sanitize email: "user@domain.com" → "u***@domain.com"
fn sanitize_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        if at_pos > 0 {
            let domain = &email[at_pos..];
            return format!("u***{}", domain);
        }
    }
    "[REDACTED:email]".to_string()
}

/// Sanitize credit card: "1234-5678-9012-3456" → "[REDACTED:cc]"
fn sanitize_credit_card(text: &str) -> Option<String> {
    let cc_pattern = regex::Regex::new(r"\b\d{4}[-\s]?\d{4}[-\s]?\d{4}[-\s]?\d{4}\b").unwrap();
    if cc_pattern.is_match(text) {
        return Some("[REDACTED:cc]".to_string());
    }
    None
}

/// Sanitize SSN: "123-45-6789" → "[REDACTED:ssn]"
fn sanitize_ssn(text: &str) -> Option<String> {
    let ssn_pattern = regex::Regex::new(r"\b\d{3}[-\s]?\d{2}[-\s]?\d{4}\b").unwrap();
    if ssn_pattern.is_match(text) {
        return Some("[REDACTED:ssn]".to_string());
    }
    None
}

/// Sanitize a string value based on patterns
fn sanitize_string_value(value: &str) -> String {
    if let Some(cc_redacted) = sanitize_credit_card(value) {
        return cc_redacted;
    }
    if let Some(ssn_redacted) = sanitize_ssn(value) {
        return ssn_redacted;
    }

    let email_pattern =
        regex::Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Z|a-z]{2,}\b").unwrap();
    if email_pattern.is_match(value) {
        return sanitize_email(value);
    }

    value.to_string()
}

/// Recursive JSON sanitization
pub fn sanitize_json_value(
    value: serde_json::Value,
    field_name: Option<&str>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            if let Some(name) = field_name {
                if is_sensitive_field(name) {
                    return serde_json::Value::String(format!(
                        "[REDACTED:{}]",
                        name.to_lowercase()
                    ));
                }
            }
            serde_json::Value::String(sanitize_string_value(&s))
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(|item| sanitize_json_value(item, None))
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let sanitized_value = sanitize_json_value(value, Some(&key));
                    (key, sanitized_value)
                })
                .collect(),
        ),
        other => other,
    }
}

/// Sanitize ToolCallLog arguments
pub fn sanitize_tool_call_args(args: &serde_json::Value, config: &LogConfig) -> serde_json::Value {
    if config.sanitize_logs {
        sanitize_json_value(args.clone(), None)
    } else {
        args.clone()
    }
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
        assert_eq!(
            serde_json::to_string(&LogLevel::Debug).unwrap(),
            "\"debug\""
        );
        assert_eq!(serde_json::to_string(&LogLevel::Info).unwrap(), "\"info\"");
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            "\"error\""
        );
        assert_eq!(
            serde_json::to_string(&LogLevel::Trace).unwrap(),
            "\"trace\""
        );
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
            sanitize_logs: true,
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
        assert_eq!(
            serde_json::to_string(&LogLevel::Debug).unwrap(),
            "\"debug\""
        );
        assert_eq!(serde_json::to_string(&LogLevel::Warn).unwrap(), "\"warn\"");
        assert_eq!(
            serde_json::to_string(&LogLevel::Error).unwrap(),
            "\"error\""
        );
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
            tool_name: "sootie_find_element".to_string(),
            request_id: Some(serde_json::Value::Number(2.into())),
            arguments: serde_json::json!({"role": "button"}),
            success: false,
            error_message: Some("Element not found".to_string()),
            duration_ms: 30,
            backend_used: None,
        };
        logger.log_tool_call(&log);
    }

    mod sanitization_tests {
        use super::*;

        #[test]
        fn test_is_sensitive_field_password() {
            assert!(is_sensitive_field("password"));
            assert!(is_sensitive_field("PASSWORD"));
            assert!(is_sensitive_field("user_password"));
        }

        #[test]
        fn test_is_sensitive_field_api_key() {
            assert!(is_sensitive_field("api_key"));
            assert!(is_sensitive_field("apikey"));
            assert!(is_sensitive_field("API_KEY"));
            assert!(is_sensitive_field("x-api-key"));
        }

        #[test]
        fn test_is_sensitive_field_token() {
            assert!(is_sensitive_field("token"));
            assert!(is_sensitive_field("access_token"));
            assert!(is_sensitive_field("auth_token"));
        }

        #[test]
        fn test_is_not_sensitive_field() {
            assert!(!is_sensitive_field("username"));
            assert!(!is_sensitive_field("email"));
            assert!(!is_sensitive_field("name"));
            assert!(!is_sensitive_field("count"));
        }

        #[test]
        fn test_sanitize_email() {
            assert_eq!(sanitize_email("user@domain.com"), "u***@domain.com");
            assert_eq!(sanitize_email("admin@company.org"), "u***@company.org");
            assert_eq!(sanitize_email("test@localhost"), "u***@localhost");
        }

        #[test]
        fn test_sanitize_credit_card() {
            assert_eq!(
                sanitize_credit_card("1234-5678-9012-3456"),
                Some("[REDACTED:cc]".to_string())
            );
            assert_eq!(
                sanitize_credit_card("1234567890123456"),
                Some("[REDACTED:cc]".to_string())
            );
            assert_eq!(sanitize_credit_card("not a card"), None);
        }

        #[test]
        fn test_sanitize_ssn() {
            assert_eq!(
                sanitize_ssn("123-45-6789"),
                Some("[REDACTED:ssn]".to_string())
            );
            assert_eq!(
                sanitize_ssn("123456789"),
                Some("[REDACTED:ssn]".to_string())
            );
            assert_eq!(sanitize_ssn("not an ssn"), None);
        }

        #[test]
        fn test_sanitize_json_simple_object() {
            let input = serde_json::json!({
                "username": "john_doe",
                "password": "secret123"
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["username"], "john_doe");
            assert_eq!(output["password"], "[REDACTED:password]");
        }

        #[test]
        fn test_sanitize_json_nested_object() {
            let input = serde_json::json!({
                "user": {
                    "name": "Alice",
                    "email": "alice@example.com",
                    "credentials": {
                        "api_key": "sk-1234567890"
                    }
                }
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["user"]["name"], "Alice");
            assert_eq!(output["user"]["email"], "u***@example.com");
            assert_eq!(
                output["user"]["credentials"]["api_key"],
                "[REDACTED:api_key]"
            );
        }

        #[test]
        fn test_sanitize_json_array() {
            let input = serde_json::json!([
                {"name": "Item1", "token": "abc123"},
                {"name": "Item2", "token": "def456"}
            ]);
            let output = sanitize_json_value(input, None);
            assert_eq!(output[0]["name"], "Item1");
            assert_eq!(output[0]["token"], "[REDACTED:token]");
            assert_eq!(output[1]["token"], "[REDACTED:token]");
        }

        #[test]
        fn test_sanitize_json_preserves_numbers_and_bools() {
            let input = serde_json::json!({
                "count": 42,
                "enabled": true,
                "ratio": 2.5,
                "password": "secret"
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["count"], 42);
            assert_eq!(output["enabled"], true);
            assert_eq!(output["ratio"], 2.5);
            assert_eq!(output["password"], "[REDACTED:password]");
        }

        #[test]
        fn test_sanitize_json_credit_card_in_text() {
            let input = serde_json::json!({
                "notes": "Card number: 1234-5678-9012-3456"
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["notes"], "[REDACTED:cc]");
        }

        #[test]
        fn test_sanitize_json_ssn_in_text() {
            let input = serde_json::json!({
                "document": "SSN: 123-45-6789"
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["document"], "[REDACTED:ssn]");
        }

        #[test]
        fn test_sanitize_tool_call_args_enabled() {
            let config = LogConfig {
                sanitize_logs: true,
                ..Default::default()
            };
            let args = serde_json::json!({
                "username": "alice",
                "password": "secret"
            });
            let sanitized = sanitize_tool_call_args(&args, &config);
            assert_eq!(sanitized["username"], "alice");
            assert_eq!(sanitized["password"], "[REDACTED:password]");
        }

        #[test]
        fn test_sanitize_tool_call_args_disabled() {
            let config = LogConfig {
                sanitize_logs: false,
                ..Default::default()
            };
            let args = serde_json::json!({
                "username": "alice",
                "password": "secret"
            });
            let sanitized = sanitize_tool_call_args(&args, &config);
            assert_eq!(sanitized["username"], "alice");
            assert_eq!(sanitized["password"], "secret");
        }

        #[test]
        fn test_sanitize_deeply_nested() {
            let input = serde_json::json!({
                "level1": {
                    "level2": {
                        "level3": {
                            "secret_token": "hidden_value",
                            "public_data": "visible"
                        }
                    }
                }
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(
                output["level1"]["level2"]["level3"]["secret_token"],
                "[REDACTED:secret_token]"
            );
            assert_eq!(
                output["level1"]["level2"]["level3"]["public_data"],
                "visible"
            );
        }

        #[test]
        fn test_sanitize_mixed_types() {
            let input = serde_json::json!({
                "credentials": [
                    {"type": "api_key", "secret": "sk-abc123"},
                    {"type": "oauth_token", "token": "oauth-xyz789"}
                ],
                "metadata": {
                    "count": 5,
                    "enabled": true
                }
            });
            let output = sanitize_json_value(input, None);
            assert_eq!(output["credentials"][0]["secret"], "[REDACTED:secret]");
            assert_eq!(output["credentials"][1]["token"], "[REDACTED:token]");
            assert_eq!(output["metadata"]["count"], 5);
            assert_eq!(output["metadata"]["enabled"], true);
        }

        #[test]
        fn test_log_config_sanitize_flag() {
            let config = LogConfig::default();
            assert!(config.sanitize_logs);
        }
    }
}
