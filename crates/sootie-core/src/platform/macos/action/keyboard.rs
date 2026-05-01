pub fn map_key_to_code(key: &str) -> u16 {
    match key.to_lowercase().as_str() {
        "return" | "enter" => 36,
        "tab" => 48,
        "space" => 49,
        "delete" | "backspace" => 51,
        "escape" | "esc" => 53,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "a" => 0,
        "b" => 11,
        "c" => 8,
        "d" => 2,
        "e" => 14,
        "f" => 3,
        "g" => 5,
        "h" => 4,
        "i" => 34,
        "j" => 38,
        "k" => 40,
        "l" => 37,
        "m" => 46,
        "n" => 45,
        "o" => 31,
        "p" => 35,
        "q" => 12,
        "r" => 15,
        "s" => 1,
        "t" => 17,
        "u" => 32,
        "v" => 9,
        "w" => 13,
        "x" => 7,
        "y" => 16,
        "z" => 6,
        "0" => 29,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "5" => 23,
        "6" => 22,
        "7" => 26,
        "8" => 28,
        "9" => 25,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "cmd" | "command" => 55,
        "shift" => 56,
        "alt" | "option" => 58,
        "ctrl" | "control" => 59,
        "capslock" => 57,
        _ => 0,
    }
}

pub fn char_to_keycode(ch: char) -> u16 {
    match ch {
        'a'..='z' => (ch as u16) - ('a' as u16),
        'A'..='Z' => (ch as u16) - ('A' as u16),
        '0' => 29, '1' => 18, '2' => 19, '3' => 20, '4' => 21,
        '5' => 23, '6' => 22, '7' => 26, '8' => 28, '9' => 25,
        ' ' => 49,
        '\n' | '\r' => 36,
        '\t' => 48,
        '!' => 18, '@' => 19, '#' => 20, '$' => 21, '%' => 23,
        '^' => 22, '&' => 26, '*' => 28, '(' => 25, ')' => 29,
        '-' => 27, '=' => 24, '[' => 33, ']' => 30, '\\' => 42,
        ';' => 41, '\'' => 39, ',' => 43, '.' => 47, '/' => 44,
        '_' => 27, '+' => 24, '{' => 33, '}' => 30, '|' => 42,
        ':' => 41, '"' => 39, '<' => 43, '>' => 47, '?' => 44,
        '`' => 50, '~' => 50,
        _ => 0,
    }
}

pub fn simulate_key_press(key: &str) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let keycode = map_key_to_code(key);

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    let down_event = CGEvent::new_keyboard_event(source.clone(), keycode, true)
        .map_err(|_| "Failed to create key down event".to_string())?;
    down_event.post(CGEventTapLocation::HID);

    let up_event = CGEvent::new_keyboard_event(source.clone(), keycode, false)
        .map_err(|_| "Failed to create key up event".to_string())?;
    up_event.post(CGEventTapLocation::HID);

    Ok(())
}

pub fn simulate_type(text: &str) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    for ch in text.chars() {
        let keycode = char_to_keycode(ch);
        let needs_shift = ch.is_ascii_uppercase()
            || "!@#$%^&*()_+{}|:\"<>?~".contains(ch);

        let flags = if needs_shift {
            CGEventFlags::CGEventFlagShift
        } else {
            CGEventFlags::CGEventFlagNull
        };

        let down = CGEvent::new_keyboard_event(source.clone(), keycode, true)
            .map_err(|_| "Failed to create key down event".to_string())?;
        down.set_flags(flags);
        down.post(CGEventTapLocation::HID);

        let up = CGEvent::new_keyboard_event(source.clone(), keycode, false)
            .map_err(|_| "Failed to create key up event".to_string())?;
        up.set_flags(flags);
        up.post(CGEventTapLocation::HID);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_key_to_code() {
        assert_eq!(map_key_to_code("return"), 36);
        assert_eq!(map_key_to_code("tab"), 48);
        assert_eq!(map_key_to_code("space"), 49);
        assert_eq!(map_key_to_code("escape"), 53);
        assert_eq!(map_key_to_code("a"), 0);
        assert_eq!(map_key_to_code("z"), 6);
        assert_eq!(map_key_to_code("0"), 29);
        assert_eq!(map_key_to_code("f1"), 122);
    }
}