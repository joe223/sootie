use std::process::Command;

use crate::action::{ActionError, ActionResult, HotkeyAction};

pub fn perform_hotkey(action: &HotkeyAction) -> Result<ActionResult, ActionError> {
    let keys_str = action.keys.join("+");

    Command::new("xdotool")
        .arg("key")
        .arg(&keys_str)
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("Hotkey failed: {}", e)))?;

    Ok(ActionResult::success(None, "xdotool"))
}