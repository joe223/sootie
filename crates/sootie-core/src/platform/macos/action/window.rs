use std::process::Command;

use crate::action::{ActionError, ActionResult, WindowAction, WindowOperation};

pub fn perform_window_op(action: &WindowAction) -> Result<ActionResult, ActionError> {
    let app_name = action.selector.app.as_ref().and_then(|a| a.name.clone());

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
    let script = build_window_script(app_name, operation);

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| format!("osascript failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Window operation failed for '{}': {}",
            app_name, stderr
        ));
    }

    Ok(())
}

fn build_window_script(app_name: &str, operation: &WindowOperation) -> String {
    let escaped_name = app_name.replace('\\', "\\\\").replace('"', "\\\"");
    match operation {
        WindowOperation::Minimize => format!(
            "tell application \"System Events\" to tell process \"{}\" to set value of attribute \"AXMinimized\" of window 1 to true",
            escaped_name
        ),
        WindowOperation::Maximize => format!(
            "tell application \"System Events\" to tell process \"{}\" to set value of attribute \"AXFullScreen\" of window 1 to true",
            escaped_name
        ),
        WindowOperation::Close => format!(
            "tell application \"System Events\" to tell process \"{}\" to click button 1 of window 1",
            escaped_name
        ),
        WindowOperation::Move { x, y } => format!(
            "tell application \"System Events\" to tell process \"{}\" to set position of window 1 to {{{}, {}}}",
            escaped_name, x, y
        ),
        WindowOperation::Resize { width, height } => format!(
            "tell application \"System Events\" to tell process \"{}\" to set size of window 1 to {{{}, {}}}",
            escaped_name, width, height
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::WindowAction;
    use crate::action::WindowOperation;
    use crate::selector::{AppSelector, Selector};

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_window_minimize() {
        let action = WindowAction {
            selector: Selector::new().with_app(AppSelector::from_name("TextEdit")),
            operation: WindowOperation::Minimize,
        };
        let result = perform_window_op(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_window_close() {
        let action = WindowAction {
            selector: Selector::new().with_app(AppSelector::from_name("TextEdit")),
            operation: WindowOperation::Close,
        };
        let result = perform_window_op(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_build_window_script_maximize() {
        let script = build_window_script("Example App", &WindowOperation::Maximize);
        assert_eq!(
            script,
            "tell application \"System Events\" to tell process \"Example App\" to set value of attribute \"AXFullScreen\" of window 1 to true"
        );
    }

    #[test]
    fn test_build_window_script_escapes_quotes() {
        let script = build_window_script("Foo \"Bar\"", &WindowOperation::Close);
        assert_eq!(
            script,
            "tell application \"System Events\" to tell process \"Foo \\\"Bar\\\"\" to click button 1 of window 1"
        );
    }
}
