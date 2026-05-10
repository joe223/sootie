use std::process::Command;

use crate::action::{ActionError, ActionResult, ScrollAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_scroll<P: PerceptionProvider>(
    action: &ScrollAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (x, y, backend) = match &action.target {
        Some(target) => {
            let (coord, backend) = resolve_target_with_cascade(perception, target).await?;
            (coord.x, coord.y, backend)
        }
        None => (0.0, 0.0, None),
    };

    let direction_arg = match action.direction {
        crate::action::ScrollDirection::Up => "4",
        crate::action::ScrollDirection::Down => "5",
        crate::action::ScrollDirection::Left => "6",
        crate::action::ScrollDirection::Right => "7",
    };

    let amount = action.amount.unwrap_or(3);

    Command::new("xdotool")
        .arg("mousemove")
        .arg("--screen")
        .arg("0")
        .arg(x.to_string())
        .arg(y.to_string())
        .output()
        .map_err(|e| ActionError::ActionFailed(format!("MouseMove failed: {}", e)))?;

    for _ in 0..amount {
        Command::new("xdotool")
            .arg("click")
            .arg(direction_arg)
            .output()
            .map_err(|e| ActionError::ActionFailed(format!("Scroll click failed: {}", e)))?;
    }

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+xdotool",
        _ => "xdotool",
    };
    Ok(ActionResult::success(None, backend_used))
}
