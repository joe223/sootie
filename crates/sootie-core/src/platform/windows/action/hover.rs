use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, ActionTarget, HoverAction};
use crate::perception::PerceptionProvider;

pub async fn perform_hover<P: PerceptionProvider>(
    action: &HoverAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
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
    }

    Ok(ActionResult::success(None, "win32"))
}