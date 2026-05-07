use std::process::Command;

use crate::action::{ActionError, ActionResult, PressAction};

pub fn perform_press(action: &PressAction) -> Result<ActionResult, ActionError> {
    Command::new("xdotool")
        .arg("key")
        .arg(&action.key)
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("Key press failed: {}", e)))?;

    Ok(ActionResult::success(None, "xdotool"))
}
