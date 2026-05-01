use std::process::Command;

use crate::action::{ActionError, ActionResult, ActionTarget, HoverAction};
use crate::perception::PerceptionProvider;

pub async fn perform_hover<P: PerceptionProvider>(
    action: &HoverAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (x, y) = match &action.target {
        ActionTarget::Coordinate(coord) => (coord.x, coord.y),
        ActionTarget::Selector(selector) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            (cx, cy)
        }
    };

    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(x.to_string())
        .arg(y.to_string())
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseMove failed: {}", e)))?;

    Ok(ActionResult::success(None, "xdotool"))
}