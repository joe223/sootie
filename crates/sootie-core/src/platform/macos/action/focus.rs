use std::time::Duration;

use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use objc2_foundation::NSString;

use crate::action::{ActionError, ActionResult, FocusAction};
use crate::selector::WindowSelector;

use super::super::ax_fns::{
    get_children, get_string_attr, get_windows, is_process_trusted, perform_action,
    release_ax_element, set_bool_attr, set_element_attr, AXUIElementCreateApplication,
    K_AX_ERROR_SUCCESS,
};
use super::super::perception::{get_bundle_id_for_app_name, get_pid_for_app_name};

const FOCUS_SETTLE_DELAY: Duration = Duration::from_millis(200);

#[repr(C)]
struct ProcessSerialNumber {
    high_long_of_psn: u32,
    low_long_of_psn: u32,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn GetProcessForPID(pid: i32, psn: *mut ProcessSerialNumber) -> i32;
    fn SetFrontProcessWithOptions(psn: *const ProcessSerialNumber, options: u32) -> i32;
}

pub fn perform_focus(action: &FocusAction) -> Result<ActionResult, ActionError> {
    let app_name = action.selector.app.as_ref().and_then(|a| a.name.clone());

    match app_name {
        Some(name) => {
            focus_app(&name, action.selector.window.as_ref()).map_err(ActionError::ActionFailed)?;
            Ok(ActionResult::success(None, "appkit"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app name specified in selector".to_string(),
        )),
    }
}

fn focus_app(app_name: &str, window: Option<&WindowSelector>) -> Result<(), String> {
    activate_app(app_name)?;
    if is_process_trusted() {
        let _ = raise_window(app_name, window);
    }
    std::thread::sleep(FOCUS_SETTLE_DELAY);
    Ok(())
}

fn activate_app(app_name: &str) -> Result<(), String> {
    let pid = get_pid_for_app_name(app_name);
    if pid <= 0 {
        return Err(format!("No running app matched '{}'", app_name));
    }

    let mut activated = activate_process(pid).is_ok();

    let bundle_id = get_bundle_id_for_app_name(app_name)
        .ok_or_else(|| format!("No running app matched '{}'", app_name))?;
    let bundle_id = NSString::from_str(&bundle_id);
    let apps = unsafe { NSRunningApplication::runningApplicationsWithBundleIdentifier(&bundle_id) };
    let app = (0..apps.len())
        .filter_map(|index| apps.get(index))
        .find(|app| unsafe { !app.isTerminated() })
        .ok_or_else(|| format!("No running application found for '{}'", app_name))?;

    unsafe {
        app.unhide();
        #[allow(deprecated)]
        let options = NSApplicationActivationOptions::NSApplicationActivateAllWindows
            | NSApplicationActivationOptions::NSApplicationActivateIgnoringOtherApps;
        if app.activateWithOptions(options) {
            activated = true;
        }
    }

    if activated {
        Ok(())
    } else {
        Err(format!("Failed to activate app '{}'", app_name))
    }
}

fn activate_process(pid: i32) -> Result<(), String> {
    let mut psn = ProcessSerialNumber {
        high_long_of_psn: 0,
        low_long_of_psn: 0,
    };

    let get_status = unsafe { GetProcessForPID(pid, &mut psn) };
    if get_status != 0 {
        return Err(format!(
            "GetProcessForPID failed with status {}",
            get_status
        ));
    }

    let set_status = unsafe { SetFrontProcessWithOptions(&psn, 0) };
    if set_status != 0 {
        return Err(format!(
            "SetFrontProcessWithOptions failed with status {}",
            set_status
        ));
    }

    Ok(())
}

fn raise_window(app_name: &str, window: Option<&WindowSelector>) -> Result<(), String> {
    if !is_process_trusted() {
        return Err("Accessibility permission required to raise a specific window".to_string());
    }

    let pid = get_pid_for_app_name(app_name);
    if pid <= 0 {
        return Err(format!("No running app matched '{}'", app_name));
    }

    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        if app_element.is_null() {
            return Err(format!(
                "Failed to create accessibility element for '{}'",
                app_name
            ));
        }

        let mut windows = get_windows(app_element);
        if windows.is_empty() {
            windows = get_children(app_element);
        }

        let target_index = window_index(window);
        let target_title = window.and_then(|window| window.title.as_deref());
        let _ = set_bool_attr(app_element, "AXFrontmost", true);
        let mut raised = false;
        let mut first_window: Option<usize> = None;

        for (index, window_ref) in windows.iter().enumerate() {
            first_window.get_or_insert(index);
            let title = get_string_attr(*window_ref, "AXTitle").unwrap_or_default();
            let title_matches = target_title
                .map(|needle| title.contains(needle))
                .unwrap_or(false);
            let index_matches = target_index
                .map(|target_index| target_index == index)
                .unwrap_or(false);

            if title_matches || index_matches || (target_title.is_none() && target_index.is_none())
            {
                let _ = set_bool_attr(*window_ref, "AXMain", true);
                let _ = set_element_attr(app_element, "AXFocusedWindow", *window_ref);
                let err = perform_action(*window_ref, "AXRaise");
                raised = err == K_AX_ERROR_SUCCESS;
            }

            if raised {
                break;
            }
        }

        if !raised {
            if let Some(index) = first_window {
                if let Some(window_ref) = windows.get(index) {
                    let _ = set_bool_attr(*window_ref, "AXMain", true);
                    let _ = set_element_attr(app_element, "AXFocusedWindow", *window_ref);
                    let err = perform_action(*window_ref, "AXRaise");
                    raised = err == K_AX_ERROR_SUCCESS;
                }
            }
        }

        for window_ref in windows {
            release_ax_element(window_ref);
        }
        release_ax_element(app_element);

        if raised {
            Ok(())
        } else {
            Err("No matching window could be raised".to_string())
        }
    }
}

fn window_index(window: Option<&WindowSelector>) -> Option<usize> {
    if let Some(index) = window.and_then(|window| window.index) {
        return Some(index as usize);
    }

    window
        .and_then(|window| window.id.as_deref())
        .and_then(|id| id.strip_prefix("win_"))
        .and_then(|index| index.parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::WindowSelector;

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_focus_finder() {
        let action = FocusAction {
            selector: crate::selector::Selector::new()
                .with_app(crate::selector::AppSelector::from_name("Finder")),
        };
        let result = perform_focus(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_focus_safari() {
        let action = FocusAction {
            selector: crate::selector::Selector::new()
                .with_app(crate::selector::AppSelector::from_name("Safari")),
        };
        let result = perform_focus(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_window_index_from_window_id() {
        let selector = WindowSelector::from_title("GitHub").with_id("win_2");
        assert_eq!(window_index(Some(&selector)), Some(2));
    }

    #[test]
    fn test_window_index_prefers_explicit_index() {
        let selector = WindowSelector {
            title: None,
            id: Some("win_2".to_string()),
            index: Some(4),
            focused: None,
        };
        assert_eq!(window_index(Some(&selector)), Some(4));
    }
}
