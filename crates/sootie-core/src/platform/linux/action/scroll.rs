use std::process::Command;

use crate::action::{ActionError, ActionResult, ActionTarget, ScrollAction};
use crate::perception::PerceptionProvider;

pub async fn perform_scroll<P: PerceptionProvider>(
    action: &ScrollAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (x, y) = match &action.target {
        Some(ActionTarget::Coordinate(coord)) => (coord.x, coord.y),
        Some(ActionTarget::Selector(selector)) => {
            let result = perception.find(selector).await
                .map_err(|e| ActionError::ActionFailed(format!("Find failed: {}", e)))?;
            if result.elements.is_empty() {
                return Err(ActionError::TargetNotFound("no element matches selector".to_string()));
            }
            let (cx, cy) = result.elements[0].bounds.center();
            (cx, cy)
        }
        None => (0.0, 0.0),
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

    Ok(ActionResult::success(None, "xdotool"))
}