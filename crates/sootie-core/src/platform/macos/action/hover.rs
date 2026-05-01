use crate::action::{ActionError, ActionResult, HoverAction};
use crate::perception::PerceptionProvider;

use super::mouse::simulate_mouse_move;
use super::utils::resolve_target;

pub async fn perform_hover<P: PerceptionProvider>(
    action: &HoverAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let coord = resolve_target(&action.target, perception).await?;
    
    simulate_mouse_move(coord.x, coord.y)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}