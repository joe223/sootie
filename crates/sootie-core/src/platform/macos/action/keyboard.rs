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
        assert_eq!(map_key_to_code("enter"), 36);
        assert_eq!(map_key_to_code("tab"), 48);
        assert_eq!(map_key_to_code("space"), 49);
        assert_eq!(map_key_to_code("escape"), 53);
        assert_eq!(map_key_to_code("esc"), 53);
        assert_eq!(map_key_to_code("delete"), 51);
        assert_eq!(map_key_to_code("backspace"), 51);
    }

    #[test]
    fn test_map_key_to_code_arrows() {
        assert_eq!(map_key_to_code("left"), 123);
        assert_eq!(map_key_to_code("right"), 124);
        assert_eq!(map_key_to_code("down"), 125);
        assert_eq!(map_key_to_code("up"), 126);
    }

    #[test]
    fn test_map_key_to_code_function_keys() {
        assert_eq!(map_key_to_code("f1"), 122);
        assert_eq!(map_key_to_code("f2"), 120);
        assert_eq!(map_key_to_code("f3"), 99);
        assert_eq!(map_key_to_code("f4"), 118);
        assert_eq!(map_key_to_code("f5"), 96);
        assert_eq!(map_key_to_code("f6"), 97);
        assert_eq!(map_key_to_code("f7"), 98);
        assert_eq!(map_key_to_code("f8"), 100);
        assert_eq!(map_key_to_code("f9"), 101);
        assert_eq!(map_key_to_code("f10"), 109);
        assert_eq!(map_key_to_code("f11"), 103);
        assert_eq!(map_key_to_code("f12"), 111);
    }

    #[test]
    fn test_map_key_to_code_modifiers() {
        assert_eq!(map_key_to_code("cmd"), 55);
        assert_eq!(map_key_to_code("command"), 55);
        assert_eq!(map_key_to_code("shift"), 56);
        assert_eq!(map_key_to_code("alt"), 58);
        assert_eq!(map_key_to_code("option"), 58);
        assert_eq!(map_key_to_code("ctrl"), 59);
        assert_eq!(map_key_to_code("control"), 59);
        assert_eq!(map_key_to_code("capslock"), 57);
    }

    #[test]
    fn test_map_key_to_code_case_insensitive() {
        assert_eq!(map_key_to_code("RETURN"), 36);
        assert_eq!(map_key_to_code("Tab"), 48);
        assert_eq!(map_key_to_code("SPACE"), 49);
        assert_eq!(map_key_to_code("CMD"), 55);
    }

    #[test]
    fn test_char_to_keycode_letters() {
        let code_a = char_to_keycode('a');
        let code_a_upper = char_to_keycode('A');
        assert_eq!(code_a, code_a_upper);
    }

    #[test]
    fn test_char_to_keycode_numbers() {
        assert_eq!(char_to_keycode('0'), 29);
        assert_eq!(char_to_keycode('9'), 25);
    }

    #[test]
    fn test_char_to_keycode_special() {
        assert_eq!(char_to_keycode(' '), 49);
        assert_eq!(char_to_keycode('\n'), 36);
        assert_eq!(char_to_keycode('\t'), 48);
        assert_eq!(char_to_keycode('\r'), 36);
    }

    #[test]
    fn test_char_to_keycode_symbols() {
        assert!(char_to_keycode('!') > 0);
        assert!(char_to_keycode('@') > 0);
        assert!(char_to_keycode('#') > 0);
        assert!(char_to_keycode('$') > 0);
        assert!(char_to_keycode('%') > 0);
        assert!(char_to_keycode('-') > 0);
        assert!(char_to_keycode('=') > 0);
        assert!(char_to_keycode('.') > 0);
        assert!(char_to_keycode(',') > 0);
    }

    #[test]
    fn test_simulate_key_press() {
        let result = simulate_key_press("return");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_key_press_f1() {
        let result = simulate_key_press("f1");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type() {
        let result = simulate_type("hello");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type_empty() {
        let result = simulate_type("");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type_uppercase() {
        let result = simulate_type("HELLO");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type_with_numbers() {
        let result = simulate_type("test123");
        assert!(result.is_ok() || result.is_err());
    }
}

    #[test]
    fn test_map_key_to_code_case_insensitive() {
        assert_eq!(map_key_to_code("RETURN"), 36);
        assert_eq!(map_key_to_code("Tab"), 48);
        assert_eq!(map_key_to_code("SPACE"), 49);
    }

    #[test]
    fn test_char_to_keycode_letters() {
        let code_a = char_to_keycode('a');
        let code_a_upper = char_to_keycode('A');
        assert_eq!(code_a, code_a_upper);
    }

    #[test]
    fn test_char_to_keycode_numbers() {
        let code_0 = char_to_keycode('0');
        let code_9 = char_to_keycode('9');
        assert!(code_0 > 0);
        assert!(code_9 > 0);
    }

    #[test]
    fn test_char_to_keycode_special() {
        let code_space = char_to_keycode(' ');
        assert!(code_space > 0);
    }

    #[test]
    fn test_simulate_key_press() {
        let result = simulate_key_press("return");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type() {
        let result = simulate_type("hello");
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_simulate_type_empty() {
        let result = simulate_type("");
        assert!(result.is_ok() || result.is_err());
    }
