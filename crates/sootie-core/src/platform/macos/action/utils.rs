use crate::action::{ActionError, ActionTarget};
use crate::perception::PerceptionProvider;
use crate::selector::{Coordinate, Selector};

pub async fn resolve_target<P: PerceptionProvider>(
    target: &ActionTarget,
    perception: &P,
) -> Result<Coordinate, ActionError> {
    match target {
        ActionTarget::Coordinate(coord) => Ok(coord.clone()),
        ActionTarget::Selector(selector) => resolve_selector_to_coord(selector, perception).await,
    }
}

async fn resolve_selector_to_coord<P: PerceptionProvider>(
    selector: &Selector,
    perception: &P,
) -> Result<Coordinate, ActionError> {
    let result = perception
        .find(selector)
        .await
        .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;

    if result.elements.is_empty() {
        return Err(ActionError::TargetNotFound(
            "no element matches selector".to_string(),
        ));
    }

    let (cx, cy) = result.elements[0].bounds.center();
    Ok(Coordinate { x: cx, y: cy })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::ActionTarget;
    use crate::perception::StubPerceptionProvider;
    use crate::selector::{Coordinate, Selector};

    #[tokio::test]
    async fn test_resolve_target_coordinate() {
        let target = ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 });
        let perception = StubPerceptionProvider;
        let result = resolve_target(&target, &perception).await;
        assert!(result.is_ok());
        let coord = result.unwrap();
        assert_eq!(coord.x, 100.0);
        assert_eq!(coord.y, 200.0);
    }

    #[tokio::test]
    async fn test_resolve_target_selector_empty_elements() {
        let selector = Selector::new().with_name("NonExistent");
        let target = ActionTarget::Selector(selector);
        let perception = StubPerceptionProvider;
        let result = resolve_target(&target, &perception).await;
        assert!(result.is_err());
    }
}
