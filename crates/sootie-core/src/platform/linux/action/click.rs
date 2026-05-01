use std::process::Command;

use crate::action::{ActionError, ActionResult, ClickAction};
use crate::perception::PerceptionProvider;

use super::mouse::click_at;

pub async fn perform_click<P: PerceptionProvider>(
    action: &ClickAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    use crate::action::ActionTarget;

    let (x, y) = match &action.target {
        ActionTarget::Coordinate(coord) => (coord.x, coord.y),
        ActionTarget::Selector(selector) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            (cx, cy)
        }
    };

    let button = match action.button {
        Some(crate::action::MouseButton::Right) => 3,
        Some(crate::action::MouseButton::Middle) => 2,
        _ => 1,
    };

    click_at(x, y, button, action.count.unwrap_or(1))
        .map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, "xdotool"))
}