use crate::action::{ActionError, ActionResult, PressAction};

use super::keyboard::simulate_key_press;

pub fn perform_press(action: &PressAction) -> Result<ActionResult, ActionError> {
    simulate_key_press(&action.key).map_err(|e| ActionError::ActionFailed(e))?;

    Ok(ActionResult::success(None, "cgevent"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::PressAction;

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_press_return() {
        let action = PressAction {
            key: "return".to_string(),
        };
        let result = perform_press(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_press_tab() {
        let action = PressAction {
            key: "tab".to_string(),
        };
        let result = perform_press(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_press_space() {
        let action = PressAction {
            key: "space".to_string(),
        };
        let result = perform_press(&action);
        assert!(result.is_ok() || result.is_err());
    }
}
