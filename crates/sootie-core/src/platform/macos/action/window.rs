use std::process::Command;

use crate::action::{ActionError, ActionResult, WindowAction, WindowOperation};

pub fn perform_window_op(action: &WindowAction) -> Result<ActionResult, ActionError> {
    let app_name = action
        .selector
        .app
        .as_ref()
        .and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            perform_window_op_internal(&name, &action.operation)
                .map_err(|e| ActionError::ActionFailed(e))?;
            Ok(ActionResult::success(None, "osascript"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}

fn perform_window_op_internal(app_name: &str, operation: &WindowOperation) -> Result<(), String> {
    let script = match operation {
        WindowOperation::Minimize => format!(
            "tell application \"System Events\" to tell process \"{}\" to set value of attribute \"AXMinimized\" of window 1 to true",
            app_name
        ),
        WindowOperation::Maximize => format!(
            "tell application \"{}\" to activate",
            app_name
        ),
        WindowOperation::Close => format!(
            "tell application \"System Events\" to tell process \"{}\" to click button 1 of window 1",
            app_name
        ),
        WindowOperation::Move { x, y } => format!(
            "tell application \"System Events\" to tell process \"{}\" to set position of window 1 to {{{}, {}}}",
            app_name, x, y
        ),
        WindowOperation::Resize { width, height } => format!(
            "tell application \"System Events\" to tell process \"{}\" to set size of window 1 to {{{}, {}}}",
            app_name, width, height
        ),
    };

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("osascript failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Window operation failed for '{}': {}", app_name, stderr));
    }

    Ok(())
}