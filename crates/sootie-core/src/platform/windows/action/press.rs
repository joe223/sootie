use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

use crate::action::{ActionError, ActionResult, PressAction};

use super::keyboard::map_key_to_vk;

pub fn perform_press(action: &PressAction) -> Result<ActionResult, ActionError> {
    let vk = map_key_to_vk(&action.key);

    unsafe {
        keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, None);
        keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);
    }

    Ok(ActionResult::success(None, "win32"))
}
