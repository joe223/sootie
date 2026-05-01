use async_trait::async_trait;
use tracing::debug;

use crate::action::{
    ActionError, ActionResult, ActionTarget, ClickAction, DragAction, FocusAction, HotkeyAction,
    HoverAction, PressAction, ScrollAction, TypeAction, WindowAction,
};
use crate::selector::Coordinate;

pub struct MacActionProvider;

impl MacActionProvider {
    pub fn new() -> Self {
        Self
    }

    fn simulate_click(&self, x: f64, y: f64, button: &str, count: u32) -> Result<(), String> {
        debug!("Simulating {} click at ({}, {}) x{}", button, x, y, count);

        use core_graphics::event::{
            CGEvent, CGEventTapLocation, CGEventType, CGMouseButton, EventField,
        };
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        let point = CGPoint::new(x, y);

        let event_type = match button {
            "right" => CGEventType::RightMouseUp,
            "middle" => CGEventType::OtherMouseUp,
            _ => CGEventType::LeftMouseUp,
        };

        let mouse_button = match button {
            "right" => CGMouseButton::Right,
            "middle" => CGMouseButton::Center,
            _ => CGMouseButton::Left,
        };

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create event source".to_string())?;

        for i in 0..count {
            let down_type = match button {
                "right" => CGEventType::RightMouseDown,
                "middle" => CGEventType::OtherMouseDown,
                _ => CGEventType::LeftMouseDown,
            };

            let down_event = CGEvent::new_mouse_event(
                source.clone(),
                down_type,
                point,
                mouse_button,
            )
            .map_err(|_| "Failed to create mouse down event".to_string())?;

            if i > 0 {
                down_event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, (i + 1) as i64);
            }

            down_event.post(CGEventTapLocation::HID);

            let up_event = CGEvent::new_mouse_event(
                source.clone(),
                event_type,
                point,
                mouse_button,
            )
            .map_err(|_| "Failed to create mouse up event".to_string())?;

            if i > 0 {
                up_event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, (i + 1) as i64);
            }

            up_event.post(CGEventTapLocation::HID);
        }

        Ok(())
    }

    fn simulate_key_press(&self, key: &str) -> Result<(), String> {
        debug!("Simulating key press: {}", key);

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

    fn simulate_hotkey(&self, keys: &[String]) -> Result<(), String> {
        debug!("Simulating hotkey: {:?}", keys);

        use core_graphics::event::{CGEvent, CGEventTapLocation};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create event source".to_string())?;

        let mut flags = core_graphics::event::CGEventFlags::CGEventFlagNull;

        for key in keys {
            match key.to_lowercase().as_str() {
                "cmd" | "command" => {
                    flags |= core_graphics::event::CGEventFlags::CGEventFlagCommand;
                }
                "shift" => {
                    flags |= core_graphics::event::CGEventFlags::CGEventFlagShift;
                }
                "alt" | "option" => {
                    flags |= core_graphics::event::CGEventFlags::CGEventFlagAlternate;
                }
                "ctrl" | "control" => {
                    flags |= core_graphics::event::CGEventFlags::CGEventFlagControl;
                }
                _ => {}
            }
        }

        let last_key = keys.last().ok_or("No keys provided")?;
        let keycode = map_key_to_code(last_key);

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

    fn simulate_scroll(&self, x: f64, y: f64, direction: &str, amount: u32) -> Result<(), String> {
        debug!("Simulating scroll {} at ({}, {}) x{}", direction, x, y, amount);

        use core_graphics::event::{CGEvent, CGEventTapLocation, EventField};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create event source".to_string())?;

        let (delta_x, delta_y) = match direction {
            "up" => (0, amount as i32),
            "down" => (0, -(amount as i32)),
            "left" => (amount as i32, 0),
            "right" => (-(amount as i32), 0),
            _ => (0, 0),
        };

        let event = CGEvent::new(source)
            .map_err(|_| "Failed to create scroll event".to_string())?;

        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, delta_y as i64);
        event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, delta_x as i64);

        event.post(CGEventTapLocation::HID);

        Ok(())
    }

    fn simulate_mouse_move(&self, x: f64, y: f64) -> Result<(), String> {
        debug!("Simulating mouse move to ({}, {})", x, y);

        use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        let point = CGPoint::new(x, y);
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create event source".to_string())?;

        let event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::MouseMoved,
            point,
            CGMouseButton::Left,
        )
        .map_err(|_| "Failed to create mouse move event".to_string())?;

        event.post(CGEventTapLocation::HID);

        Ok(())
    }

    fn simulate_drag(&self, from: Coordinate, to: Coordinate) -> Result<(), String> {
        debug!("Simulating drag from ({}, {}) to ({}, {})", from.x, from.y, to.x, to.y);

        use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        use core_graphics::geometry::CGPoint;

        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| "Failed to create event source".to_string())?;

        let from_point = CGPoint::new(from.x, from.y);
        let to_point = CGPoint::new(to.x, to.y);

        let down_event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseDown,
            from_point,
            CGMouseButton::Left,
        )
        .map_err(|_| "Failed to create mouse down event".to_string())?;
        down_event.post(CGEventTapLocation::HID);

        let move_event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseDragged,
            to_point,
            CGMouseButton::Left,
        )
        .map_err(|_| "Failed to create mouse drag event".to_string())?;
        move_event.post(CGEventTapLocation::HID);

        let up_event = CGEvent::new_mouse_event(
            source.clone(),
            CGEventType::LeftMouseUp,
            to_point,
            CGMouseButton::Left,
        )
        .map_err(|_| "Failed to create mouse up event".to_string())?;
        up_event.post(CGEventTapLocation::HID);

        Ok(())
    }
}

fn map_key_to_code(key: &str) -> u16 {
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

#[async_trait]
impl crate::action::ActionProvider for MacActionProvider {
    async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError> {
        let button = match &action.button {
            Some(crate::action::MouseButton::Right) => "right",
            Some(crate::action::MouseButton::Middle) => "middle",
            _ => "left",
        };
        let count = action.count.unwrap_or(1);

        match &action.target {
            ActionTarget::Coordinate(coord) => {
                self.simulate_click(coord.x, coord.y, button, count)
                    .map_err(|e| ActionError::ActionFailed(e))?;
                Ok(ActionResult::success(None, "cgevent"))
            }
            ActionTarget::Selector(_) => {
                Err(ActionError::NotImplemented(
                    "Selector-based click requires perception provider".to_string(),
                ))
            }
        }
    }

    async fn r#type(&self, _action: &TypeAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented(
            "macOS type not yet implemented".to_string(),
        ))
    }

    async fn press(&self, action: &PressAction) -> Result<ActionResult, ActionError> {
        self.simulate_key_press(&action.key)
            .map_err(|e| ActionError::ActionFailed(e))?;
        Ok(ActionResult::success(None, "cgevent"))
    }

    async fn hotkey(&self, action: &HotkeyAction) -> Result<ActionResult, ActionError> {
        self.simulate_hotkey(&action.keys)
            .map_err(|e| ActionError::ActionFailed(e))?;
        Ok(ActionResult::success(None, "cgevent"))
    }

    async fn scroll(&self, action: &ScrollAction) -> Result<ActionResult, ActionError> {
        let direction = match action.direction {
            crate::action::ScrollDirection::Up => "up",
            crate::action::ScrollDirection::Down => "down",
            crate::action::ScrollDirection::Left => "left",
            crate::action::ScrollDirection::Right => "right",
        };
        let amount = action.amount.unwrap_or(3);

        let (x, y) = match &action.target {
            Some(ActionTarget::Coordinate(coord)) => (coord.x, coord.y),
            _ => (0.0, 0.0),
        };

        self.simulate_scroll(x, y, direction, amount)
            .map_err(|e| ActionError::ActionFailed(e))?;
        Ok(ActionResult::success(None, "cgevent"))
    }

    async fn hover(&self, action: &HoverAction) -> Result<ActionResult, ActionError> {
        match &action.target {
            ActionTarget::Coordinate(coord) => {
                self.simulate_mouse_move(coord.x, coord.y)
                    .map_err(|e| ActionError::ActionFailed(e))?;
                Ok(ActionResult::success(None, "cgevent"))
            }
            ActionTarget::Selector(_) => Err(ActionError::NotImplemented(
                "Selector-based hover requires perception provider".to_string(),
            )),
        }
    }

    async fn drag(&self, action: &DragAction) -> Result<ActionResult, ActionError> {
        let from = match &action.from {
            ActionTarget::Coordinate(coord) => coord.clone(),
            _ => {
                return Err(ActionError::NotImplemented(
                    "Selector-based drag requires perception provider".to_string(),
                ))
            }
        };

        let to = match &action.to {
            ActionTarget::Coordinate(coord) => coord.clone(),
            _ => {
                return Err(ActionError::NotImplemented(
                    "Selector-based drag requires perception provider".to_string(),
                ))
            }
        };

        self.simulate_drag(from, to)
            .map_err(|e| ActionError::ActionFailed(e))?;
        Ok(ActionResult::success(None, "cgevent"))
    }

    async fn focus(&self, _action: &FocusAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented(
            "macOS focus not yet implemented".to_string(),
        ))
    }

    async fn window_op(&self, _action: &WindowAction) -> Result<ActionResult, ActionError> {
        Err(ActionError::NotImplemented(
            "macOS window operation not yet implemented".to_string(),
        ))
    }
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

    #[test]
    fn test_mac_action_provider_creation() {
        let provider = MacActionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }
}
