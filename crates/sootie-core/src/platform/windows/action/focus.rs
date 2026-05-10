use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::{ActionError, ActionResult, FocusAction};

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let hwnd = super::resolver::resolve_window(&action.selector)?;
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
    Ok(ActionResult::success(None, "win32"))
}
