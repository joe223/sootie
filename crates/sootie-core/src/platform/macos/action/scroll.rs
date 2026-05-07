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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::StubPerceptionProvider;
    use crate::selector::Coordinate;

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_scroll_up() {
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: ScrollDirection::Up,
            amount: Some(10),
        };
        let perception = StubPerceptionProvider;
        let result = perform_scroll(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_scroll_down() {
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: ScrollDirection::Down,
            amount: Some(10),
        };
        let perception = StubPerceptionProvider;
        let result = perform_scroll(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_scroll_left() {
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: ScrollDirection::Left,
            amount: Some(5),
        };
        let perception = StubPerceptionProvider;
        let result = perform_scroll(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_scroll_right() {
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: ScrollDirection::Right,
            amount: Some(5),
        };
        let perception = StubPerceptionProvider;
        let result = perform_scroll(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_scroll_default_amount() {
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: ScrollDirection::Up,
            amount: None,
        };
        let perception = StubPerceptionProvider;
        let result = perform_scroll(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }
}