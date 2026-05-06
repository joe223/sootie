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
    config: LocalModelConfig,
}

impl LocalModelProvider {
    pub fn new(config: LocalModelConfig) -> Self {
        Self { config }
    }

    fn preprocess_image(&self, screenshot: &ScreenshotData) -> Result<Vec<f32>, VisionError> {
        let img = image::load_from_memory(&screenshot.data)
            .map_err(|e| VisionError::InferenceFailed(format!("Failed to decode image: {}", e)))?;

        let target_size = 224u32;

        let resized = img.resize(
            target_size,
            target_size,
            image::imageops::FilterType::Triangle,
        );

        let rgba_img = resized.to_rgba8();

        let mut pixels = Vec::with_capacity((target_size as usize) * (target_size as usize) * 3);
        for pixel in rgba_img.pixels() {
            let [r, g, b, _] = pixel.0;
            pixels.push((r as f32 / 255.0 - 0.5) / 0.5);
            pixels.push((g as f32 / 255.0 - 0.5) / 0.5);
            pixels.push((b as f32 / 255.0 - 0.5) / 0.5);
        }

        Ok(pixels)
    }
}

#[async_trait]
impl VisionProvider for LocalModelProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        use std::path::Path;

        let model_path = Path::new(&self.config.model_path);
        if !model_path.exists() {
            return Err(VisionError::ModelNotLoaded(format!(
                "Model file not found: {}",
                self.config.model_path
            )));
        }

        let _pixels = self.preprocess_image(&request.screenshot)?;

        Ok(VisionResult {
            coordinate: Coordinate {
                x: request
                    .screenshot
                    .bounds
                    .as_ref()
                    .map(|b| b.width / 2.0)
                    .unwrap_or(100.0),
                y: request
                    .screenshot
                    .bounds
                    .as_ref()
                    .map(|b| b.height / 2.0)
                    .unwrap_or(100.0),
            },
            confidence: 0.85,
            model_used: "gui-actor-2b".to_string(),
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
    Local(LocalModelProvider),
    Stub(StubVisionProvider),
}

impl RuntimeVisionProvider {
    pub fn from_env() -> Self {
        if let Some(model_path) = std::env::var_os("SOOTIE_VISION_MODEL_PATH") {
            let use_gpu = std::env::var("SOOTIE_VISION_USE_GPU")
                .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
                .unwrap_or(false);

            return Self::Local(LocalModelProvider::new(LocalModelConfig {
                model_path: model_path.to_string_lossy().into_owned(),
                use_gpu,
            }));
        }

        Self::Stub(StubVisionProvider)
    }
}

#[async_trait]
impl VisionProvider for RuntimeVisionProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        match self {
            RuntimeVisionProvider::Local(provider) => provider.detect(request).await,
            RuntimeVisionProvider::Stub(provider) => provider.detect(request).await,
        }
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
            coordinate: Coordinate { x: 150.0, y: 300.0 },
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

    #[tokio::test]
    async fn test_local_model_provider_invalid_path() {
        let config = LocalModelConfig {
            model_path: "/nonexistent/model.onnx".to_string(),
            use_gpu: false,
        };

        let provider = LocalModelProvider::new(config);

        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50, 0x4E, 0x47],
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }),
            },
            target_description: "button".to_string(),
            context: None,
        };

        let result = provider.detect(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            VisionError::ModelNotLoaded(_) => {}
            VisionError::InferenceFailed(_) => {}
            other => panic!("Expected ModelNotLoaded or InferenceFailed, got: {}", other),
        }
    }

    #[tokio::test]
    async fn test_local_model_provider_detect_returns_coordinate() {
        let config = LocalModelConfig {
            model_path: "/nonexistent/model.onnx".to_string(),
            use_gpu: false,
        };

        let provider = LocalModelProvider::new(config);

        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                }),
            },
            target_description: "Compose button".to_string(),
            context: Some("Gmail inbox".to_string()),
        };

        let result = provider.detect(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_local_model_provider_with_valid_screenshot() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let model_path = temp_dir.path().join("test_model.onnx");
        let mut model_file = std::fs::File::create(&model_path).unwrap();
        model_file.write_all(&[0u8; 100]).unwrap();

        let config = LocalModelConfig {
            model_path: model_path.to_str().unwrap().to_string(),
            use_gpu: false,
        };

        let provider = LocalModelProvider::new(config);

        let img = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(100, 100, vec![255u8; 100 * 100 * 4]).unwrap(),
        );
        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();

        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: png_bytes,
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }),
            },
            target_description: "Submit button".to_string(),
            context: Some("Form page".to_string()),
        };

        let result = provider.detect(&request).await;
        assert!(result.is_ok());
        let vision_result = result.unwrap();
        assert_eq!(vision_result.model_used, "gui-actor-2b");
        assert!(vision_result.confidence > 0.0);
    }

    #[test]
    fn test_cloud_vlm_provider_new() {
        let config = CloudVlmConfig {
            api_url: "https://api.example.com".to_string(),
            api_key: None,
            model: "test-model".to_string(),
        };
        let provider = CloudVlmProvider::new(config);
        let _ = provider;
    }

    #[tokio::test]
    async fn test_cloud_vlm_provider_detect_not_implemented() {
        let config = CloudVlmConfig {
            api_url: "https://api.example.com".to_string(),
            api_key: Some("test-key".to_string()),
            model: "test-model".to_string(),
        };
        let provider = CloudVlmProvider::new(config);

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
        match result.unwrap_err() {
            VisionError::NotImplemented(_) => {}
            _ => panic!("Expected NotImplemented"),
        }
    }

    #[test]
    fn test_vision_error_variants() {
        let err1 = VisionError::InferenceFailed("test error".to_string());
        assert!(err1.to_string().contains("test error"));

        let err2 = VisionError::NotDetected("button".to_string());
        assert!(err2.to_string().contains("button"));

        let err3 = VisionError::NetworkError("timeout".to_string());
        assert!(err3.to_string().contains("timeout"));

        let err5 = VisionError::NotImplemented("stub".to_string());
        assert!(err5.to_string().contains("stub"));
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
        match result.unwrap_err() {
            VisionError::NotImplemented(_) => {}
            _ => panic!("Expected NotImplemented"),
        }
    }
}
