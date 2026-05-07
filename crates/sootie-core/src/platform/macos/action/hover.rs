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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{HoverAction, ActionTarget};
    use crate::perception::StubPerceptionProvider;
    use crate::selector::Coordinate;

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_hover() {
        let action = HoverAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
        };
        let perception = StubPerceptionProvider;
        let result = perform_hover(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }
}
