use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::action::{ActionError, ActionResult, ActionTarget, DragAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_drag<P: PerceptionProvider>(
    action: &DragAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (from, from_backend) = resolve_target_with_cascade(perception, &action.from).await?;
    let (to, to_backend) = resolve_target_with_cascade(perception, &action.to).await?;
    let from = (from.x as i32, from.y as i32);
    let to = (to.x as i32, to.y as i32);

    unsafe {
        SetCursorPos(from.0, from.1);
        mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, None);

        SetCursorPos(to.0, to.1);
        mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, None);
    }

    let backend_used = if matches!(from_backend, Some(crate::cascade::Backend::Vision))
        || matches!(to_backend, Some(crate::cascade::Backend::Vision))
    {
        "vision+win32"
    } else {
        "win32"
    };
    Ok(ActionResult::success(None, backend_used))
}
