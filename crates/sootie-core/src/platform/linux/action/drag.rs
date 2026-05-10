use std::process::Command;

use crate::action::{ActionError, ActionResult, DragAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_drag<P: PerceptionProvider>(
    action: &DragAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (from, from_backend) = resolve_target_with_cascade(perception, &action.from).await?;
    let (to, to_backend) = resolve_target_with_cascade(perception, &action.to).await?;

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

    let backend_used = if matches!(from_backend, Some(crate::cascade::Backend::Vision))
        || matches!(to_backend, Some(crate::cascade::Backend::Vision))
    {
        "vision+xdotool"
    } else {
        "xdotool"
    };
    Ok(ActionResult::success(None, backend_used))
}
