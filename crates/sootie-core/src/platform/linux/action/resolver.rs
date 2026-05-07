use std::process::Command;

use crate::action::ActionError;
use crate::selector::Selector;

pub fn resolve_window_id(selector: &Selector) -> Result<String, ActionError> {
    let active_window_id = active_window_id();
    let output = Command::new("wmctrl")
        .args(["-lx"])
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("wmctrl lookup failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActionError::ActionFailed(format!(
            "wmctrl -lx failed: {}",
            stderr.trim()
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut matches = Vec::new();
    for line in stdout.lines() {
        let Some(entry) = parse_wmctrl_window(line, active_window_id.as_deref()) else {
            continue;
        };

        if matches_selector(&entry, selector) {
            matches.push(entry);
        }
    }

    matches
        .into_iter()
        .max_by_key(|entry| entry.focused)
        .map(|entry| entry.id)
        .ok_or_else(|| ActionError::TargetNotFound("no matching window found".to_string()))
}

fn active_window_id() -> Option<String> {
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

#[derive(Debug)]
struct WindowEntry {
    id: String,
    wm_class: String,
    title: String,
    focused: bool,
}

fn parse_wmctrl_window(line: &str, active_window_id: Option<&str>) -> Option<WindowEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let id = parts[0].to_string();
    let wm_class = parts[3].to_string();
    let title = parts[4..].join(" ");
    let focused = active_window_id.map(|active| active == id).unwrap_or(false);

    Some(WindowEntry {
        id,
        wm_class,
        title,
        focused,
    })
}

fn matches_selector(entry: &WindowEntry, selector: &Selector) -> bool {
    if let Some(app) = selector.app.as_ref() {
        if let Some(name) = app.name.as_ref() {
            let short_class = entry.wm_class.split('.').next().unwrap_or(&entry.wm_class);
            if !short_class.to_lowercase().contains(&name.to_lowercase())
                && !entry.title.to_lowercase().contains(&name.to_lowercase())
            {
                return false;
            }
        }

        if let Some(bundle_id) = app.bundle_id.as_ref() {
            if !entry
                .wm_class
                .to_lowercase()
                .contains(&bundle_id.to_lowercase())
            {
                return false;
            }
        }

        if let Some(frontmost) = app.is_frontmost {
            if entry.focused != frontmost {
                return false;
            }
        }
    }

    if let Some(window) = selector.window.as_ref() {
        if let Some(title) = window.title.as_ref() {
            if !entry.title.to_lowercase().contains(&title.to_lowercase()) {
                return false;
            }
        }

        if let Some(id) = window.id.as_ref() {
            if &entry.id != id {
                return false;
            }
        }

        if let Some(focused) = window.focused {
            if entry.focused != focused {
                return false;
            }
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::{AppSelector, Selector, WindowSelector};

    #[test]
    fn test_parse_wmctrl_window() {
        let entry = parse_wmctrl_window(
            "0x01200007  0  host code.Code hello.txt - Visual Studio Code",
            Some("0x01200007"),
        )
        .unwrap();

        assert_eq!(entry.id, "0x01200007");
        assert_eq!(entry.wm_class, "code.Code");
        assert_eq!(entry.title, "hello.txt - Visual Studio Code");
        assert!(entry.focused);
    }

    #[test]
    fn test_matches_selector_with_app_and_window() {
        let entry = WindowEntry {
            id: "0x1".to_string(),
            wm_class: "code.Code".to_string(),
            title: "hello.txt - Visual Studio Code".to_string(),
            focused: true,
        };
        let selector = Selector::new()
            .with_app(AppSelector::from_name("code").with_bundle_id("code.Code"))
            .with_window(WindowSelector::from_title("hello.txt").with_focused(true));

        assert!(matches_selector(&entry, &selector));
    }
}
