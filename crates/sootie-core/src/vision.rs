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

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SidecarRequest {
    image: String,
    description: String,
    screen_width: f64,
    screen_height: f64,
}

pub struct SidecarVisionProvider {
    base_url: String,
    client: reqwest::Client,
}

impl SidecarVisionProvider {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn health_check(&self) -> Result<bool, VisionError> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

#[async_trait]
impl VisionProvider for SidecarVisionProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        let img_b64 = base64_encode(&request.screenshot.data);

        let screen_w = request
            .screenshot
            .bounds
            .as_ref()
            .map(|b| b.width)
            .unwrap_or(1920.0);
        let screen_h = request
            .screenshot
            .bounds
            .as_ref()
            .map(|b| b.height)
            .unwrap_or(1080.0);

        let body = SidecarRequest {
            image: img_b64,
            description: request.target_description.clone(),
            screen_width: screen_w,
            screen_height: screen_h,
        };

        let url = format!("{}/ground", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| VisionError::NetworkError(format!("Sidecar unreachable on {}: {}", self.base_url, e)))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(VisionError::InferenceFailed(format!(
                "Sidecar returned {}: {}",
                status, body
            )));
        }

        let result: SidecarResponse = resp
            .json()
            .await
            .map_err(|e| VisionError::InferenceFailed(format!("Failed to parse sidecar response: {}", e)))?;

        if let Some(err) = result.error {
            return Err(VisionError::InferenceFailed(err));
        }

        Ok(VisionResult {
            coordinate: Coordinate {
                x: result.x,
                y: result.y,
            },
            confidence: result.confidence,
            model_used: "showui-2b".to_string(),
        })
    }
}

pub struct StubVisionProvider;

#[async_trait]
impl VisionProvider for StubVisionProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented("stub provider".to_string()))
    }
}

pub enum RuntimeVisionProvider {
    Sidecar(SidecarVisionProvider),
    Stub(StubVisionProvider),
}

impl RuntimeVisionProvider {
    pub fn from_env() -> Self {
        if let Some(model_path) = std::env::var_os("SOOTIE_VISION_MODEL_PATH") {
            if std::path::Path::new(&model_path).exists() {
                let port = std::env::var("SOOTIE_SIDECAR_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(9876);
                return Self::Sidecar(SidecarVisionProvider::new(port));
            }
        }
        Self::Stub(StubVisionProvider)
    }
}

#[async_trait]
impl VisionProvider for RuntimeVisionProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        match self {
            RuntimeVisionProvider::Sidecar(provider) => provider.detect(request).await,
            RuntimeVisionProvider::Stub(provider) => provider.detect(request).await,
        }
    }
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(char::from(TABLE[((triple >> 18) & 0x3F) as usize]));
        result.push(char::from(TABLE[((triple >> 12) & 0x3F) as usize]));

        if chunk.len() > 1 {
            result.push(char::from(TABLE[((triple >> 6) & 0x3F) as usize]));
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(char::from(TABLE[(triple & 0x3F) as usize]));
        } else {
            result.push('=');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::ScreenshotFormat;
    use crate::selector::Bounds;

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(&[]), "");
    }

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

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
    }

    #[test]
    fn test_vision_result_serialize() {
        let result = VisionResult {
            coordinate: Coordinate { x: 150.0, y: 300.0 },
            confidence: 0.95,
            model_used: "showui-2b".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: VisionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_sidecar_provider_url() {
        let provider = SidecarVisionProvider::new(9876);
        assert!(provider.base_url.contains("9876"));
    }

    #[test]
    fn test_sidecar_response_deserialize() {
        let json = r#"{"x": 150.0, "y": 300.0, "confidence": 0.95}"#;
        let resp: SidecarResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.x, 150.0);
        assert_eq!(resp.y, 300.0);
        assert_eq!(resp.confidence, 0.95);
    }

    #[test]
    fn test_sidecar_response_with_error() {
        let json = r#"{"x": 0.0, "y": 0.0, "confidence": 0.0, "error": "model not loaded"}"#;
        let resp: SidecarResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error, Some("model not loaded".to_string()));
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

    #[test]
    fn test_vision_error_network() {
        let err = VisionError::NetworkError("timeout".to_string());
        assert!(err.to_string().contains("network error"));
    }

    #[tokio::test]
    async fn test_stub_vision_provider_returns_not_implemented() {
        let provider = StubVisionProvider;
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![],
                bounds: None,
            },
            target_description: "test".to_string(),
            context: None,
        };
        let result = provider.detect(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_runtime_from_env_no_model_path() {
        let previous = std::env::var_os("SOOTIE_VISION_MODEL_PATH");
        if let Some(_path) = previous {
            std::env::remove_var("SOOTIE_VISION_MODEL_PATH");
        }
        let provider = RuntimeVisionProvider::from_env();
        match provider {
            RuntimeVisionProvider::Stub(_) => {}
            _ => panic!("expected Stub variant"),
        }
    }

    #[tokio::test]
    async fn test_runtime_from_env_with_valid_path() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let model_path = temp_dir.path().join("ShowUI-2B");
        std::fs::create_dir_all(&model_path).unwrap();
        let mut f = std::fs::File::create(model_path.join("model.safetensors")).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        std::fs::File::create(model_path.join("config.json")).unwrap();

        let previous = std::env::var_os("SOOTIE_VISION_MODEL_PATH");
        std::env::set_var("SOOTIE_VISION_MODEL_PATH", model_path.to_str().unwrap());
        std::env::set_var("SOOTIE_SIDECAR_PORT", "9876");

        let provider = RuntimeVisionProvider::from_env();
        match provider {
            RuntimeVisionProvider::Sidecar(ref p) => {
                assert!(p.base_url.contains("9876"));
            }
            _ => panic!("expected Sidecar variant with valid model path"),
        }

        if let Some(path) = previous {
            std::env::set_var("SOOTIE_VISION_MODEL_PATH", path);
        } else {
            std::env::remove_var("SOOTIE_VISION_MODEL_PATH");
        }
        std::env::remove_var("SOOTIE_SIDECAR_PORT");
    }
}
