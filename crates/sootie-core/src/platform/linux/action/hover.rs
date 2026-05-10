use std::process::Command;

use crate::action::{ActionError, ActionResult, HoverAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_hover<P: PerceptionProvider>(
    action: &HoverAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (coord, backend) = resolve_target_with_cascade(perception, &action.target).await?;
    let (x, y) = (coord.x, coord.y);

    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(x.to_string())
        .arg(y.to_string())
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseMove failed: {}", e)))?;

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+xdotool",
        _ => "xdotool",
    };
    Ok(ActionResult::success(None, backend_used))
}
