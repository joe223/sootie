use windows::Win32::Foundation::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub fn mouse_move(x: i32, y: i32) -> Result<(), String> {
    unsafe {
        SetCursorPos(x, y);
    }
    Ok(())
}

pub fn mouse_click(button: u8, count: u32) -> Result<(), String> {
    unsafe {
        let button_flag = match button {
            1 => MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP,
            2 => MOUSEEVENTF_MIDDLEDOWN | MOUSEEVENTF_MIDDLEUP,
            3 => MOUSEEVENTF_RIGHTDOWN | MOUSEEVENTF_RIGHTUP,
            _ => MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_LEFTUP,
        };

        for _ in 0..count {
            mouse_event(button_flag, 0, 0, 0, None);
        }
    }
    Ok(())
}
