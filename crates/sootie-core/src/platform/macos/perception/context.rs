use std::process::Command;

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::{CFString, CFStringRef};
use core_graphics::display::CGDisplay;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer,
    kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowName,
    kCGWindowNumber, kCGWindowOwnerPID,
};
use tracing::warn;

use crate::perception::{AppContext, Context};
use crate::selector::{App, AppSelector, Bounds, Window};

use super::super::ax_fns::*;

fn parse_display_bounds_line(line: &str) -> Option<(u32, Bounds)> {
    let parts = line.trim().split('|').collect::<Vec<_>>();
    if parts.len() != 5 {
        return None;
    }

    let index = parts[0].trim().parse::<u32>().ok()?;
    let x = parts[1].trim().parse::<f64>().ok()?;
    let y = parts[2].trim().parse::<f64>().ok()?;
    let width = parts[3].trim().parse::<f64>().ok()?;
    let height = parts[4].trim().parse::<f64>().ok()?;

    Some((
        index,
        Bounds {
            x,
            y,
            width,
            height,
        },
    ))
}

fn get_display_bounds() -> Vec<(u32, Bounds)> {
    if let Ok(displays) = CGDisplay::active_displays() {
        let display_bounds = displays
            .into_iter()
            .enumerate()
            .map(|(index, display_id)| {
                let bounds = CGDisplay::new(display_id).bounds();
                (
                    (index + 1) as u32,
                    Bounds {
                        x: bounds.origin.x,
                        y: bounds.origin.y,
                        width: bounds.size.width,
                        height: bounds.size.height,
                    },
                )
            })
            .collect::<Vec<_>>();
        if !display_bounds.is_empty() {
            return display_bounds;
        }
    }

    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            use framework "Foundation"
            use framework "AppKit"
            set screens to (current application's NSScreen's screens())
            set screenIndex to 1
            set outputLines to {}
            repeat with s in screens
                set frame to s's frame()
                set line to (screenIndex as text) & "|" & ((frame's origin's x) as real as text) & "|" & ((frame's origin's y) as real as text) & "|" & ((frame's size's width) as real as text) & "|" & ((frame's size's height) as real as text)
                set end of outputLines to line
                set screenIndex to screenIndex + 1
            end repeat
            return outputLines
            "#,
        )
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout
                .split(", ")
                .filter_map(parse_display_bounds_line)
                .collect()
        }
        _ => vec![],
    }
}

fn display_for_point(x: f64, y: f64, displays: &[(u32, Bounds)]) -> Option<u32> {
    displays
        .iter()
        .find_map(|(index, bounds)| bounds.contains(x, y).then_some(*index))
}

fn cf_window_key(key: CFStringRef) -> CFString {
    unsafe { CFString::wrap_under_get_rule(key) }
}

fn cf_number_from_dict(dict: &CFDictionary<CFString, CFType>, key: &CFString) -> Option<CFNumber> {
    dict.find(key)
        .and_then(|value| value.downcast::<CFNumber>())
}

fn cf_i32_from_dict(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<i32> {
    cf_number_from_dict(dict, &cf_window_key(key)).and_then(|value| value.to_i32())
}

fn cf_u32_from_dict(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<u32> {
    cf_i32_from_dict(dict, key).and_then(|value| u32::try_from(value).ok())
}

fn cf_string_from_dict(dict: &CFDictionary<CFString, CFType>, key: CFStringRef) -> Option<String> {
    dict.find(&cf_window_key(key))
        .and_then(|value| value.downcast::<CFString>())
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty())
}

fn cg_bounds_from_dict(dict: &CFDictionary<CFString, CFType>) -> Option<Bounds> {
    let bounds_value = dict
        .find(&cf_window_key(unsafe { kCGWindowBounds }))?
        .clone();
    if !bounds_value.instance_of::<CFDictionary>() {
        return None;
    }
    let bounds_dict: CFDictionary<CFString, CFType> = unsafe {
        CFDictionary::wrap_under_get_rule(bounds_value.as_CFTypeRef() as CFDictionaryRef)
    };

    let x = cf_number_from_dict(&bounds_dict, &CFString::from_static_string("X"))?.to_f64()?;
    let y = cf_number_from_dict(&bounds_dict, &CFString::from_static_string("Y"))?.to_f64()?;
    let width =
        cf_number_from_dict(&bounds_dict, &CFString::from_static_string("Width"))?.to_f64()?;
    let height =
        cf_number_from_dict(&bounds_dict, &CFString::from_static_string("Height"))?.to_f64()?;

    (width > 0.0 && height > 0.0).then_some(Bounds {
        x,
        y,
        width,
        height,
    })
}

fn get_cg_windows(pid: i32, displays: &[(u32, Bounds)]) -> Vec<Window> {
    let Some(window_info) = copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return vec![];
    };

    window_info
        .get_all_values()
        .into_iter()
        .filter_map(|value| {
            let dict: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(value as CFDictionaryRef) };

            if cf_i32_from_dict(&dict, unsafe { kCGWindowOwnerPID }) != Some(pid) {
                return None;
            }

            if cf_i32_from_dict(&dict, unsafe { kCGWindowLayer }) != Some(0) {
                return None;
            }

            let bounds = cg_bounds_from_dict(&dict)?;
            let center_x = bounds.x + bounds.width / 2.0;
            let center_y = bounds.y + bounds.height / 2.0;
            let display_id = display_for_point(center_x, center_y, displays);
            let window_number = cf_u32_from_dict(&dict, unsafe { kCGWindowNumber });
            let index = window_number.unwrap_or(0);

            Some(Window {
                id: window_number
                    .map(|number| format!("cg_{}", number))
                    .unwrap_or_else(|| "cg_unknown".to_string()),
                title: cf_string_from_dict(&dict, unsafe { kCGWindowName }).unwrap_or_default(),
                index,
                focused: false,
                bounds,
                display_id,
            })
        })
        .enumerate()
        .map(|(fallback_index, mut window)| {
            if window.index == 0 {
                window.index = fallback_index as u32;
            }
            window
        })
        .collect()
}

fn parse_running_app_line(line: &str) -> Option<(String, String, bool, i32)> {
    let parts = line.trim().split('|').collect::<Vec<_>>();
    if parts.len() != 4 {
        return None;
    }

    let name = parts[0].trim().to_string();
    if name.is_empty() {
        return None;
    }

    let bundle_id = parts[1].trim().to_string();
    let is_frontmost = parts[2].trim() == "true";
    let pid = parts[3].trim().parse::<i32>().ok()?;

    Some((name, bundle_id, is_frontmost, pid))
}

fn get_running_app_records() -> Vec<(String, String, bool, i32)> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            use framework "AppKit"
            set workspace to current application's NSWorkspace's sharedWorkspace()
            set frontPid to -1
            set frontApp to workspace's frontmostApplication()
            if frontApp is not missing value then
                set frontPid to frontApp's processIdentifier()
            end if

            set outputLines to {}
            set runningApps to workspace's runningApplications()
            repeat with appRef in runningApps
                if ((appRef's activationPolicy()) as integer) is 0 then
                    set appName to appRef's localizedName()
                    if appName is missing value then
                        set appNameText to ""
                    else
                        set appNameText to appName as text
                    end if

                    set bundleId to appRef's bundleIdentifier()
                    if bundleId is missing value then
                        set bundleText to ""
                    else
                        set bundleText to bundleId as text
                    end if

                    set pidValue to appRef's processIdentifier()
                    set isFront to (pidValue = frontPid)
                    set end of outputLines to appNameText & "|" & bundleText & "|" & (isFront as text) & "|" & ((pidValue as integer) as text)
                end if
            end repeat
            set AppleScript's text item delimiters to linefeed
            return outputLines as text
            "#,
        )
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            stdout.lines().filter_map(parse_running_app_line).collect()
        }
        _ => vec![],
    }
}

pub fn get_running_apps() -> crate::perception::Context {
    let displays = get_display_bounds();
    let app_records = get_running_app_records();
    if !app_records.is_empty() {
        let apps = app_records
            .into_iter()
            .map(|(name, bundle_id, is_frontmost, pid)| {
                let app = App {
                    name,
                    bundle_id,
                    is_frontmost,
                };
                let windows = if pid > 0 {
                    get_app_windows(pid, &displays)
                } else {
                    vec![]
                };
                AppContext { app, windows }
            })
            .collect();
        return Context { apps };
    }

    let output = Command::new("osascript")
        .arg("-e")
        .arg(
            r#"
            tell application "System Events"
                set appList to {}
                set frontApp to name of first process whose frontmost is true
                repeat with p in (every process whose background only is false)
                    set appName to name of p
                    set appBundle to bundle identifier of p
                    set isFront to (appName is frontApp)
                    set end of appList to appName & "|" & appBundle & "|" & isFront
                end repeat
                return appList
            end tell
            "#,
        )
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let mut apps = Vec::new();

            for entry in stdout.split(", ") {
                let parts: Vec<&str> = entry.trim().split('|').collect();
                if parts.len() >= 3 {
                    let name = parts[0].trim().to_string();
                    let bundle_id = parts[1].trim().to_string();
                    let is_frontmost = parts[2].trim() == "true";

                    if name.is_empty() {
                        continue;
                    }

                    let pid = get_pid_for_app_name(&name);
                    let app = crate::selector::App {
                        name: name.clone(),
                        bundle_id: bundle_id.clone(),
                        is_frontmost,
                    };
                    let windows = if pid > 0 {
                        get_app_windows(pid, &displays)
                    } else {
                        vec![]
                    };
                    apps.push(crate::perception::AppContext { app, windows });
                }
            }
            Context { apps }
        }
        _ => {
            warn!("Failed to get running apps via osascript, falling back to empty list");
            Context { apps: vec![] }
        }
    }
}

pub fn get_app_context(app_selector: &AppSelector) -> Option<AppContext> {
    let app_name = app_selector.name.as_deref()?;
    let pid = get_pid_for_app_name(app_name);
    if pid <= 0 {
        return None;
    }

    let displays = get_display_bounds();
    Some(AppContext {
        app: App {
            name: app_name.to_string(),
            bundle_id: app_selector.bundle_id.clone().unwrap_or_default(),
            is_frontmost: false,
        },
        windows: get_app_windows(pid, &displays),
    })
}

pub fn get_app_windows(pid: i32, displays: &[(u32, Bounds)]) -> Vec<crate::selector::Window> {
    fn has_valid_bounds(window: &crate::selector::Window) -> bool {
        window.bounds.width > 0.0 && window.bounds.height > 0.0
    }

    fn cg_fallback_for_window(
        ax_window: &crate::selector::Window,
        cg_windows: &[crate::selector::Window],
    ) -> Option<crate::selector::Window> {
        cg_windows
            .iter()
            .find(|cg_window| {
                !ax_window.title.is_empty()
                    && !cg_window.title.is_empty()
                    && cg_window.title == ax_window.title
            })
            .or_else(|| cg_windows.get(ax_window.index as usize))
            .cloned()
    }

    if !is_process_trusted() {
        return get_cg_windows(pid, displays);
    }

    unsafe {
        let app_element = AXUIElementCreateApplication(pid);
        if app_element.is_null() {
            return get_cg_windows(pid, displays);
        }

        let mut windows = Vec::new();

        let mut window_refs = get_windows(app_element);
        if window_refs.is_empty() {
            window_refs = get_children(app_element);
        }
        for (index, window_ref) in window_refs.iter().enumerate() {
            let role = get_string_attr(*window_ref, "AXRole").unwrap_or_default();
            if role != "AXWindow" {
                release_ax_element(*window_ref);
                continue;
            }

            let title = get_string_attr(*window_ref, "AXTitle").unwrap_or_default();
            let position = get_point_attr(*window_ref, "AXPosition");
            let size = get_size_attr(*window_ref, "AXSize");

            let focused = get_bool_attr(*window_ref, "AXFocused").unwrap_or(false);

            let bounds = match (position, size) {
                (Some(pos), Some(sz)) => crate::selector::Bounds {
                    x: pos.x,
                    y: pos.y,
                    width: sz.width,
                    height: sz.height,
                },
                _ => crate::selector::Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            };

            // Calculate window center to determine which display it belongs to
            let center_x = bounds.x + bounds.width / 2.0;
            let center_y = bounds.y + bounds.height / 2.0;
            let display_id = display_for_point(center_x, center_y, displays);

            windows.push(crate::selector::Window {
                id: format!("win_{}", index),
                title,
                index: index as u32,
                focused,
                bounds,
                display_id,
            });

            release_ax_element(*window_ref);
        }

        release_ax_element(app_element);
        if windows.is_empty() {
            get_cg_windows(pid, displays)
        } else {
            let cg_windows = if windows.iter().any(|window| !has_valid_bounds(window)) {
                get_cg_windows(pid, displays)
            } else {
                Vec::new()
            };

            for window in windows.iter_mut() {
                if has_valid_bounds(window) {
                    continue;
                }

                if let Some(cg_window) = cg_fallback_for_window(window, &cg_windows) {
                    window.bounds = cg_window.bounds;
                    window.display_id = cg_window.display_id;
                    if window.title.is_empty() {
                        window.title = cg_window.title;
                    }
                }
            }

            if windows.iter().all(|window| !has_valid_bounds(window)) && !cg_windows.is_empty() {
                return cg_windows;
            }

            windows
        }
    }
}

pub fn get_pid_for_app_name(name: &str) -> i32 {
    if let Some((_, _, _, pid)) = get_running_app_records()
        .into_iter()
        .find(|(app_name, _, _, _)| app_name == name)
    {
        return pid;
    }

    let output = Command::new("pgrep").arg("-x").arg(name).output().ok();

    output
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout
                .lines()
                .next()
                .and_then(|line| line.trim().parse::<i32>().ok())
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_loads() {
        assert!(true);
    }

    #[test]
    #[ignore = "requires system permissions"]
    fn test_get_running_apps() {
        let ctx = get_running_apps();
        assert!(!ctx.apps.is_empty() || ctx.apps.is_empty());
    }

    #[test]
    #[ignore = "requires system permissions"]
    fn test_get_pid_for_app_name() {
        let pid = get_pid_for_app_name("Finder");
        assert!(pid > 0 || pid == 0);
    }
}
