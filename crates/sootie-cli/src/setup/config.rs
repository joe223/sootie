use std::path::PathBuf;
use std::fs;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FallbackBackend {
    #[serde(rename = "cdp")]
    Cdp,
    #[serde(rename = "at_tree")]
    AtTree,
    #[serde(rename = "vision")]
    Vision,
}

impl std::fmt::Display for FallbackBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FallbackBackend::Cdp => write!(f, "cdp"),
            FallbackBackend::AtTree => write!(f, "at_tree"),
            FallbackBackend::Vision => write!(f, "vision"),
        }
    }
}

impl Default for FallbackBackend {
    fn default() -> Self {
        FallbackBackend::Cdp
    }
}

fn default_fallback_priority() -> Vec<FallbackBackend> {
    vec![FallbackBackend::Cdp, FallbackBackend::AtTree, FallbackBackend::Vision]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    #[serde(default)]
    pub model_path: String,
    #[serde(default = "default_model_name")]
    pub model_name: String,
    #[serde(default = "default_sidecar_port")]
    pub sidecar_port: u16,
    #[serde(default)]
    pub use_gpu: bool,
    #[serde(default = "default_auto_start")]
    pub auto_start: bool,
}

fn default_auto_start() -> bool {
    false
}

fn default_model_name() -> String {
    "showui-2b".to_string()
}

fn default_sidecar_port() -> u16 {
    9876
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpConfig {
    #[serde(default = "default_cdp_host")]
    pub host: String,
    #[serde(default = "default_cdp_port")]
    pub port: u16,
}

fn default_cdp_host() -> String {
    "127.0.0.1".to_string()
}

fn default_cdp_port() -> u16 {
    9222
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_sanitize_logs")]
    pub sanitize_logs: bool,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_sanitize_logs() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    #[serde(default = "default_fallback_priority")]
    pub priority: Vec<FallbackBackend>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SootieConfig {
    #[serde(default = "default_fallback_config")]
    pub fallback: FallbackConfig,
    #[serde(default = "default_vision_config")]
    pub vision: VisionConfig,
    #[serde(default = "default_cdp_config")]
    pub cdp: CdpConfig,
    #[serde(default = "default_logging_config")]
    pub logging: LoggingConfig,
}

fn default_fallback_config() -> FallbackConfig {
    FallbackConfig {
        priority: default_fallback_priority(),
    }
}

fn default_vision_config() -> VisionConfig {
    VisionConfig {
        model_path: String::new(),
        model_name: default_model_name(),
        sidecar_port: default_sidecar_port(),
        use_gpu: false,
        auto_start: default_auto_start(),
    }
}

fn default_cdp_config() -> CdpConfig {
    CdpConfig {
        host: default_cdp_host(),
        port: default_cdp_port(),
    }
}

fn default_logging_config() -> LoggingConfig {
    LoggingConfig {
        level: default_log_level(),
        sanitize_logs: default_sanitize_logs(),
    }
}

impl Default for SootieConfig {
    fn default() -> Self {
        Self {
            fallback: default_fallback_config(),
            vision: default_vision_config(),
            cdp: default_cdp_config(),
            logging: default_logging_config(),
        }
    }
}

pub fn home_config_dir() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join("sootie")
}

pub fn config_dir() -> PathBuf {
    home_config_dir()
}

pub fn config_file_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn model_dir() -> PathBuf {
    dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("sootie")
        .join("models")
}

pub fn showui_model_path() -> PathBuf {
    model_dir().join("ShowUI-2B")
}

pub fn load_config() -> Result<SootieConfig> {
    let path = config_file_path();
    if !path.exists() {
        return Ok(SootieConfig::default());
    }
    let content = fs::read_to_string(&path)?;
    let config: SootieConfig = toml::from_str(&content)?;
    Ok(config)
}

pub fn save_config(config: &SootieConfig) -> Result<()> {
    let dir = config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    let path = config_file_path();
    let content = toml::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn generate_default_config() -> Result<PathBuf> {
    let path = config_file_path();
    if path.exists() {
        return Ok(path);
    }
    let config = SootieConfig::default();
    save_config(&config)?;
    Ok(path)
}

pub fn resolve_vision_model_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("SOOTIE_VISION_MODEL_PATH") {
        let pb = PathBuf::from(path);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(config) = load_config() {
        if !config.vision.model_path.is_empty() {
            let pb = PathBuf::from(&config.vision.model_path);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    let default_path = showui_model_path();
    if default_path.exists() {
        return Some(default_path);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_backend_serialize() {
        let backend = FallbackBackend::Cdp;
        let json = serde_json::to_string(&backend).unwrap();
        assert_eq!(json, "\"cdp\"");
    }

    #[test]
    fn test_fallback_backend_deserialize() {
        let backend: FallbackBackend = serde_json::from_str("\"at_tree\"").unwrap();
        assert_eq!(backend, FallbackBackend::AtTree);
    }

    #[test]
    fn test_fallback_priority_default() {
        let priority = default_fallback_priority();
        assert_eq!(priority.len(), 3);
        assert_eq!(priority[0], FallbackBackend::Cdp);
        assert_eq!(priority[1], FallbackBackend::AtTree);
        assert_eq!(priority[2], FallbackBackend::Vision);
    }

    #[test]
    fn test_default_config_serialize() {
        let mut config = SootieConfig::default();
        config.vision.model_name = "showui-2b".to_string();
        config.vision.sidecar_port = 9876;
        config.cdp.port = 9222;
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[fallback]"));
        assert!(toml_str.contains("[vision]"));
        assert!(toml_str.contains("[cdp]"));
        assert!(toml_str.contains("priority"));
    }

    #[test]
    fn test_config_with_custom_priority() {
        let toml_str = "
[fallback]
priority = [\"vision\", \"at_tree\", \"cdp\"]
";
        let config: SootieConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.fallback.priority.len(), 3);
        assert_eq!(config.fallback.priority[0], FallbackBackend::Vision);
        assert_eq!(config.fallback.priority[1], FallbackBackend::AtTree);
        assert_eq!(config.fallback.priority[2], FallbackBackend::Cdp);
    }

    #[test]
    fn test_config_dir_is_home_config() {
        let dir = config_dir();
        assert!(dir.to_string_lossy().contains(".config"));
        assert!(dir.ends_with("sootie"));
    }

    #[test]
    fn test_model_dir_path() {
        let dir = model_dir();
        let path_str = dir.to_string_lossy();
        assert!(path_str.contains(".local"));
        assert!(path_str.contains("share"));
        assert!(path_str.contains("sootie"));
        assert!(path_str.contains("models"));
        assert!(!path_str.contains("Library"));
    }
}