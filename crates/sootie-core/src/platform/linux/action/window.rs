use std::process::Command;

use crate::action::{ActionError, ActionResult, WindowAction, WindowOperation};

pub fn perform_window_op(action: &WindowAction) -> Result<ActionResult, ActionError> {
    let window_id = super::resolver::resolve_window_id(&action.selector)?;

    match action.operation {
        WindowOperation::Minimize => {
            Command::new("xdotool")
                .arg("windowminimize")
                .arg(&window_id)
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("windowminimize failed: {}", e)))?;
        }
        WindowOperation::Maximize => {
            Command::new("wmctrl")
                .args(["-ir", &window_id, "-b", "add,maximized_vert,maximized_horz"])
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("maximize failed: {}", e)))?;
        }
        WindowOperation::Close => {
            Command::new("wmctrl")
                .args(["-ic", &window_id])
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("window close failed: {}", e)))?;
        }
        WindowOperation::Move { x, y } => {
            Command::new("xdotool")
                .arg("windowmove")
                .arg(&window_id)
                .arg(x.to_string())
                .arg(y.to_string())
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("windowmove failed: {}", e)))?;
        }
        WindowOperation::Resize { width, height } => {
            Command::new("xdotool")
                .arg("windowsize")
                .arg(&window_id)
                .arg(width.to_string())
                .arg(height.to_string())
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("windowsize failed: {}", e)))?;
        }
    }

    Ok(ActionResult::success(None, "xdotool"))
}
