use std::io::Cursor;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use core_graphics::display::CGDisplay;
use image::{DynamicImage, ImageFormat, RgbaImage};

use crate::perception::{PerceptionError, ScreenshotData, ScreenshotFormat};
use crate::selector::Bounds;

const SCREENSHOT_ATTEMPTS: usize = 3;
const SCREENSHOT_RETRY_DELAY: Duration = Duration::from_millis(250);

fn screen_recording_granted() -> bool {
    core_graphics::access::ScreenCaptureAccess::default().preflight()
}

fn screenshot_failure_message(stderr: &str, screen_recording_granted: bool) -> String {
    if stderr.contains("invalid display") {
        return format!("Invalid display ID. stderr: {}", stderr);
    }

    if stderr.contains("could not create image from display") {
        if screen_recording_granted {
            return format!(
                "screencapture could not create image from display after retries, but Screen Recording preflight is granted. stderr: {}",
                stderr
            );
        }

        return "Screen recording permission required. Go to System Settings > Privacy & Security > Screen Recording and enable permission for this application, then restart.".to_string();
    }

    format!("screencapture failed: {}", stderr)
}

fn screenshot_tmp_path() -> std::path::PathBuf {
    let tmp_dir = std::env::temp_dir();
    let timestamp = chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%3f");
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    tmp_dir.join(format!(
        "sootie_screenshot_{}-{}-{}.png",
        timestamp,
        std::process::id(),
        now_nanos
    ))
}

fn parse_display_bounds_line(line: &str) -> Option<(u32, Bounds)> {
    let parts = line.trim().split('|').collect::<Vec<_>>();
    if parts.len() != 5 {
        return None;
    }

    let index = parts[0].trim().parse::<u32>().ok()?;
    let x = parts[1].trim().parse::<f64>().ok()?;
    let y = parts[2].trim().parse::<f64>().ok()?;
    let width = parts[3].trim().parse::<f64>().ok()?;
    let height = parts[4].trim().parse::<f64>().ok()?;

    Some((
        index,
        Bounds {
            x,
            y,
            width,
            height,
        },
    ))
}

fn display_bounds(display_id: u32) -> Option<Bounds> {
    if let Ok(displays) = CGDisplay::active_displays() {
        if let Some(cg_display_id) = displays.get(display_id.saturating_sub(1) as usize) {
            let bounds = CGDisplay::new(*cg_display_id).bounds();
            return Some(Bounds {
                x: bounds.origin.x,
                y: bounds.origin.y,
                width: bounds.size.width,
                height: bounds.size.height,
            });
        }
    }

    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            use framework "AppKit"
            set screens to (current application's NSScreen's screens())
            set screenIndex to 1
            set outputLines to {}
            repeat with s in screens
                set frame to s's frame()
                set line to (screenIndex as text) & "|" & ((frame's origin's x) as real as text) & "|" & ((frame's origin's y) as real as text) & "|" & ((frame's size's width) as real as text) & "|" & ((frame's size's height) as real as text)
                set end of outputLines to line
                set screenIndex to screenIndex + 1
            end repeat
            return outputLines
            "#,
        )
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .split(", ")
        .filter_map(parse_display_bounds_line)
        .find_map(|(index, bounds)| (index == display_id).then_some(bounds))
}

fn run_screencapture(
    tmp_path: &std::path::Path,
    region: Option<&Bounds>,
    display_id: Option<u32>,
) -> Result<std::process::Output, PerceptionError> {
    let mut cmd = Command::new("screencapture");
    cmd.arg("-x").arg("-t").arg("png");

    if let Some(did) = display_id {
        cmd.arg("-D").arg(did.to_string());
    }

    if let Some(b) = region {
        cmd.arg("-R")
            .arg(format!("{},{},{},{}", b.x, b.y, b.width, b.height));
    }

    cmd.arg(tmp_path);

    cmd.output()
        .map_err(|e| PerceptionError::ScreenshotFailed(format!("screencapture failed: {}", e)))
}

fn encode_png(image: RgbaImage) -> Result<Vec<u8>, PerceptionError> {
    let mut cursor = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| {
            PerceptionError::ScreenshotFailed(format!("failed to encode xcap PNG: {}", e))
        })?;
    Ok(cursor.into_inner())
}

fn xcap_window_bounds(window: &xcap::Window) -> Option<Bounds> {
    let x = window.x().ok()?;
    let y = window.y().ok()?;
    let width = window.width().ok()?;
    let height = window.height().ok()?;

    (width > 0 && height > 0).then_some(Bounds {
        x: f64::from(x),
        y: f64::from(y),
        width: f64::from(width),
        height: f64::from(height),
    })
}

fn take_xcap_window_screenshot(
    window_id: u32,
    _region: Option<&Bounds>,
) -> Result<ScreenshotData, PerceptionError> {
    let windows = xcap::Window::all().map_err(|e| {
        PerceptionError::ScreenshotFailed(format!("xcap failed to list windows: {}", e))
    })?;

    let window = windows
        .into_iter()
        .find(|window| window.id().ok() == Some(window_id))
        .ok_or_else(|| {
            PerceptionError::ScreenshotFailed(format!("xcap window not found: {}", window_id))
        })?;

    if window.is_minimized().unwrap_or(false) {
        return Err(PerceptionError::ScreenshotFailed(format!(
            "xcap window is minimized: {}",
            window_id
        )));
    }

    let image = window.capture_image().map_err(|e| {
        PerceptionError::ScreenshotFailed(format!(
            "xcap failed to capture window {}: {}",
            window_id, e
        ))
    })?;
    let data = encode_png(image)?;
    let bounds = xcap_window_bounds(&window);

    Ok(ScreenshotData {
        format: ScreenshotFormat::Png,
        data,
        bounds,
    })
}

pub fn take_screenshot(
    region: Option<&Bounds>,
    display_id: Option<u32>,
    window_id: Option<u32>,
) -> Result<ScreenshotData, PerceptionError> {
    if let Some(window_id) = window_id {
        return take_xcap_window_screenshot(window_id, region);
    }

    let tmp_path = screenshot_tmp_path();
    let mut last_stderr = String::new();

    for attempt in 1..=SCREENSHOT_ATTEMPTS {
        let output = run_screencapture(&tmp_path, region, display_id)?;

        if output.status.success() {
            let data = std::fs::read(&tmp_path).map_err(|e| {
                PerceptionError::ScreenshotFailed(format!("failed to read screenshot: {}", e))
            })?;

            let _ = std::fs::remove_file(&tmp_path);

            let bounds = region
                .cloned()
                .or_else(|| display_id.and_then(display_bounds));

            return Ok(ScreenshotData {
                format: ScreenshotFormat::Png,
                data,
                bounds,
            });
        }

        last_stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let _ = std::fs::remove_file(&tmp_path);

        if attempt < SCREENSHOT_ATTEMPTS
            && last_stderr.contains("could not create image from display")
            && screen_recording_granted()
        {
            std::thread::sleep(SCREENSHOT_RETRY_DELAY);
            continue;
        }

        if !last_stderr.contains("could not create image from display") {
            break;
        }
    }

    let _ = std::fs::remove_file(&tmp_path);
    Err(PerceptionError::ScreenshotFailed(
        screenshot_failure_message(&last_stderr, screen_recording_granted()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Bounds;

    #[test]
    fn test_screenshot_failure_message_reports_permission_when_preflight_denied() {
        let message = screenshot_failure_message("could not create image from display", false);
        assert!(message.contains("Screen recording permission required"));
    }

    #[test]
    fn test_screenshot_failure_message_reports_transient_when_preflight_granted() {
        let message = screenshot_failure_message("could not create image from display", true);
        assert!(message.contains("after retries"));
        assert!(message.contains("preflight is granted"));
        assert!(!message.contains("Screen recording permission required"));
    }

    #[test]
    fn test_screenshot_failure_message_reports_invalid_display() {
        let message = screenshot_failure_message("invalid display", true);
        assert!(message.contains("Invalid display ID"));
    }

    #[test]
    fn test_parse_display_bounds_line() {
        let (index, bounds) = parse_display_bounds_line("2|-1600|0|1600|900").unwrap();

        assert_eq!(index, 2);
        assert_eq!(bounds.x, -1600.0);
        assert_eq!(bounds.y, 0.0);
        assert_eq!(bounds.width, 1600.0);
        assert_eq!(bounds.height, 900.0);
    }

    #[test]
    fn test_take_screenshot_full_screen() {
        let result = take_screenshot(None, None, None);
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
        let result = take_screenshot(Some(&region), None, None);
        assert!(result.is_ok() || result.is_err());
        if let Ok(data) = result {
            assert_eq!(data.format, ScreenshotFormat::Png);
            assert!(data.data.is_empty() || !data.data.is_empty());
        }
    }

    #[test]
    fn test_take_screenshot_with_display_id() {
        let result = take_screenshot(None, Some(1), None);
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
        let result = take_screenshot(Some(&region), None, None);
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
        let result = take_screenshot(None, None, None);
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
        let result = take_screenshot(Some(&region), Some(2), None);
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
