use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, TypeAction};
use crate::perception::PerceptionProvider;

use super::keyboard::type_text;

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    use crate::action::ActionTarget;

    if let Some(ref target) = action.target {
        let (x, y) = match target {
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
            mouse_event(MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP, 0, 0, 0, None);
        }
    }

    if action.clear_first.unwrap_or(false) {
        unsafe {
            keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, None);
            keybd_event(VK_KEY_A as u8, 0, KEYEVENTF_EXTENDEDKEY, None);
            keybd_event(VK_KEY_A as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);
            keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);
        }
    }

    type_text(&action.text)
        .map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, "win32"))
}