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
            let output = Command::new("wmctrl")
                .arg("-a")
                .arg(&name)
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("wmctrl focus failed: {}", e)))?;

            if !output.status.success() {
                return Err(ActionError::ActionFailed(format!("Failed to focus app '{}'", name)));
            }

            Ok(ActionResult::success(None, "wmctrl"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}