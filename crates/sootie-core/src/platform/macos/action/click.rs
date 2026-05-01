use crate::action::{ActionError, ActionResult, ClickAction};
use crate::perception::PerceptionProvider;

use super::mouse::simulate_click;
use super::utils::resolve_target;

pub async fn perform_click<P: PerceptionProvider>(
    action: &ClickAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let button = match action.button {
        Some(crate::action::MouseButton::Right) => "right",
        Some(crate::action::MouseButton::Middle) => "middle",
        _ => "left",
    };
    let count = action.count.unwrap_or(1);

    let coord = resolve_target(&action.target, perception).await?;
    
    simulate_click(coord.x, coord.y, button, count)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}