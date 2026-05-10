use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

use crate::action::{ActionError, ActionResult, TypeAction};
use crate::cascade::resolve_target_with_cascade;
use crate::perception::PerceptionProvider;

use super::keyboard::type_text;

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    let mut backend_used = "win32";
    if let Some(ref target) = action.target {
        let (coord, backend) = resolve_target_with_cascade(perception, target).await?;
        let (x, y) = (coord.x as i32, coord.y as i32);
        if backend == Some(crate::cascade::Backend::Vision) {
            backend_used = "vision+win32";
        }

        unsafe {
            let _ = SetCursorPos(x, y);
            mouse_event(MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP, 0, 0, 0, 0);
        }
    }

    if action.clear_first.unwrap_or(false) {
        unsafe {
            keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_A.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(VK_A.0 as u8, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
            keybd_event(
                VK_CONTROL.0 as u8,
                0,
                KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                0,
            );
        }
    }

    type_text(&action.text).map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, backend_used))
}
