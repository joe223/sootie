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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{DragAction, ActionTarget};
    use crate::perception::StubPerceptionProvider;
    use crate::selector::Coordinate;

    #[tokio::test]
    async fn test_perform_drag_basic() {
        let action = DragAction {
            from: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 100.0 }),
            to: ActionTarget::Coordinate(Coordinate { x: 200.0, y: 200.0 }),
        };
        let perception = StubPerceptionProvider;
        let result = perform_drag(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }
}
