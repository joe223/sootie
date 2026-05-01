use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, ClickAction};
use crate::perception::PerceptionProvider;

pub async fn perform_click<P: PerceptionProvider>(
    action: &ClickAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    use crate::action::ActionTarget;

    let (x, y) = match &action.target {
        ActionTarget::Coordinate(coord) => (coord.x as i32, coord.y as i32),
        ActionTarget::Selector(selector) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            (cx as i32, cy as i32)
        }
    };

    unsafe {
        SetCursorPos(x, y);

        let button_flag = match action.button {
            Some(crate::action::MouseButton::Right) => MOUSEEVENTF_RIGHTDOWN | MOUSEEVENTF_RIGHTUP,
            Some(crate::action::MouseButton::Middle) => MOUSEEVENTF_MIDDLEDOWN | MOUSEEVENTF_MIDDLEUP,
            _ => MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP,
        };

        for _ in 0..action.count.unwrap_or(1) {
            mouse_event(button_flag, 0, 0, 0, None);
        }
    }

    Ok(ActionResult::success(None, "win32"))
}