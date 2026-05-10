use std::collections::HashMap;
use std::process::Command;

use crate::perception::{AppContext, Context, PerceptionError};
use crate::selector::{App, Bounds, Window};

pub fn get_running_apps() -> Result<Context, PerceptionError> {
    let output = Command::new("wmctrl")
        .args(["-lxG"])
        .output()
        .map_err(|e| PerceptionError::PlatformError(format!("wmctrl failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(PerceptionError::PlatformError(format!(
            "wmctrl -lxG failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let front_window_id = front_window_id();
    let mut app_windows: HashMap<String, (String, bool, Vec<Window>)> = HashMap::new();

    for line in stdout.lines() {
        if let Some((window_id, desktop, x, y, width, height, wm_class, title)) =
            parse_wmctrl_line(line)
        {
            let app_name = wm_class.split('.').next().unwrap_or(&wm_class).to_string();
            let is_frontmost = front_window_id
                .as_ref()
                .map(|id| id == &window_id)
                .unwrap_or(false);

            let entry = app_windows
                .entry(wm_class.clone())
                .or_insert_with(|| (app_name.clone(), false, Vec::new()));

            entry.1 |= is_frontmost;
            entry.2.push(Window {
                id: window_id.clone(),
                title: title.to_string(),
                index: desktop.max(0) as u32,
                focused: is_frontmost,
                bounds: Bounds {
                    x,
                    y,
                    width,
                    height,
                },
                display_id: None,
            });
        }
    }

    let apps = app_windows
        .into_iter()
        .map(|(bundle_id, (name, is_frontmost, windows))| AppContext {
            app: App {
                name,
                bundle_id,
                is_frontmost,
            },
            windows,
        })
        .collect();

    Ok(Context { apps })
}

fn front_window_id() -> Option<String> {
    let output = Command::new("xdotool")
        .args(["getactivewindow"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = stdout.trim();
    if id.is_empty() {
        None
    } else {
        Some(id.to_string())
    }
}

fn parse_wmctrl_line(line: &str) -> Option<(String, i32, f64, f64, f64, f64, String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 9 {
        return None;
    }

    let window_id = parts[0].to_string();
    let desktop = parts[1].parse::<i32>().ok()?;
    let x = parts[2].parse::<f64>().ok()?;
    let y = parts[3].parse::<f64>().ok()?;
    let width = parts[4].parse::<f64>().ok()?;
    let height = parts[5].parse::<f64>().ok()?;
    let wm_class = parts[7].to_string();
    let title = parts[8..].join(" ");

    Some((window_id, desktop, x, y, width, height, wm_class, title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wmctrl_line() {
        let line = "0x01200007 0 10 20 1280 720 code.Code hello.txt - Visual Studio Code";
        let parsed = parse_wmctrl_line(line).unwrap();
        assert_eq!(parsed.0, "0x01200007");
        assert_eq!(parsed.1, 0);
        assert_eq!(parsed.2, 10.0);
        assert_eq!(parsed.3, 20.0);
        assert_eq!(parsed.4, 1280.0);
        assert_eq!(parsed.5, 720.0);
        assert_eq!(parsed.6, "code.Code");
        assert_eq!(parsed.7, "hello.txt - Visual Studio Code");
    }
}
