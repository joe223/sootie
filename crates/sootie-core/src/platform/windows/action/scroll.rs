use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

use crate::action::{ActionError, ActionResult, ScrollAction, ScrollDirection};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_scroll<P: PerceptionProvider>(
    action: &ScrollAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (x, y, backend) = match &action.target {
        Some(target) => {
            let (coord, backend) = resolve_target_with_cascade(perception, target).await?;
            (coord.x as i32, coord.y as i32, backend)
        }
        None => (0, 0, None),
    };

    let amount = action.amount.unwrap_or(3);
    let delta = match action.direction {
        ScrollDirection::Up => amount as i32,
        ScrollDirection::Down => -(amount as i32),
        ScrollDirection::Left => amount as i32,
        ScrollDirection::Right => -(amount as i32),
    };

    unsafe {
        let _ = SetCursorPos(x, y);

        for _ in 0..amount {
            mouse_event(MOUSEEVENTF_WHEEL, 0, 0, delta, 0);
        }
    }

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+win32",
        _ => "win32",
    };
    Ok(ActionResult::success(None, backend_used))
}
