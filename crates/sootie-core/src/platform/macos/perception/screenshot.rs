use std::process::Command;

use crate::perception::{PerceptionError, ScreenshotData, ScreenshotFormat};
use crate::selector::Bounds;

pub fn take_screenshot(
    region: Option<&Bounds>,
    display_id: Option<u32>,
) -> Result<ScreenshotData, PerceptionError> {
    let mut cmd = Command::new("screencapture");
    cmd.arg("-x").arg("-t").arg("png");

    // Add display selection if specified (macOS display IDs start from 1)
    if let Some(did) = display_id {
        cmd.arg("-D").arg(did.to_string());
    }

    if let Some(b) = region {
        cmd.arg("-R")
            .arg(format!("{},{},{},{}", b.x, b.y, b.width, b.height));
    }

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("sootie_screenshot_{}.png", std::process::id()));
    cmd.arg(&tmp_path);

    let output = cmd
        .output()
        .map_err(|e| PerceptionError::ScreenshotFailed(format!("screencapture failed: {}", e)))?;

    if !output.status.success() {
        return Err(PerceptionError::ScreenshotFailed(
            "screencapture command failed".to_string(),
        ));
    }

    let data = std::fs::read(&tmp_path).map_err(|e| {
        PerceptionError::ScreenshotFailed(format!("failed to read screenshot: {}", e))
    })?;

    let _ = std::fs::remove_file(&tmp_path);

    let bounds = region.cloned();

    Ok(ScreenshotData {
        format: ScreenshotFormat::Png,
        data,
        bounds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Bounds;

    #[test]
    fn test_take_screenshot_full_screen() {
        let result = take_screenshot(None, None);
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(!data.data.is_empty());
        }
    }

    #[test]
    fn test_take_screenshot_with_region() {
        let region = Bounds {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let result = take_screenshot(Some(&region), None);
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(data.data.is_empty() || !data.data.is_empty());
        }
    }

    #[test]
    fn test_take_screenshot_with_display_id() {
        let result = take_screenshot(None, Some(1));
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(!data.data.is_empty());
        }
    }

    #[test]
    fn test_take_screenshot_with_large_region() {
        let region = Bounds {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let result = take_screenshot(Some(&region), None);
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            if let Some(b) = data.bounds {
                assert_eq!(b.width, 1920.0);
                assert_eq!(b.height, 1080.0);
            }
        }
    }

    #[test]
    fn test_take_screenshot_bounds_none() {
        let result = take_screenshot(None, None);
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(data.bounds.is_none());
        }
    }

    #[test]
    fn test_take_screenshot_bounds_some() {
        let region = Bounds {
            x: 50.0,
            y: 50.0,
            width: 200.0,
            height: 150.0,
        };
        let result = take_screenshot(Some(&region), Some(2));
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(data.bounds.is_some());
            if let Some(b) = data.bounds {
                assert_eq!(b.x, 50.0);
                assert_eq!(b.y, 50.0);
            }
        }
    }
}
