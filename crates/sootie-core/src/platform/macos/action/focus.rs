use std::process::Command;

use crate::action::{ActionError, ActionResult, FocusAction};

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let app_name = action
        .selector
        .app
        .as_ref()
        .and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            focus_app(&name)
                .map_err(|e| ActionError::ActionFailed(e))?;
            Ok(ActionResult::success(None, "osascript"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}

fn focus_app(app_name: &str) -> Result<(), String> {
    let script = format!(
        "tell application \"System Events\" to set frontmost of process \"{}\" to true",
        app_name
    );
    
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