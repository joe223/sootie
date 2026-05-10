use std::path::Path;

use windows::Win32::Foundation::*;
use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::action::ActionError;
use crate::selector::Selector;

pub fn resolve_window(selector: &Selector) -> Result<HWND, ActionError> {
    let mut search = WindowSearch {
        selector,
        matches: Vec::new(),
        foreground: unsafe { GetForegroundWindow() },
    };

    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut search as *mut _ as isize),
        );
    }

    search
        .matches
        .into_iter()
        .max_by_key(|entry| entry.focused)
        .map(|entry| entry.hwnd)
        .ok_or_else(|| ActionError::TargetNotFound("no matching window found".to_string()))
}

struct WindowSearch<'a> {
    selector: &'a Selector,
    matches: Vec<MatchedWindow>,
    foreground: HWND,
}

#[derive(Debug)]
struct MatchedWindow {
    hwnd: HWND,
    focused: bool,
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut WindowSearch<'_>);

    if !IsWindowVisible(hwnd).as_bool() {
        return BOOL(1);
    }

    let title = window_title(hwnd);
    if title.is_empty() {
        return BOOL(1);
    }

    let exe_path = match executable_path(hwnd) {
        Some(path) => path,
        None => return BOOL(1),
    };

    if matches_selector(
        search.selector,
        hwnd,
        &title,
        &exe_path,
        search.foreground == hwnd,
    ) {
        search.matches.push(MatchedWindow {
            hwnd,
            focused: search.foreground == hwnd,
        });
    }

    BOOL(1)
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let mut title = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut title);
        if len == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&title[..len as usize])
    }
}

fn executable_path(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut process_id: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));
        let process = OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            process_id,
        )
        .ok()?;
        let mut exe_name = [0u16; 260];
        let len = GetModuleFileNameExW(process, HMODULE(std::ptr::null_mut()), &mut exe_name);
        if len == 0 {
            None
        } else {
            Some(String::from_utf16_lossy(&exe_name[..len as usize]))
        }
    }
}

fn matches_selector(
    selector: &Selector,
    hwnd: HWND,
    title: &str,
    exe_path: &str,
    focused: bool,
) -> bool {
    if let Some(app) = selector.app.as_ref() {
        if let Some(name) = app.name.as_ref() {
            let process_name = normalize_process_name(exe_path);
            if !process_name.to_lowercase().contains(&name.to_lowercase())
                && !title.to_lowercase().contains(&name.to_lowercase())
            {
                return false;
            }
        }

        if let Some(bundle_id) = app.bundle_id.as_ref() {
            if !exe_path.to_lowercase().contains(&bundle_id.to_lowercase()) {
                return false;
            }
        }

        if let Some(is_frontmost) = app.is_frontmost {
            if focused != is_frontmost {
                return false;
            }
        }
    }

    if let Some(window) = selector.window.as_ref() {
        if let Some(id) = window.id.as_ref() {
            if id != &format!("win_{:?}", hwnd.0) {
                return false;
            }
        }

        if let Some(title_match) = window.title.as_ref() {
            if !title.to_lowercase().contains(&title_match.to_lowercase()) {
                return false;
            }
        }

        if let Some(wanted_focus) = window.focused {
            if focused != wanted_focus {
                return false;
            }
        }
    }

    true
}

fn normalize_process_name(exe_path: &str) -> String {
    Path::new(exe_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(exe_path)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::{AppSelector, Selector, WindowSelector};

    #[test]
    fn test_normalize_process_name() {
        assert_eq!(
            normalize_process_name("C:\\Program Files\\Code.exe"),
            "Code"
        );
        assert_eq!(normalize_process_name("Code.exe"), "Code");
    }

    #[test]
    fn test_matches_selector() {
        let selector = Selector::new()
            .with_app(AppSelector::from_name("Code").with_bundle_id("Code.exe"))
            .with_window(WindowSelector::from_title("hello.txt").with_focused(true));

        assert!(matches_selector(
            &selector,
            HWND(42),
            "hello.txt - Visual Studio Code",
            "C:\\Program Files\\Microsoft VS Code\\Code.exe",
            true,
        ));
    }
}
