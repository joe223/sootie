use crate::action::{ActionError, ActionResult, TypeAction};
use crate::perception::PerceptionProvider;

use super::mouse::simulate_mouse_move;
use super::mouse::simulate_click;
use super::keyboard::simulate_type;
use super::hotkey::simulate_hotkey;
use super::utils::resolve_target;

pub async fn perform_type<P: PerceptionProvider>(
    action: &TypeAction,
    perception: &P,
) -> Result<ActionResult, ActionError> {
    if let Some(ref target) = action.target {
        let coord = resolve_target(target, perception).await?;
        simulate_mouse_move(coord.x, coord.y)
            .map_err(|e| ActionError::ActionFailed(e))?;
        simulate_click(coord.x, coord.y, "left", 1)
            .map_err(|e| ActionError::ActionFailed(e))?;
    }

    if action.clear_first.unwrap_or(false) {
        simulate_hotkey(&["Cmd".to_string(), "A".to_string()])
            .map_err(|e| ActionError::ActionFailed(e))?;
        
        use core_graphics::event::{CGEvent, CGEventTapLocation};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
        
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .map_err(|_| ActionError::ActionFailed("Failed to create event source".to_string()))?;
        
        let delete_event = CGEvent::new_keyboard_event(source.clone(), 51, true)
            .map_err(|_| ActionError::ActionFailed("Failed to create delete event".to_string()))?;
        delete_event.post(CGEventTapLocation::HID);
        
        let delete_up = CGEvent::new_keyboard_event(source.clone(), 51, false)
            .map_err(|_| ActionError::ActionFailed("Failed to create delete up event".to_string()))?;
        delete_up.post(CGEventTapLocation::HID);
    }

    simulate_type(&action.text)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}