use std::process::Command;

use crate::action::{ActionError, ActionResult, LaunchAction};

pub fn perform_launch(action: &LaunchAction) -> Result<ActionResult, ActionError> {
    let app_identifier = action.app.name.or(action.app.bundle_id).clone();

    match app_identifier {
        Some(identifier) => {
            let mut cmd = Command::new("gtk-launch");
            cmd.arg(&identifier);

            for arg in &action.args {
                cmd.arg(arg);
            }

            let output = cmd
                .output()
                .map_err(|e| ActionError::ActionFailed(format!("Failed to launch app: {}", e)))?;

            if !output.status.success() {
                // Fallback: try direct execution
                let fallback_output = Command::new(&identifier)
                    .args(&action.args)
                    .output()
                    .map_err(|e| {
                        ActionError::ActionFailed(format!("Fallback launch failed: {}", e))
                    })?;

                if !fallback_output.status.success() {
                    let stderr = String::from_utf8_lossy(&fallback_output.stderr);
                    return Err(ActionError::ActionFailed(format!(
                        "Launch failed: {}",
                        stderr
                    )));
                }
            }

            Ok(ActionResult::success(None, "gtk-launch"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app identifier specified".to_string(),
        )),
    }
}
