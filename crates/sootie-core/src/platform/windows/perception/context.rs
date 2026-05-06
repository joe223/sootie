use std::path::Path;
use windows::Win32::Foundation::*;
use windows::Win32::System::ProcessStatus::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::perception::{AppContext, Context};
use crate::selector::{App, Bounds, Window};

pub fn get_running_apps() -> Result<Context, crate::perception::PerceptionError> {
    unsafe {
        let mut apps = Vec::new();

        let hwnd = GetDesktopWindow();
        EnumWindows(
            Some(enum_windows_callback),
            LPARAM(&mut apps as *mut _ as isize),
        );

        Ok(Context { apps })
    }
}

unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let apps = &mut *(lparam.0 as *mut Vec<AppContext>);

    let mut title = [0u16; 512];
    let title_len = GetWindowTextW(hwnd, &mut title, 512);
    if title_len == 0 {
        return BOOL(1);
    }

    let title_string = String::from_utf16_lossy(&title[..title_len as usize]);
    if title_string.is_empty() {
        return BOOL(1);
    }

    let mut process_id: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut process_id));

    let process_handle = OpenProcess(
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
        false,
        process_id,
    );
    if process_handle.is_err() {
        return BOOL(1);
    }

    let mut exe_name = [0u16; 260];
    let exe_len = GetModuleFileNameExW(process_handle.unwrap(), HMODULE(0), &mut exe_name, 260);

    let exe_string = if exe_len > 0 {
        String::from_utf16_lossy(&exe_name[..exe_len as usize])
    } else {
        "Unknown".to_string()
    };
    let app_name = normalize_process_name(&exe_string);

    let is_visible = IsWindowVisible(hwnd).as_bool();
    if !is_visible {
        return BOOL(1);
    }

    let mut rect = windows::Win32::Graphics::Gdi::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    GetWindowRect(hwnd, &mut rect);

    let bounds = Bounds {
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left) as f64,
        height: (rect.bottom - rect.top) as f64,
    };

    apps.push(AppContext {
        app: App {
            name: app_name,
            bundle_id: exe_string,
            is_frontmost: GetForegroundWindow() == hwnd,
        },
        windows: vec![Window {
            id: format!("win_{}", hwnd.0),
            title: title_string,
            index: 0,
            focused: GetForegroundWindow() == hwnd,
            bounds,
        }],
    });

    BOOL(1)
}

fn normalize_process_name(exe_path: &str) -> String {
    Path::new(exe_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(exe_path)
        .to_string()
}
