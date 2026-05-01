use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, HotkeyAction};

use super::keyboard::map_key_to_vk;

pub fn perform_hotkey(action: &HotkeyAction) -> Result<ActionResult, ActionError> {
    let vks: Vec<u8> = action.keys.iter().map(|k| map_key_to_vk(k)).collect();

    unsafe {
        for vk in &vks[..vks.len().saturating_sub(1)] {
            keybd_event(*vk, 0, KEYEVENTF_EXTENDEDKEY, None);
        }

        if let Some(last_vk) = vks.last() {
            keybd_event(*last_vk, 0, KEYEVENTF_EXTENDEDKEY, None);
            keybd_event(*last_vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);
        }

        for vk in &vks[..vks.len().saturating_sub(1)] {
            keybd_event(*vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);
        }
    }

    Ok(ActionResult::success(None, "win32"))
}