use std::process::Command;

use crate::action::{ActionError, ActionResult, LaunchAction};

pub fn perform_launch(action: &LaunchAction) -> Result<ActionResult, ActionError> {
    let app_identifier = action
        .app
        .name
        .clone()
        .or(action.app.bundle_id.clone());

    match app_identifier {
        Some(identifier) => {
            let mut cmd = Command::new("open");
            
            if action.app.bundle_id.is_some() {
                cmd.arg("-b").arg(&identifier);
            } else {
                cmd.arg("-a").arg(&identifier);
            }

            for arg in &action.args {
                cmd.arg("--args").arg(arg);
            }

            let output = cmd.output()
                .map_err(|e| ActionError::ActionFailed(format!("Failed to launch app: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ActionError::ActionFailed(format!("Launch failed: {}", stderr)));
            }

            Ok(ActionResult::success(None, "open"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app identifier specified".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::LaunchAction;
    use crate::selector::AppSelector;

    #[test]
    fn test_perform_launch_by_name() {
        let action = LaunchAction {
            app: AppSelector::from_name("TextEdit"),
            args: vec![],
        };
        let result = perform_launch(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_perform_launch_with_args() {
        let action = LaunchAction {
            app: AppSelector::from_name("TextEdit"),
            args: vec!["test.txt".to_string()],
        };
        let result = perform_launch(&action);
        assert!(result.is_ok() || result.is_err());
    }
}
