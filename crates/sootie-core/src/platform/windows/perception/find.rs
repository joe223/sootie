use windows::Win32::Foundation::*;
use windows::Win32::System::ProcessStatus::GetModuleFileNameExW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::perception::PerceptionError;
use crate::selector::{Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector};

pub fn find_elements(selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
    let mut search = WindowSearch {
        selector,
        elements: Vec::new(),
        foreground: unsafe { GetForegroundWindow() },
        index: 0,
    };

    unsafe {
        let _ = EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut search as *mut _ as isize),
        );
    }

    let (status, total_matches) = match search.elements.len() {
        0 => (MatchStatus::None, 0),
        1 => (MatchStatus::Unique, 1),
        n => (MatchStatus::Multiple, n as u32),
    };

    Ok(ResolvedTarget {
        status,
        total_matches,
        app: None,
        window: None,
        elements: search.elements,
    })
}

struct WindowSearch<'a> {
    selector: &'a Selector,
    elements: Vec<Element>,
    foreground: HWND,
    index: u32,
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

    let exe_path = executable_path(hwnd).unwrap_or_default();
    if !matches_selector(
        search.selector,
        hwnd,
        &title,
        &exe_path,
        search.foreground == hwnd,
    ) {
        return BOOL(1);
    }

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let _ = GetWindowRect(hwnd, &mut rect);

    search.elements.push(Element {
        role: "window".to_string(),
        name: title,
        text: None,
        id: Some(format!("win_{:?}", hwnd.0)),
        state: ElementState {
            visible: true,
            focused: Some(search.foreground == hwnd),
            enabled: Some(true),
        },
        bounds: Bounds {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left).max(0) as f64,
            height: (rect.bottom - rect.top).max(0) as f64,
        },
        index: search.index,
    });
    search.index += 1;

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
            let exe_lower = exe_path.to_lowercase();
            let title_lower = title.to_lowercase();
            let name_lower = name.to_lowercase();
            if !exe_lower.contains(&name_lower) && !title_lower.contains(&name_lower) {
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

    let matches_role = selector
        .element
        .role
        .as_ref()
        .map(|role| role.eq_ignore_ascii_case("window"))
        .unwrap_or(true);
    let matches_name = selector
        .element
        .name
        .as_ref()
        .map(|name| title.to_lowercase().contains(&name.to_lowercase()))
        .unwrap_or(true);

    matches_role && matches_name
}
