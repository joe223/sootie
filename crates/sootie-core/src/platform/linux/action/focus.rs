use std::process::Command;

use crate::action::{ActionError, ActionResult, FocusAction};

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let window_id = super::resolver::resolve_window_id(&action.selector)?;
    let output = Command::new("wmctrl")
        .args(["-ia", &window_id])
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("wmctrl focus failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ActionError::ActionFailed(format!(
            "Failed to focus window '{}': {}",
            window_id,
            stderr.trim()
        )));
    }

    Ok(ActionResult::success(None, "wmctrl"))
}
