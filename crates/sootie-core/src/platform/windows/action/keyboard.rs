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
        "a" => VK_A.0 as u8,
        "b" => VK_B.0 as u8,
        "c" => VK_C.0 as u8,
        "d" => VK_D.0 as u8,
        "e" => VK_E.0 as u8,
        "f" => VK_F.0 as u8,
        "g" => VK_G.0 as u8,
        "h" => VK_H.0 as u8,
        "i" => VK_I.0 as u8,
        "j" => VK_J.0 as u8,
        "k" => VK_K.0 as u8,
        "l" => VK_L.0 as u8,
        "m" => VK_M.0 as u8,
        "n" => VK_N.0 as u8,
        "o" => VK_O.0 as u8,
        "p" => VK_P.0 as u8,
        "q" => VK_Q.0 as u8,
        "r" => VK_R.0 as u8,
        "s" => VK_S.0 as u8,
        "t" => VK_T.0 as u8,
        "u" => VK_U.0 as u8,
        "v" => VK_V.0 as u8,
        "w" => VK_W.0 as u8,
        "x" => VK_X.0 as u8,
        "y" => VK_Y.0 as u8,
        "z" => VK_Z.0 as u8,
        "0" => VK_0.0 as u8,
        "1" => VK_1.0 as u8,
        "2" => VK_2.0 as u8,
        "3" => VK_3.0 as u8,
        "4" => VK_4.0 as u8,
        "5" => VK_5.0 as u8,
        "6" => VK_6.0 as u8,
        "7" => VK_7.0 as u8,
        "8" => VK_8.0 as u8,
        "9" => VK_9.0 as u8,
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
                keybd_event(VK_SHIFT.0 as u8, 0, KEYEVENTF_EXTENDEDKEY, 0);
            }

            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);

            if needs_shift {
                keybd_event(
                    VK_SHIFT.0 as u8,
                    0,
                    KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP,
                    0,
                );
            }
        }
    }

    Ok(())
}

fn char_to_vk(ch: char) -> u8 {
    match ch {
        'a'..='z' => VK_A.0 as u8 + (ch as u8 - b'a'),
        'A'..='Z' => VK_A.0 as u8 + (ch as u8 - b'A'),
        '0'..='9' => VK_0.0 as u8 + (ch as u8 - b'0'),
        ' ' => VK_SPACE.0 as u8,
        '\n' | '\r' => VK_RETURN.0 as u8,
        '\t' => VK_TAB.0 as u8,
        _ => 0,
    }
}
