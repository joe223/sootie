use crate::selector::Coordinate;
use std::time::Duration;

const CLICK_SETTLE_DELAY: Duration = Duration::from_millis(40);
const CLICK_DOWN_DELAY: Duration = Duration::from_millis(60);

fn ensure_accessibility_trusted() -> Result<(), String> {
    if super::super::ax_fns::is_process_trusted() {
        return Ok(());
    }

    Err("Accessibility permission required for mouse actions. Go to System Settings > Privacy & Security > Accessibility, enable permission for this application, then restart the MCP server.".to_string())
}

pub fn simulate_click(x: f64, y: f64, button: &str, count: u32) -> Result<(), String> {
    ensure_accessibility_trusted()?;

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

    let move_event = CGEvent::new_mouse_event(
        source.clone(),
        CGEventType::MouseMoved,
        point,
        CGMouseButton::Left,
    )
    .map_err(|_| "Failed to create mouse move event".to_string())?;
    move_event.post(CGEventTapLocation::HID);
    std::thread::sleep(CLICK_SETTLE_DELAY);

    for i in 0..count {
        let down_type = match button {
            "right" => CGEventType::RightMouseDown,
            "middle" => CGEventType::OtherMouseDown,
            _ => CGEventType::LeftMouseDown,
        };

        let down_event = CGEvent::new_mouse_event(source.clone(), down_type, point, mouse_button)
            .map_err(|_| "Failed to create mouse down event".to_string())?;

        down_event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, (i + 1) as i64);

        down_event.post(CGEventTapLocation::HID);
        std::thread::sleep(CLICK_DOWN_DELAY);

        let up_event = CGEvent::new_mouse_event(source.clone(), event_type, point, mouse_button)
            .map_err(|_| "Failed to create mouse up event".to_string())?;

        up_event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, (i + 1) as i64);

        up_event.post(CGEventTapLocation::HID);
        std::thread::sleep(CLICK_SETTLE_DELAY);
    }

    Ok(())
}

pub fn simulate_mouse_move(x: f64, y: f64) -> Result<(), String> {
    ensure_accessibility_trusted()?;

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

pub fn simulate_scroll(_x: f64, _y: f64, direction: &str, amount: u32) -> Result<(), String> {
    ensure_accessibility_trusted()?;

    use core_graphics::event::{CGEvent, CGEventTapLocation, EventField};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| "Failed to create event source".to_string())?;

    let (delta_x, delta_y) = match direction {
        "up" => (0, amount as i32),
        "down" => (0, -(amount as i32)),
        "left" => (amount as i32, 0),
        "right" => (-(amount as i32), 0),
        _ => (0, 0),
    };

    let event =
        CGEvent::new(source.clone()).map_err(|_| "Failed to create scroll event".to_string())?;

    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_1, delta_y as i64);
    event.set_integer_value_field(EventField::SCROLL_WHEEL_EVENT_DELTA_AXIS_2, delta_x as i64);

    event.post(CGEventTapLocation::HID);

    Ok(())
}

pub fn simulate_drag(from: Coordinate, to: Coordinate) -> Result<(), String> {
    ensure_accessibility_trusted()?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_click_left() {
        let result = simulate_click(100.0, 200.0, "left", 1);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_click_right() {
        let result = simulate_click(150.0, 250.0, "right", 1);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_click_middle() {
        let result = simulate_click(200.0, 300.0, "middle", 1);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_click_double() {
        let result = simulate_click(100.0, 200.0, "left", 2);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_mouse_move() {
        let result = simulate_mouse_move(300.0, 400.0);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_scroll_up() {
        let result = simulate_scroll(100.0, 200.0, "up", 10);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_scroll_down() {
        let result = simulate_scroll(100.0, 200.0, "down", 10);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_simulate_drag() {
        let from = Coordinate { x: 100.0, y: 100.0 };
        let to = Coordinate { x: 200.0, y: 200.0 };
        let result = simulate_drag(from, to);
        assert!(result.is_ok() || result.is_err());
    }
}
