use anyhow::Result;
use sootie_core::perception::{ScreenshotData, ScreenshotFormat};
use std::path::PathBuf;

pub struct FixturesLoader;

impl FixturesLoader {
    fn base_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fixtures")
    }

    pub fn load_html_page(name: &str) -> Result<String> {
        let path = Self::base_dir().join("html-pages").join(name);
        std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("Failed to load {}: {}", name, e))
    }

    pub fn load_screenshot(name: &str) -> Result<ScreenshotData> {
        let path = Self::base_dir().join("screenshots").join(name);
        let data =
            std::fs::read(&path).map_err(|e| anyhow::anyhow!("Failed to load {}: {}", name, e))?;

        Ok(ScreenshotData {
            data,
            bounds: None,
            format: ScreenshotFormat::Png,
        })
    }

    pub fn load_expected_result(name: &str) -> Result<serde_json::Value> {
        let path = Self::base_dir().join("expected-results").join(name);
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", name, e))
    }

    pub fn load_config(name: &str) -> Result<serde_json::Value> {
        let path = Self::base_dir().join("configs").join(name);
        let content = std::fs::read_to_string(&path)?;
        serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", name, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_dir_exists() {
        let dir = FixturesLoader::base_dir();
        assert!(dir.exists());
    }
}
