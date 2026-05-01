use std::process::Command;

use crate::action::{ActionError, ActionResult, TypeAction};
use crate::perception::PerceptionProvider;

use super::mouse::click_at;
use super::keyboard::type_text;

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    use crate::action::ActionTarget;

    if let Some(ref target) = action.target {
        let (x, y) = match target {
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

        click_at(x, y, 1, 1)
            .map_err(|e| ActionError::ActionFailed(e))?;
    }

    if action.clear_first.unwrap_or(false) {
        Command::new("xdotool")
            .arg("key")
            .arg("Ctrl+A")
            .output()
            .map_err(|e| ActionError::ActionFailed(format!("Ctrl+A failed: {}", e)))?;
        
        Command::new("xdotool")
            .arg("key")
            .arg("Delete")
            .output()
            .map_err(|e| ActionError::ActionFailed(format!("Delete failed: {}", e)))?;
    }

    type_text(&action.text)
        .map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, "xdotool"))
}