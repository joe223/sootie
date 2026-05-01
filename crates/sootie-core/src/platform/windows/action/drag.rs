use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, ActionTarget, DragAction};
use crate::perception::PerceptionProvider;

pub async fn perform_drag<P: PerceptionProvider>(
    action: &DragAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let from = match &action.from {
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

    let to = match &action.to {
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
        SetCursorPos(from.0, from.1);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, None);

        SetCursorPos(to.0, to.1);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, None);
    }

    Ok(ActionResult::success(None, "win32"))
}