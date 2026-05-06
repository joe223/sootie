use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::{ActionError, ActionResult, ClickAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

pub async fn perform_click<P: PerceptionProvider>(
    action: &ClickAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let (coord, backend) = resolve_target_with_cascade(perception, &action.target).await?;
    let (x, y) = (coord.x as i32, coord.y as i32);

    unsafe {
        SetCursorPos(x, y);

        let button_flag = match action.button {
            Some(crate::action::MouseButton::Right) => MOUSEEVENTF_RIGHTDOWN | MOUSEEVENTF_RIGHTUP,
            Some(crate::action::MouseButton::Middle) => {
                MOUSEEVENTF_MIDDLEDOWN | MOUSEEVENTF_MIDDLEUP
            }
            _ => MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP,
        };

        for _ in 0..action.count.unwrap_or(1) {
            mouse_event(button_flag, 0, 0, 0, None);
        }
    }

    let backend_used = match backend {
        Some(crate::cascade::Backend::Vision) => "vision+win32",
        _ => "win32",
    };
    Ok(ActionResult::success(None, backend_used))
}
