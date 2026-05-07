use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub fn map_key_to_vk(key: &str) -> u8 {
    match key.to_lowercase().as_str() {
        "return" | "enter" => VK_RETURN.0 as u8,
        "tab" => VK_TAB.0 as u8,
        "space" => VK_SPACE.0 as u8,
        "delete" | "backspace" => VK_BACK.0 as u8,
        "escape" | "esc" => VK_ESCAPE.0 as u8,
        "left" => VK_LEFT.0 as u8,
        "right" => VK_RIGHT.0 as u8,
        "down" => VK_DOWN.0 as u8,
        "up" => VK_UP.0 as u8,
        "a" => VK_KEY_A as u8,
        "b" => VK_KEY_B as u8,
        "c" => VK_KEY_C as u8,
        "d" => VK_KEY_D as u8,
        "e" => VK_KEY_E as u8,
        "f" => VK_KEY_F as u8,
        "g" => VK_KEY_G as u8,
        "h" => VK_KEY_H as u8,
        "i" => VK_KEY_I as u8,
        "j" => VK_KEY_J as u8,
        "k" => VK_KEY_K as u8,
        "l" => VK_KEY_L as u8,
        "m" => VK_KEY_M as u8,
        "n" => VK_KEY_N as u8,
        "o" => VK_KEY_O as u8,
        "p" => VK_KEY_P as u8,
        "q" => VK_KEY_Q as u8,
        "r" => VK_KEY_R as u8,
        "s" => VK_KEY_S as u8,
        "t" => VK_KEY_T as u8,
        "u" => VK_KEY_U as u8,
        "v" => VK_KEY_V as u8,
        "w" => VK_KEY_W as u8,
        "x" => VK_KEY_X as u8,
        "y" => VK_KEY_Y as u8,
        "z" => VK_KEY_Z as u8,
        "0" => VK_KEY_0 as u8,
        "1" => VK_KEY_1 as u8,
        "2" => VK_KEY_2 as u8,
        "3" => VK_KEY_3 as u8,
        "4" => VK_KEY_4 as u8,
        "5" => VK_KEY_5 as u8,
        "6" => VK_KEY_6 as u8,
        "7" => VK_KEY_7 as u8,
        "8" => VK_KEY_8 as u8,
        "9" => VK_KEY_9 as u8,
        "f1" => VK_F1.0 as u8,
        "f2" => VK_F2.0 as u8,
        "f3" => VK_F3.0 as u8,
        "f4" => VK_F4.0 as u8,
        "f5" => VK_F5.0 as u8,
        "f6" => VK_F6.0 as u8,
        "f7" => VK_F7.0 as u8,
        "f8" => VK_F8.0 as u8,
        "f9" => VK_F9.0 as u8,
        "f10" => VK_F10.0 as u8,
        "f11" => VK_F11.0 as u8,
        "f12" => VK_F12.0 as u8,
        "cmd" | "command" | "ctrl" | "control" => VK_CONTROL.0 as u8,
        "shift" => VK_SHIFT.0 as u8,
        "alt" | "option" => VK_MENU.0 as u8,
        "capslock" => VK_CAPITAL.0 as u8,
        _ => 0,
    }
}

pub fn type_text(text: &str) -> Result<(), String> {
    unsafe {
        for ch in text.chars() {
            let vk = char_to_vk(ch);
            let needs_shift = ch.is_ascii_uppercase() || "!@#$%^&*()_+{}|:\"<>?~".contains(ch);

            if needs_shift {
                keybd_event(VK_SHIFT.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, None);
            }

            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, None);
            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, None);

            if needs_shift {
                keybd_event(
                    VK_SHIFT.0 as u8,
                    0,
                    KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    None,
                );
            }
        }
    }

    Ok(())
}

fn char_to_vk(ch: char) -> u8 {
    match ch {
        'a'..='z' => VK_KEY_A as u8 + (ch as u8 - 'a' as u8),
        'A'..='Z' => VK_KEY_A as u8 + (ch as u8 - 'A' as u8),
        '0'..='9' => VK_KEY_0 as u8 + (ch as u8 - '0' as u8),
        ' ' => VK_SPACE.0 as u8,
        '\n' | '\r' => VK_RETURN.0 as u8,
        '\t' => VK_TAB.0 as u8,
        _ => 0,
    }
}
