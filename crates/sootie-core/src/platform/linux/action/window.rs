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
            match action.operation {
                WindowOperation::Minimize => {
                    Command::new("xdotool")
                        .arg("windowminimize")
                        .arg(&name)
                        .output()
                        .map_err(|e| ActionError::ActionFailed(format!("windowminimize failed: {}", e)))?;
                }
                WindowOperation::Maximize => {
                    Command::new("wmctrl")
                        .arg("-r")
                        .arg(&name)
                        .arg("-b")
                        .arg("add,maximized_vert,maximized_horz")
                        .output()
                        .map_err(|e| ActionError::ActionFailed(format!("maximize failed: {}", e)))?;
                }
                WindowOperation::Close => {
                    Command::new("wmctrl")
                        .arg("-c")
                        .arg(&name)
                        .output()
                        .map_err(|e| ActionError::ActionFailed(format!("window close failed: {}", e)))?;
                }
                WindowOperation::Move { x, y } => {
                    Command::new("xdotool")
                        .arg("windowmove")
                        .arg(&name)
                        .arg(x.to_string())
                        .arg(y.to_string())
                        .output()
                        .map_err(|e| ActionError::ActionFailed(format!("windowmove failed: {}", e)))?;
                }
                WindowOperation::Resize { width, height } => {
                    Command::new("xdotool")
                        .arg("windowsize")
                        .arg(&name)
                        .arg(width.to_string())
                        .arg(height.to_string())
                        .output()
                        .map_err(|e| ActionError::ActionFailed(format!("windowsize failed: {}", e)))?;
                }
            }

            Ok(ActionResult::success(None, "xdotool"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}