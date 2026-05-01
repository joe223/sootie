use crate::action::{ActionError, ActionResult, HotkeyAction};

pub fn perform_hotkey(action: &HotkeyAction) -> Result<ActionResult, ActionError> {
    simulate_hotkey(&action.keys)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}

pub fn simulate_hotkey(keys: &[String]) -> Result<(), String> {
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    let mut flags = CGEventFlags::CGEventFlagNull;

    for key in keys.iter().take(keys.len().saturating_sub(1)) {
        match key.to_lowercase().as_str() {
            "cmd" | "command" => {
                flags |= CGEventFlags::CGEventFlagCommand;
            }
            "shift" => {
                flags |= CGEventFlags::CGEventFlagShift;
            }
            "alt" | "option" => {
                flags |= CGEventFlags::CGEventFlagAlternate;
            }
            "ctrl" | "control" => {
                flags |= CGEventFlags::CGEventFlagControl;
            }
            _ => {}
        }
    }

    let last_key = keys.last().ok_or("No keys provided")?;
    let keycode = super::keyboard::map_key_to_code(last_key);

    let down_event = CGEvent::new_keyboard_event(source.clone(), keycode, true)
        .map_err(|_| "Failed to create key down event".to_string())?;
    down_event.set_flags(flags);
    down_event.post(CGEventTapLocation::HID);

    let up_event = CGEvent::new_keyboard_event(source.clone(), keycode, false)
        .map_err(|_| "Failed to create key up event".to_string())?;
    up_event.set_flags(flags);
    up_event.post(CGEventTapLocation::HID);

    Ok(())
}