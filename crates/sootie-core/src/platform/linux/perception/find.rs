use std::process::Command;

use crate::perception::PerceptionError;
use crate::selector::{Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector};

pub fn find_elements(selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
    let output = Command::new("xdotool")
        .arg("search")
        .arg("--name")
        .arg(selector.element.name.as_deref().unwrap_or(""))
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let window_ids: Vec<&str> = stdout.lines().collect();

            let elements: Vec<Element> = window_ids
                .iter()
                .enumerate()
                .filter_map(|(index, win_id)| {
                    let window_info = get_window_info(win_id)?;
                    Some(Element {
                        role: "window".to_string(),
                        name: window_info.title,
                        text: None,
                        id: Some(win_id.to_string()),
                        state: ElementState {
                            visible: true,
                            focused: Some(true),
                            enabled: Some(true),
                        },
                        bounds: window_info.bounds,
                        index: index as u32,
                    })
                })
                .collect();

            let (status, total_matches) = match elements.len() {
                0 => (MatchStatus::None, 0),
                1 => (MatchStatus::Unique, 1),
                n => (MatchStatus::Multiple, n as u32),
            };

            Ok(ResolvedTarget {
                status,
                total_matches,
                app: None,
                window: None,
                elements,
            })
        }
        _ => Ok(ResolvedTarget {
            status: MatchStatus::None,
            total_matches: 0,
            app: None,
            window: None,
            elements: vec![],
        }),
    }
}

struct WindowInfo {
    title: String,
    bounds: Bounds,
}

fn get_window_info(window_id: &str) -> Option<WindowInfo> {
    let title_output = Command::new("xdotool")
        .arg("getwindowname")
        .arg(window_id)
        .output()
        .ok()?;

    let title = if title_output.status.success() {
        String::from_utf8_lossy(&title_output.stdout).trim().to_string()
    } else {
        String::new()
    };

    let geometry_output = Command::new("xdotool")
        .arg("getwindowgeometry")
        .arg(window_id)
        .output()
        .ok()?;

    let bounds = if geometry_output.status.success() {
        let geo_stdout = String::from_utf8_lossy(&geometry_output.stdout);
        parse_geometry(&geo_stdout)
    } else {
        Bounds {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        }
    };

    Some(WindowInfo { title, bounds })
}

fn parse_geometry(geometry_str: &str) -> Bounds {
    let parts: Vec<&str> = geometry_str.split_whitespace().collect();
    for part in parts {
        if part.starts_with("Position:") {
            let pos_parts: Vec<&str> = part.split(',').collect();
            if pos_parts.len() >= 2 {
                let x = pos_parts[0].replace("Position:", "").trim().parse().unwrap_or(0.0);
                let y = pos_parts[1].trim().parse().unwrap_or(0.0);
                return Bounds { x, y, width: 800.0, height: 600.0 };
            }
        }
        if part.starts_with("Geometry:") {
            let geo_parts: Vec<&str> = part.split('x').collect();
            if geo_parts.len() >= 2 {
                let width = geo_parts[0].replace("Geometry:", "").trim().parse().unwrap_or(800.0);
                let height = geo_parts[1].trim().parse().unwrap_or(600.0);
                return Bounds { x: 0.0, y: 0.0, width, height };
            }
        }
    }
    Bounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    }
}