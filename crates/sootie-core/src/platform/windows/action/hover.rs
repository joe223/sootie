use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::action::{ActionError, ActionResult, ActionTarget, HoverAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_hover<P: PerceptionProvider>(
    action: &HoverAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (coord, backend) = resolve_target_with_cascade(perception, &action.target).await?;
    let (x, y) = (coord.x as i32, coord.y as i32);

    unsafe {
        SetCursorPos(x, y);
    }

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+win32",
        _ => "win32",
    };
    Ok(ActionResult::success(None, backend_used))
}
