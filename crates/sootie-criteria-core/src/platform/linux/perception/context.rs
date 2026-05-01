use std::process::Command;

use tracing::warn;

use crate::perception::{AppContext, Context};
use crate::selector::{App, Bounds, Window};

pub fn get_running_apps() -> Result<Context, crate::perception::PerceptionError> {
    let output = Command::new("wmctrl")
        .arg("-l")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut apps = Vec::new();

            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let window_id = parts[0].to_string();
                    let desktop_id = parts[1];
                    let title = parts[2..].join(" ");

                    apps.push(AppContext {
                        app: App {
                            name: title.clone(),
                            bundle_id: format!("desktop_{}", desktop_id),
                            is_frontmost: true,
                        },
                        windows: vec![Window {
                            id: window_id,
                            title,
                            index: 0,
                            focused: true,
                            bounds: Bounds {
                                x: 0.0,
                                y: 0.0,
                                width: 1920.0,
                                height: 1080.0,
                            },
                        }],
                    });
                }
            }
            Ok(Context { apps })
        }
        _ => {
            warn!("wmctrl not available, returning empty context");
            Ok(Context { apps: vec![] })
        }
    }
}