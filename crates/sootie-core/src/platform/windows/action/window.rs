use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::{ActionError, ActionResult, WindowAction, WindowOperation};

pub fn perform_window_op(action: &WindowAction) -> Result<ActionResult, ActionError> {
    let hwnd = super::resolver::resolve_window(&action.selector)?;
    unsafe {
        match action.operation {
            WindowOperation::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            WindowOperation::Maximize => {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            WindowOperation::Close => {
                let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            WindowOperation::Move { x, y } => {
                let _ = SetWindowPos(
                    hwnd,
                    HWND(std::ptr::null_mut()),
                    x as i32,
                    y as i32,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER,
                );
            }
            WindowOperation::Resize { width, height } => {
                let _ = SetWindowPos(
                    hwnd,
                    HWND(std::ptr::null_mut()),
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
