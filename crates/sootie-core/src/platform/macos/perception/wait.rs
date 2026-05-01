use std::time::{Duration, Instant};

use crate::perception::{PerceptionError, WaitCondition, WaitResult};
use crate::selector::Selector;

use super::find::find_elements;

pub fn wait_for_element(
    selector: &Selector,
    condition: &WaitCondition,
) -> Result<WaitResult, PerceptionError> {
    let start = Instant::now();
    let timeout = Duration::from_millis(condition.timeout_ms);

    loop {
        let result = find_elements(selector)?;

        if !result.elements.is_empty() {
            let element = &result.elements[0];

            let want_visible = condition.state.get("visible").and_then(|v| v.as_bool());
            let want_enabled = condition.state.get("enabled").and_then(|v| v.as_bool());
            let want_focused = condition.state.get("focused").and_then(|v| v.as_bool());

            let visible_ok = want_visible.map_or(true, |v| element.state.visible == v);
            let enabled_ok = want_enabled.map_or(true, |v| element.state.enabled == Some(v));
            let focused_ok = want_focused.map_or(true, |v| element.state.focused == Some(v));

            if visible_ok && enabled_ok && focused_ok {
                return Ok(WaitResult {
                    matched: true,
                    element: Some(element.clone()),
                    timed_out: false,
                });
            }
        }

        if start.elapsed() >= timeout {
            return Ok(WaitResult {
                matched: false,
                element: None,
                timed_out: true,
            });
        }

        std::thread::sleep(Duration::from_millis(100));
    }
}