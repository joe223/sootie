use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::perception::ScreenshotData;
use crate::selector::Coordinate;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionRequest {
    pub screenshot: ScreenshotData,
    pub target_description: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisionResult {
    pub coordinate: Coordinate,
    pub confidence: f64,
    pub model_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudVlmConfig {
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelConfig {
    pub model_path: String,
    pub use_gpu: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("model inference failed: {0}")]
    InferenceFailed(String),

    #[error("element not detected: {0}")]
    NotDetected(String),

    #[error("low confidence: {confidence:.2} for '{target}'")]
    LowConfidence { target: String, confidence: f64 },

    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[async_trait]
pub trait VisionProvider: Send + Sync {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError>;
}

pub struct CloudVlmProvider {
    _config: CloudVlmConfig,
}

impl CloudVlmProvider {
    pub fn new(config: CloudVlmConfig) -> Self {
        Self { _config: config }
    }
}

#[async_trait]
impl VisionProvider for CloudVlmProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented(
            "cloud VLM provider not yet implemented".to_string(),
        ))
    }
}

pub struct LocalModelProvider {
    _config: LocalModelConfig,
}

impl LocalModelProvider {
    pub fn new(config: LocalModelConfig) -> Self {
        Self { _config: config }
    }
}

#[async_trait]
impl VisionProvider for LocalModelProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented(
            "local ONNX model provider not yet implemented".to_string(),
        ))
    }
}

pub struct StubVisionProvider;

#[async_trait]
impl VisionProvider for StubVisionProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented("stub provider".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::ScreenshotFormat;
    use crate::selector::Bounds;

    #[test]
    fn test_vision_request_serialize() {
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50],
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }),
            },
            target_description: "Compose button".to_string(),
            context: Some("Gmail inbox".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Compose button"));
        assert!(json.contains("Gmail inbox"));
    }

    #[test]
    fn test_vision_result_serialize() {
        let result = VisionResult {
            coordinate: Coordinate {
                x: 150.0,
                y: 300.0,
            },
            confidence: 0.95,
            model_used: "gui-actor-2b".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: VisionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_cloud_vlm_config() {
        let config = CloudVlmConfig {
            api_url: "https://api.example.com/vision".to_string(),
            api_key: Some("key123".to_string()),
            model: "gui-actor-2b".to_string(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: CloudVlmConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.api_url, "https://api.example.com/vision");
    }

    #[test]
    fn test_local_model_config() {
        let config = LocalModelConfig {
            model_path: "/models/gui-actor-2b.onnx".to_string(),
            use_gpu: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("gui-actor-2b.onnx"));
        assert!(json.contains("true"));
    }

    #[test]
    fn test_vision_error_display() {
        let err = VisionError::LowConfidence {
            target: "Submit".to_string(),
            confidence: 0.3,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("0.30"));
        assert!(msg.contains("Submit"));
    }
}
