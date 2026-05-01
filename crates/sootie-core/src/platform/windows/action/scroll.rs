use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, ActionTarget, ScrollAction, ScrollDirection};
use crate::perception::PerceptionProvider;

pub async fn perform_scroll<P: PerceptionProvider>(
    action: &ScrollAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (x, y) = match &action.target {
        Some(ActionTarget::Coordinate(coord)) => (coord.x as i32, coord.y as i32),
        Some(ActionTarget::Selector(selector)) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            (cx as i32, cy as i32)
        }
        None => (0, 0),
    };

    let amount = action.amount.unwrap_or(3);
    let delta = match action.direction {
        ScrollDirection::Up => amount as i32,
        ScrollDirection::Down => -(amount as i32),
        ScrollDirection::Left => amount as i32,
        ScrollDirection::Right => -(amount as i32),
    };

    unsafe {
        SetCursorPos(x, y);

        for _ in 0..amount {
            mouse_event(
                MOUSEEVENTF_WHEEL,
                0,
                0,
                delta,
                None,
            );
        }
    }

    Ok(ActionResult::success(None, "win32"))
}