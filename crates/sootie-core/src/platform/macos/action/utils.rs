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