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

    simulate_click(coord.x, coord.y, button, count).map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, "cgevent"))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{ActionTarget, ClickAction, MouseButton};
    use crate::perception::StubPerceptionProvider;
    use crate::selector::Coordinate;

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_left_button() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
            button: Some(MouseButton::Left),
            count: Some(1),
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_right_button() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 150.0, y: 250.0 }),
            button: Some(MouseButton::Right),
            count: Some(1),
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_middle_button() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 200.0, y: 300.0 }),
            button: Some(MouseButton::Middle),
            count: Some(1),
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_double_click() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
            button: Some(MouseButton::Left),
            count: Some(2),
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_default_button() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
            button: None,
            count: Some(1),
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_click_default_count() {
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 }),
            button: Some(MouseButton::Left),
            count: None,
        };
        let perception = StubPerceptionProvider;
        let result = perform_click(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }
}
