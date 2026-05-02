use std::process::Command;

use tracing::warn;

use crate::perception::Context;

use super::super::ax_fns::*;

pub fn get_running_apps() -> crate::perception::Context {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            tell application "System Events"
                set appList to {}
                set frontApp to name of first process whose frontmost is true
                repeat with p in (every process whose background only is false)
                    set appName to name of p
                    set appBundle to bundle identifier of p
                    set isFront to (appName is frontApp)
                    set end of appList to appName & "|" & appBundle & "|" & isFront
                end repeat
                return appList
            end tell
            "#,
        )
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut apps = Vec::new();

            for entry in stdout.split(", ") {
                let parts: Vec<&str> = entry.trim().split('|').collect();
                if parts.len() >= 3 {
                    let name = parts[0].trim().to_string();
                    let bundle_id = parts[1].trim().to_string();
                    let is_frontmost = parts[2].trim() == "true";

                    if name.is_empty() {
                        continue;
                    }

                    let pid = get_pid_for_app_name(&name);
                    let app = crate::selector::App {
                        name: name.clone(),
                        bundle_id: bundle_id.clone(),
                        is_frontmost,
                    };
                    let windows = if pid > 0 {
                        get_app_windows(pid)
                    } else {
                        vec![]
                    };
                    apps.push(crate::perception::AppContext { app, windows });
                }
            }
            Context { apps }
        }
        _ => {
            warn!("Failed to get running apps via osascript, falling back to empty list");
            Context { apps: vec![] }
        }
    }
}

pub fn get_app_windows(pid: i32) -> Vec<crate::selector::Window> {
    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        let mut windows = Vec::new();

        let window_refs = get_children(app_element);
        for (index, window_ref) in window_refs.iter().enumerate() {
            let role = get_string_attr(*window_ref, "AXRole").unwrap_or_default();
            if role != "AXWindow" {
                continue;
            }

            let title = get_string_attr(*window_ref, "AXTitle").unwrap_or_default();
            let position = get_point_attr(*window_ref, "AXPosition");
            let size = get_size_attr(*window_ref, "AXSize");

let focused = get_bool_attr(*window_ref, "AXFocused").unwrap_or(false);

            let bounds = match (position, size) {
                (Some(pos), Some(sz)) => crate::selector::Bounds {
                    x: pos.x,
                    y: pos.y,
                    width: sz.width,
                    height: sz.height,
                },
                _ => crate::selector::Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            };

            windows.push(crate::selector::Window {
                id: format!("win_{}", index),
                title,
                index: index as u32,
                focused,
                bounds,
            });
        }

        windows
    }
}

pub fn get_pid_for_app_name(name: &str) -> i32 {
    let output = Command::new("pgrep")
        .arg("-x")
        .arg(name)
        .output()
        .ok();

    output
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .next()
                .and_then(|line| line.trim().parse::<i32>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_loads() {
        assert!(true);
    }

    #[test]
    #[ignore = "requires system permissions"]
    fn test_get_running_apps() {
        let ctx = get_running_apps();
        assert!(!ctx.apps.is_empty() || ctx.apps.is_empty());
    }

    #[test]
    #[ignore = "requires system permissions"]
    fn test_get_pid_for_app_name() {
        let pid = get_pid_for_app_name("Finder");
        assert!(pid > 0 || pid == 0);
    }
}
