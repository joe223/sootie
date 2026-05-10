use crate::action::{ActionError, ActionResult, ClickAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

use super::mouse::click_at;

pub async fn perform_click<P: PerceptionProvider>(
    action: &ClickAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (coord, backend) = resolve_target_with_cascade(perception, &action.target).await?;
    let (x, y) = (coord.x, coord.y);

    let button = match action.button {
        Some(crate::action::MouseButton::Right) => 3,
        Some(crate::action::MouseButton::Middle) => 2,
        _ => 1,
    };

    click_at(x, y, button, action.count.unwrap_or(1)).map_err(|e| ActionError::ActionFailed(e))?;

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+xdotool",
        _ => "xdotool",
    };
    Ok(ActionResult::success(None, backend_used))
}
