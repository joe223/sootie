use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::{ActionError, ActionResult, WindowAction, WindowOperation};

pub fn perform_window_op(action: &WindowAction) -> Result<ActionResult, ActionError> {
    let hwnd = super::resolver::resolve_window(&action.selector)?;
    unsafe {
        match action.operation {
            WindowOperation::Minimize => {
                ShowWindow(hwnd, SW_MINIMIZE);
            }
            WindowOperation::Maximize => {
                ShowWindow(hwnd, SW_MAXIMIZE);
            }
            WindowOperation::Close => {
                PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            WindowOperation::Move { x, y } => {
                SetWindowPos(
                    hwnd,
                    HWND(0),
                    x as i32,
                    y as i32,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
            }
            WindowOperation::Resize { width, height } => {
                SetWindowPos(
                    hwnd,
                    HWND(0),
                    0,
                    0,
                    width as i32,
                    height as i32,
                    SWP_NOMOVE | SWP_NOZORDER,
                );
            }
        }
    }

    Ok(ActionResult::success(None, "win32"))
}
