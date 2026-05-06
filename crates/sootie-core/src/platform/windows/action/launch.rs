use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::{ActionError, ActionResult, LaunchAction};

pub fn perform_launch(action: &LaunchAction) -> Result<ActionResult, ActionError> {
    let app_identifier = action.app.name.or(action.app.bundle_id).clone();

    match app_identifier {
        Some(identifier) => {
            unsafe {
                let exe_path = identifier
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>();

                let mut args_string = identifier.clone();
                for arg in &action.args {
                    args_string.push(' ');
                    args_string.push_str(arg);
                }
                let params = args_string
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>();

                let result = ShellExecuteW(
                    HWND(0),
                    windows::core::w!("open"),
                    windows::core::PCWSTR(exe_path.as_ptr()),
                    windows::core::PCWSTR(params.as_ptr()),
                    windows::core::PCWSTR(std::ptr::null()),
                    SW_SHOW,
                );

                if result <= 32 {
                    return Err(ActionError::ActionFailed(format!(
                        "ShellExecute failed: {}",
                        result
                    )));
                }
            }

            Ok(ActionResult::success(None, "shellExecute"))
        }
        None => Err(ActionError::TargetNotFound(
            "no app identifier specified".to_string(),
        )),
    }
}
