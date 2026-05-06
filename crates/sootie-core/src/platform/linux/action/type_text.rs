use std::process::Command;

use crate::action::{ActionError, ActionResult, TypeAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

use super::keyboard::type_text;
use super::mouse::click_at;

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let mut backend_used = "xdotool";
    if let Some(ref target) = action.target {
        let (coord, backend) = resolve_target_with_cascade(perception, target).await?;
        let (x, y) = (coord.x, coord.y);
        if backend == Some(crate::cascade::Backend::Vision) {
            backend_used = "vision+xdotool";
        }

        click_at(x, y, 1, 1).map_err(|e| ActionError::ActionFailed(e))?;
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

    type_text(&action.text).map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, backend_used))
}
