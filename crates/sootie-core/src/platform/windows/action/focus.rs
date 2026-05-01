use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::Foundation::*;

use crate::action::{ActionError, ActionResult, FocusAction};

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let app_name = action
        .selector
        .app
        .as_ref()
        .and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            unsafe {
                let hwnd = FindWindowW(None, windows::core::PCWSTR(
                    name.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>().as_ptr()
                ));

                if hwnd == HWND(0) {
                    return Err(ActionError::ActionFailed(format!("Window '{}' not found", name)));
                }

                SetForegroundWindow(hwnd);
            }

            Ok(ActionResult::success(None, "win32"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}