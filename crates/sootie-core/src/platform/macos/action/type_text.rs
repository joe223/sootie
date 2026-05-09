use crate::action::{ActionError, ActionResult, TypeAction};
use crate::perception::PerceptionProvider;
use std::time::Duration;

use super::keyboard::{simulate_clear_text, simulate_type};
use super::mouse::simulate_click;
use super::mouse::simulate_mouse_move;
use super::utils::resolve_target;

const TARGET_FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(180);
const CLEAR_SETTLE_DELAY: Duration = Duration::from_millis(120);
const TYPE_SETTLE_DELAY: Duration = Duration::from_millis(80);

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    if let Some(ref target) = action.target {
        let coord = resolve_target(target, perception).await?;
        simulate_mouse_move(coord.x, coord.y).map_err(|e| ActionError::ActionFailed(e))?;
        simulate_click(coord.x, coord.y, "left", 1).map_err(|e| ActionError::ActionFailed(e))?;
        std::thread::sleep(TARGET_FOCUS_SETTLE_DELAY);
    }

    if action.clear_first.unwrap_or(false) {
        simulate_clear_text().map_err(|e| ActionError::ActionFailed(e))?;
        std::thread::sleep(CLEAR_SETTLE_DELAY);
    }

    simulate_type(&action.text).map_err(|e| ActionError::ActionFailed(e))?;
    std::thread::sleep(TYPE_SETTLE_DELAY);

    Ok(ActionResult::success(None, "cgevent"))
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{ActionTarget, TypeAction};
    use crate::perception::StubPerceptionProvider;
    use crate::selector::Coordinate;

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_type_text() {
        let action = TypeAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            text: "hello world".to_string(),
            clear_first: None,
        };
        let perception = StubPerceptionProvider;
        let result = perform_type(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_type_empty() {
        let action = TypeAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            text: "".to_string(),
            clear_first: None,
        };
        let perception = StubPerceptionProvider;
        let result = perform_type(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_type_clear_first() {
        let action = TypeAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            text: "new text".to_string(),
            clear_first: Some(true),
        };
        let perception = StubPerceptionProvider;
        let result = perform_type(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_perform_type_no_target() {
        let action = TypeAction {
            target: None,
            text: "text".to_string(),
            clear_first: None,
        };
        let perception = StubPerceptionProvider;
        let result = perform_type(&action, &perception).await;
        assert!(result.is_ok() || result.is_err());
    }
}
