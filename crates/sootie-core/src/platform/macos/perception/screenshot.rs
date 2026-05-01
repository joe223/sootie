use std::process::Command;

use crate::perception::{PerceptionError, ScreenshotData, ScreenshotFormat};
use crate::selector::Bounds;

pub fn take_screenshot(region: Option<&Bounds>) -> Result<ScreenshotData, PerceptionError> {
    let mut cmd = Command::new("screencapture");
    cmd.arg("-x").arg("-t").arg("png");

    if let Some(b) = region {
        cmd.arg("-R")
            .arg(format!("{},{},{},{}", b.x, b.y, b.width, b.height));
    }

    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("sootie_screenshot_{}.png", std::process::id()));
    cmd.arg(&tmp_path);

    let output = cmd.output().map_err(|e| {
        PerceptionError::ScreenshotFailed(format!("screencapture failed: {}", e))
    })?;

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