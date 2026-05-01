use std::process::Command;

use crate::action::{ActionError, ActionResult, ActionTarget, DragAction};
use crate::perception::PerceptionProvider;

pub async fn perform_drag<P: PerceptionProvider>(
    action: &DragAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let from = match &action.from {
        ActionTarget::Coordinate(coord) => coord.clone(),
        ActionTarget::Selector(selector) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            crate::selector::Coordinate { x: cx, y: cy }
        }
    };

    let to = match &action.to {
        ActionTarget::Coordinate(coord) => coord.clone(),
        ActionTarget::Selector(selector) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            crate::selector::Coordinate { x: cx, y: cy }
        }
    };

    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(from.x.to_string())
        .arg(from.y.to_string())
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseMove failed: {}", e)))?;

    Command::new("xdotool")
        .arg("mousedown")
        .arg("1")
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseDown failed: {}", e)))?;

    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(to.x.to_string())
        .arg(to.y.to_string())
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseMove failed: {}", e)))?;

    Command::new("xdotool")
        .arg("mouseup")
        .arg("1")
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseUp failed: {}", e)))?;

    Ok(ActionResult::success(None, "xdotool"))
}