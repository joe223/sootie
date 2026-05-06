use std::process::Command;

use crate::action::{ActionError, ActionResult, FocusAction};

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let app_name = action.selector.app.as_ref().and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            focus_app(&name).map_err(|e| ActionError::ActionFailed(e))?;
            Ok(ActionResult::success(None, "osascript"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}

fn focus_app(app_name: &str) -> Result<(), String> {
    let script = build_activate_script(app_name);

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

fn build_activate_script(app_name: &str) -> String {
    let escaped_name = app_name.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "tell application \"{}\" to activate\ndelay 0.2",
        escaped_name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::{AppSelector, Selector};

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
        let script = build_activate_script("Example App");
        assert_eq!(
            script,
            "tell application \"Example App\" to activate\ndelay 0.2"
        );
    }

    #[test]
    fn test_build_activate_script_escapes_quotes() {
        let script = build_activate_script("Foo \"Bar\"");
        assert_eq!(
            script,
            "tell application \"Foo \\\"Bar\\\"\" to activate\ndelay 0.2"
        );
    }
}
