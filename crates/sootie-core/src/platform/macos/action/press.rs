use crate::action::{ActionError, ActionResult, PressAction};

use super::keyboard::simulate_key_press;

pub fn perform_press(action: &PressAction) -> Result<ActionResult, ActionError> {
    simulate_key_press(&action.key)
        .map_err(|e| ActionError::ActionFailed(e))?;
    
    Ok(ActionResult::success(None, "cgevent"))
}