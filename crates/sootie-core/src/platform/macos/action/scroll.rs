use crate::action::{ActionError, ActionResult, ActionTarget, ScrollAction, ScrollDirection};
use crate::perception::PerceptionProvider;

use super::mouse::simulate_scroll;
use super::utils::resolve_target;

pub async fn perform_scroll<P: PerceptionProvider>(
    action: &ScrollAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let direction = match action.direction {
        ScrollDirection::Up => "up",
        ScrollDirection::Down => "down",
        ScrollDirection::Left => "left",
        ScrollDirection::Right => "right",
    };
    let amount = action.amount.unwrap_or(3);

    let (x, y) = match &action.target {
        Some(ActionTarget::Coordinate(coord)) => (coord.x, coord.y),
        Some(ActionTarget::Selector(_)) => {
            let coord = resolve_target(&action.target.clone().unwrap(), perception).await?;
            (coord.x, coord.y)
        }
        None => (0.0, 0.0),
    };

    simulate_scroll(x, y, direction, amount)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}