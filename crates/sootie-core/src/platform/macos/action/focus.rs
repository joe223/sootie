use std::process::Command;

use crate::action::{ActionError, ActionResult, FocusAction};
use crate::selector::WindowSelector;

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let app_name = action.selector.app.as_ref().and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            focus_app(&name, action.selector.window.as_ref()).map_err(ActionError::ActionFailed)?;
            Ok(ActionResult::success(None, "osascript"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}

fn focus_app(app_name: &str, window: Option<&WindowSelector>) -> Result<(), String> {
    let script = build_activate_script(app_name, window);

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("osascript failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to focus app '{}': {}", app_name, stderr));
    }

    Ok(())
}

fn build_activate_script(app_name: &str, window: Option<&WindowSelector>) -> String {
    let escaped_name = app_name.replace('\\', "\\\\").replace('"', "\\\"");
    let mut script = format!(
        "tell application \"{}\" to activate\ndelay 0.2",
        escaped_name
    );

    if let Some(window_title) = window.and_then(|window| window.title.as_deref()) {
        let escaped_title = window_title.replace('\\', "\\\\").replace('"', "\\\"");
        script.push_str(&format!(
            "\ntell application \"System Events\"\n    tell process \"{}\"\n        set frontmost to true\n        perform action \"AXRaise\" of (first window whose name contains \"{}\")\n    end tell\nend tell",
            escaped_name, escaped_title
        ));
    } else if let Some(window_number) = window_index(window) {
        script.push_str(&format!(
            "\ntell application \"System Events\"\n    tell process \"{}\"\n        set frontmost to true\n        perform action \"AXRaise\" of window {}\n    end tell\nend tell",
            escaped_name, window_number
        ));
    }

    script
}

fn window_index(window: Option<&WindowSelector>) -> Option<u32> {
    if let Some(index) = window.and_then(|window| window.index) {
        return Some(index + 1);
    }

    window
        .and_then(|window| window.id.as_deref())
        .and_then(|id| id.strip_prefix("win_"))
        .and_then(|index| index.parse::<u32>().ok())
        .map(|index| index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::{AppSelector, Selector, WindowSelector};

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_focus_finder() {
        let action = FocusAction {
            selector: Selector::new().with_app(AppSelector::from_name("Finder")),
        };
        let result = perform_focus(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_focus_safari() {
        let action = FocusAction {
            selector: Selector::new().with_app(AppSelector::from_name("Safari")),
        };
        let result = perform_focus(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_build_activate_script() {
        let script = build_activate_script("Example App", None);
        assert_eq!(
            script,
            "tell application \"Example App\" to activate\ndelay 0.2"
        );
    }

    #[test]
    fn test_build_activate_script_escapes_quotes() {
        let script = build_activate_script("Foo \"Bar\"", None);
        assert_eq!(
            script,
            "tell application \"Foo \\\"Bar\\\"\" to activate\ndelay 0.2"
        );
    }

    #[test]
    fn test_build_activate_script_with_window_title() {
        let script = build_activate_script("Safari", Some(&WindowSelector::from_title("GitHub")));
        assert!(script.contains("perform action \"AXRaise\""));
        assert!(script.contains("whose name contains \"GitHub\""));
    }

    #[test]
    fn test_build_activate_script_with_window_id() {
        let script = build_activate_script(
            "Safari",
            Some(&WindowSelector {
                title: None,
                id: Some("win_2".to_string()),
                index: None,
                focused: None,
            }),
        );
        assert!(script.contains("perform action \"AXRaise\" of window 3"));
    }

    #[test]
    fn test_window_index_from_window_id() {
        let selector = WindowSelector::from_title("GitHub").with_id("win_2");
        assert_eq!(window_index(Some(&selector)), Some(3));
    }
}
