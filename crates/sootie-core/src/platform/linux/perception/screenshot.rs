use std::process::Command;

use crate::perception::{PerceptionError, ScreenshotData, ScreenshotFormat};
use crate::selector::Bounds;

pub fn take_screenshot(
    region: Option<&Bounds>,
    display_id: Option<u32>,
) -> Result<ScreenshotData, PerceptionError> {
    let tmp_dir = std::env::temp_dir();
    let timestamp = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S");
    let tmp_path = tmp_dir.join(format!("sootie_screenshot_{}.png", timestamp));

    if display_id.is_some() {
        return Err(PerceptionError::ScreenshotFailed(
            "display-specific screenshots are not supported by the Linux X11 fallback".to_string(),
        ));
    }

    let mut cmd = Command::new("import");
    cmd.arg("-window").arg("root");

    if let Some(b) = region {
        cmd.arg("-crop")
            .arg(format!("{}x{}+{}+{}", b.width, b.height, b.x, b.y));
    }

    cmd.arg(&tmp_path);

    let output = cmd
        .output()
        .map_err(|e| PerceptionError::ScreenshotFailed(format!("import command failed: {}", e)))?;

    if !output.status.success() {
        return Err(PerceptionError::ScreenshotFailed(
            "import command failed (ImageMagick not installed?)".to_string(),
        ));
    }

    let data = std::fs::read(&tmp_path).map_err(|e| {
        PerceptionError::ScreenshotFailed(format!("failed to read screenshot: {}", e))
    })?;

    let _ = std::fs::remove_file(&tmp_path);

    Ok(ScreenshotData {
        format: ScreenshotFormat::Png,
        data,
        bounds: region.cloned(),
    })
}
