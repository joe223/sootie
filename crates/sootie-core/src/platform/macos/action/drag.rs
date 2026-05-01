use crate::action::{ActionError, ActionResult, DragAction};
use crate::perception::PerceptionProvider;

use super::mouse::simulate_drag;
use super::utils::resolve_target;

pub async fn perform_drag<P: PerceptionProvider>(
    action: &DragAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let from = resolve_target(&action.from, perception).await?;
    let to = resolve_target(&action.to, perception).await?;
    
    simulate_drag(from, to)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}