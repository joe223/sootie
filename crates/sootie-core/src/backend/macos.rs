use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::json;

use crate::backend::{
    cdp, command_output, command_output_stdin_timeout, command_output_timeout,
    element_at_from_elements, element_from_window, filter_elements, has_element_target,
    png_dimensions, tmp_screenshot_path, DesktopBackend,
};
use crate::types::{
    ActionResult, AppInfo, Bounds, ContextSnapshot, ElementInfo, FindQuery, RuntimeDiagnostic,
    Screenshot, SootieError, SootieResult, WindowCommand, WindowInfo,
};

pub struct MacosBackend;

const ACCESSIBILITY_ELEMENT_LIMIT: usize = 80;
const APP_SNAPSHOT_SCRIPT_TIMEOUT_MS: u64 = 1_500;
const APP_LIST_SCRIPT_TIMEOUT_MS: u64 = 5_000;
const APP_STATE_SCRIPT_TIMEOUT_MS: u64 = 10_000;
const ACCESSIBILITY_SCRIPT_TIMEOUT_MS: u64 = 5_000;
const SCREENSHOT_TIMEOUT_MS: u64 = 5_000;
const FOCUS_CONFIRM_TIMEOUT_MS: u64 = 1_200;
const BROWSER_URL_TIMEOUT_MS: u64 = 1_000;
const POINTER_EVENT_TIMEOUT_MS: u64 = 1_000;
const KEYBOARD_EVENT_TIMEOUT_MS: u64 = 1_000;

#[derive(Debug, Clone)]
struct AccessibilityElement {
    element: ElementInfo,
    focused: bool,
}

fn osascript(script: &str) -> SootieResult<String> {
    command_output("osascript", &["-e", script])
}

fn osascript_with_timeout(script: &str, timeout: Duration) -> SootieResult<String> {
    osascript_args_with_timeout(&["-e", script], timeout)
}

fn osascript_e_with_timeout(script: &str, timeout: Duration) -> SootieResult<String> {
    osascript_with_timeout(script, timeout)
}

fn osascript_args_with_timeout(args: &[&str], timeout: Duration) -> SootieResult<String> {
    command_output_timeout("osascript", args, timeout)
}

fn run_jxa(script: &str, timeout: Duration) -> SootieResult<String> {
    osascript_args_with_timeout(&["-l", "JavaScript", "-e", script], timeout)
}

fn run_swift(script: &str, timeout: Duration) -> SootieResult<String> {
    command_output_timeout(
        "env",
        &[
            "CLANG_MODULE_CACHE_PATH=/tmp/sootie-swift-cache",
            "swift",
            "-e",
            script,
        ],
        timeout,
    )
}

fn esc(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn looks_like_bundle_id(value: &str) -> bool {
    value.contains('.') && !value.contains('/')
}

fn application_name_from_bundle_id(bundle_id: &str) -> Option<String> {
    osascript(&format!(
        "tell application id \"{}\" to name",
        esc(bundle_id)
    ))
    .ok()
    .map(|output| output.trim().to_string())
    .filter(|name| !name.is_empty())
}

fn browser_application_name(app_name: &str) -> Option<&'static str> {
    let normalized = app_name.to_lowercase();
    match normalized.as_str() {
        "safari" => Some("Safari"),
        "google chrome" | "chrome" => Some("Google Chrome"),
        "microsoft edge" | "edge" => Some("Microsoft Edge"),
        "brave browser" | "brave" => Some("Brave Browser"),
        "chromium" => Some("Chromium"),
        _ => None,
    }
}

fn browser_url_script(app_name: &str) -> String {
    if app_name == "Safari" {
        format!(
            r#"tell application "System Events" to set appRunning to exists (process "{}")
if appRunning then
  tell application "{}"
    if (count of windows) is 0 then return ""
    return URL of current tab of front window
  end tell
end if
return """#,
            esc(app_name),
            esc(app_name)
        )
    } else {
        format!(
            r#"tell application "System Events" to set appRunning to exists (process "{}")
if appRunning then
  tell application "{}"
    if (count of windows) is 0 then return ""
    return URL of active tab of front window
  end tell
end if
return """#,
            esc(app_name),
            esc(app_name)
        )
    }
}

fn current_browser_url(app_name: &str) -> Option<String> {
    let app_name = browser_application_name(app_name)?;
    if let Some(url) = cdp_browser_url(app_name) {
        return Some(url);
    }
    osascript_with_timeout(
        &browser_url_script(app_name),
        Duration::from_millis(BROWSER_URL_TIMEOUT_MS),
    )
    .ok()
    .map(|output| output.trim().to_string())
    .filter(|url| !url.is_empty() && url != "missing value")
}

fn cdp_browser_url(app_name: &str) -> Option<String> {
    if app_name == "Safari" {
        return None;
    }
    let port = browser_cdp_port(app_name)?;
    cdp::current_page_url(port)
}

fn browser_cdp_port(app_name: &str) -> Option<u16> {
    let output = browser_process_commands()?;
    parse_cdp_port_from_process_commands(&output, app_name)
}

fn first_browser_cdp_port() -> Option<u16> {
    let output = browser_process_commands()?;
    parse_first_browser_cdp_port(&output)
}

fn browser_process_commands() -> Option<String> {
    command_output("ps", &["-ax", "-o", "command="]).ok()
}

fn parse_cdp_port_from_process_commands(output: &str, app_name: &str) -> Option<u16> {
    output.lines().find_map(|row| {
        let command = row.trim();
        let process_app = app_name_from_process_command(command)?;
        if !app_name_equals(&process_app, app_name) {
            return None;
        }
        cdp::parse_remote_debugging_port(command)
    })
}

fn parse_first_browser_cdp_port(output: &str) -> Option<u16> {
    output.lines().find_map(|row| {
        let command = row.trim();
        let process_app = app_name_from_process_command(command)?;
        browser_application_name(&process_app)?;
        cdp::parse_remote_debugging_port(command)
    })
}

fn frontmost_app_info(app_filter: Option<&str>) -> Option<AppInfo> {
    if let Ok(apps) = window_server_app_list_result(app_filter) {
        if let Some(app) = apps
            .iter()
            .find(|app| app.is_frontmost)
            .or_else(|| apps.first())
        {
            return Some(app.clone());
        }
    }
    if let Some(app) = frontmost_app_name_info(app_filter) {
        return Some(app);
    }
    osascript_with_timeout(
        frontmost_app_script(),
        Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
    )
    .ok()
    .and_then(|output| parse_frontmost_app_info(&output, app_filter))
}

fn frontmost_app_name_info(app_filter: Option<&str>) -> Option<AppInfo> {
    osascript_e_with_timeout(
        frontmost_app_name_script(),
        Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
    )
    .ok()
    .and_then(|output| app_info_from_frontmost_name(&output, app_filter))
}

fn frontmost_app_name_script() -> &'static str {
    r#"tell application "System Events" to name of first process whose frontmost is true"#
}

fn ax_window_probe_script(app_reference: &str) -> String {
    let app_json = serde_json::to_string(app_reference).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"import AppKit
import ApplicationServices
import Darwin
import Foundation

func fail(_ message: String, _ code: Int32 = 2) -> Never {{
    FileHandle.standardError.write(Data(message.utf8))
    exit(code)
}}

let wanted = {app_json}
let normalized = wanted.lowercased()
let matches = NSWorkspace.shared.runningApplications.filter {{
    $0.activationPolicy == .regular &&
    ((($0.localizedName ?? "").lowercased().contains(normalized)) ||
     (($0.bundleIdentifier ?? "").lowercased().contains(normalized)))
}}
let exact = matches.first(where: {{
    (($0.localizedName ?? "").lowercased() == normalized) ||
    (($0.bundleIdentifier ?? "").lowercased() == normalized)
}})
guard let app = exact ?? matches.first else {{
    fail("application not found: \(wanted)")
}}

let axApp = AXUIElementCreateApplication(app.processIdentifier)
var rawWindows: CFTypeRef?
let err = AXUIElementCopyAttributeValue(axApp, kAXWindowsAttribute as CFString, &rawWindows)
guard err == .success, let windows = rawWindows as? [AXUIElement], !windows.isEmpty else {{
    fail("AX window probe failed: \(err.rawValue)")
}}
print("\(app.localizedName ?? wanted)\u{{1f}}\(windows.count)")
"#
    )
}

fn macos_accessibility_diagnostic_from_result(result: SootieResult<String>) -> RuntimeDiagnostic {
    match result {
        Ok(output) => {
            let mut fields = output.trim().split('\u{1f}');
            let app = fields.next().unwrap_or_default();
            let window_count = fields.next().and_then(|value| value.parse::<usize>().ok());
            RuntimeDiagnostic {
                name: "macos_accessibility".to_string(),
                success: true,
                message: "macOS Accessibility probe succeeded".to_string(),
                details: Some(json!({ "app": app, "window_count": window_count })),
            }
        }
        Err(error) => RuntimeDiagnostic {
            name: "macos_accessibility".to_string(),
            success: false,
            message: "macOS Accessibility denied for the Sootie launch path".to_string(),
            details: Some(json!({
                "error": error.to_string(),
                "recovery": "Grant Accessibility permission to the app or terminal that launches Sootie, then restart that launcher."
            })),
        },
    }
}

fn macos_accessibility_diagnostic() -> RuntimeDiagnostic {
    let Some(app) = window_server_app_list_result(None)
        .ok()
        .and_then(|apps| apps.into_iter().next())
    else {
        return RuntimeDiagnostic {
            name: "macos_accessibility".to_string(),
            success: false,
            message: "macOS Accessibility probe found no visible app".to_string(),
            details: Some(json!({
                "recovery": "Run Sootie from a process attached to an active Aqua desktop session with at least one visible application window."
            })),
        };
    };
    macos_accessibility_diagnostic_from_result(run_swift(
        &ax_window_probe_script(&app.name),
        Duration::from_millis(APP_STATE_SCRIPT_TIMEOUT_MS),
    ))
}

fn macos_window_server_diagnostic_from_result(
    result: SootieResult<Vec<AppInfo>>,
) -> RuntimeDiagnostic {
    match result {
        Ok(apps) if !apps.is_empty() => RuntimeDiagnostic {
            name: "macos_window_server".to_string(),
            success: true,
            message: "macOS WindowServer visible-window probe succeeded".to_string(),
            details: Some(json!({
                "app_count": apps.len(),
                "frontmost_candidate": apps.first().map(|app| app.name.clone()),
            })),
        },
        Ok(_) => RuntimeDiagnostic {
            name: "macos_window_server".to_string(),
            success: false,
            message: "macOS WindowServer visible-window probe found no windows".to_string(),
            details: Some(json!({
                "recovery": "Run Sootie from a process attached to an active Aqua desktop session. If screenshots fail too, grant Screen Recording permission to the launcher and verify the display is awake."
            })),
        },
        Err(error) => RuntimeDiagnostic {
            name: "macos_window_server".to_string(),
            success: false,
            message: "macOS WindowServer visible-window probe failed".to_string(),
            details: Some(json!({
                "error": error.to_string(),
                "recovery": "Run Sootie from a process attached to an active Aqua desktop session. If screenshots fail too, grant Screen Recording permission to the launcher and verify the display is awake."
            })),
        },
    }
}

fn macos_window_server_diagnostic() -> RuntimeDiagnostic {
    macos_window_server_diagnostic_from_result(window_server_app_list_result(None))
}

fn frontmost_app_script() -> &'static str {
    r#"tell application "System Events"
set oldDelimiters to AppleScript's text item delimiters
set fieldSep to character id 31
set targetProcess to first process whose frontmost is true
set appName to name of targetProcess as text
set appPid to ""
set appBundleId to ""
try
  set appPid to unix id of targetProcess as text
end try
try
  set appBundleId to bundle identifier of targetProcess as text
end try
set AppleScript's text item delimiters to fieldSep
set resultText to {appName, appPid, appBundleId} as text
set AppleScript's text item delimiters to oldDelimiters
return resultText
end tell"#
}

fn fallback_app_info(app_filter: Option<&str>) -> AppInfo {
    frontmost_app_info(app_filter).unwrap_or_else(|| {
        let name = app_filter
            .and_then(non_empty_string)
            .unwrap_or_else(|| "unknown".to_string());
        minimal_app_info(name, false)
    })
}

fn fallback_app_list(app_filter: Option<&str>) -> Vec<AppInfo> {
    let frontmost = frontmost_app_info(app_filter);
    let frontmost_name = frontmost.as_ref().map(|app| app.name.as_str());
    let mut apps = fallback_ps_app_list(app_filter, frontmost_name);
    if let Some(frontmost) = frontmost {
        if let Some(app) = apps
            .iter_mut()
            .find(|app| app_name_equals(&app.name, &frontmost.name))
        {
            app.is_frontmost = true;
        } else {
            apps.insert(0, frontmost);
        }
    }
    if apps.is_empty() {
        vec![fallback_app_info(app_filter)]
    } else {
        apps
    }
}

fn fallback_app_list_without_frontmost_retry(app_filter: Option<&str>) -> Vec<AppInfo> {
    let apps = window_server_app_list(app_filter);
    if !apps.is_empty() {
        return apps;
    }
    let apps = fallback_ps_app_list(app_filter, None);
    if apps.is_empty() {
        vec![fallback_app_info_without_frontmost_retry(app_filter)]
    } else {
        apps
    }
}

fn fallback_state_app_list(app_filter: Option<&str>) -> Vec<AppInfo> {
    if app_filter.is_some() {
        let frontmost = frontmost_app_info(app_filter);
        let frontmost_name = frontmost.as_ref().map(|app| app.name.as_str());
        let mut apps = fallback_ps_app_list(app_filter, frontmost_name);
        if let Some(frontmost) = frontmost {
            if let Some(app) = apps
                .iter_mut()
                .find(|app| app_name_equals(&app.name, &frontmost.name))
            {
                app.is_frontmost = true;
            } else {
                apps.insert(0, frontmost);
            }
        }
        apps
    } else {
        fallback_app_list(None)
    }
}

fn fallback_state_app_list_without_frontmost_retry(app_filter: Option<&str>) -> Vec<AppInfo> {
    if app_filter.is_some() {
        let apps = window_server_app_list(app_filter);
        if apps.is_empty() {
            fallback_ps_app_list(app_filter, None)
        } else {
            apps
        }
    } else {
        fallback_app_list_without_frontmost_retry(None)
    }
}

fn fallback_app_info_without_frontmost_retry(app_filter: Option<&str>) -> AppInfo {
    let name = app_filter
        .and_then(non_empty_string)
        .unwrap_or_else(|| "unknown".to_string());
    minimal_app_info(name, false)
}

fn fallback_ps_app_list(app_filter: Option<&str>, frontmost_name: Option<&str>) -> Vec<AppInfo> {
    ps_app_list(app_filter, frontmost_name).unwrap_or_default()
}

fn app_info_from_frontmost_name(name: &str, app_filter: Option<&str>) -> Option<AppInfo> {
    let name = non_empty_string(name)?;
    if let Some(filter) = app_filter {
        if !app_name_matches(&name, filter) {
            return None;
        }
    }
    Some(minimal_app_info(name, true))
}

fn parse_frontmost_app_info(output: &str, app_filter: Option<&str>) -> Option<AppInfo> {
    let trimmed = output.trim();
    let fields = trimmed.split('\u{1f}').collect::<Vec<_>>();
    if fields.len() < 3 {
        return app_info_from_frontmost_name(trimmed, app_filter);
    }
    let name = non_empty_string(fields[0])?;
    let pid = fields[1].trim().parse::<u32>().ok();
    let bundle_id = non_empty_string(fields[2]);
    if let Some(filter) = app_filter {
        if !app_identity_matches(&name, bundle_id.as_deref(), filter) {
            return None;
        }
    }
    Some(AppInfo {
        app_id: Some(name.clone()),
        platform_app_id: bundle_id.clone(),
        name,
        pid,
        bundle_id,
        is_frontmost: true,
        windows: Vec::new(),
    })
}

fn ps_app_list(
    app_filter: Option<&str>,
    frontmost_name: Option<&str>,
) -> SootieResult<Vec<AppInfo>> {
    let output = command_output("ps", &["-ax", "-o", "pid=", "-o", "comm="])?;
    Ok(parse_ps_app_rows(&output, app_filter, frontmost_name))
}

fn parse_ps_app_rows(
    output: &str,
    app_filter: Option<&str>,
    frontmost_name: Option<&str>,
) -> Vec<AppInfo> {
    let mut apps = Vec::new();
    for row in output.lines() {
        let row = row.trim();
        if row.is_empty() {
            continue;
        }
        let mut parts = row.splitn(2, char::is_whitespace);
        let pid = parts.next().and_then(|value| value.parse::<u32>().ok());
        let command = parts.next().unwrap_or_default().trim();
        let Some(name) = app_name_from_process_command(command) else {
            continue;
        };
        if let Some(filter) = app_filter {
            if !app_name_matches(&name, filter) {
                continue;
            }
        }
        if apps.iter().any(|app: &AppInfo| app.name == name) {
            continue;
        }
        let is_frontmost = frontmost_name
            .map(|frontmost| app_name_equals(&name, frontmost))
            .unwrap_or(false);
        apps.push(AppInfo {
            app_id: Some(name.clone()),
            platform_app_id: None,
            name,
            pid,
            bundle_id: None,
            is_frontmost,
            windows: Vec::new(),
        });
    }
    apps
}

fn app_name_from_process_command(command: &str) -> Option<String> {
    let marker = ".app/Contents/";
    let end = command.find(marker)? + ".app".len();
    command[..end]
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".app"))
        .and_then(non_empty_string)
}

fn app_name_matches(name: &str, filter: &str) -> bool {
    name.to_lowercase().contains(&filter.to_lowercase())
}

fn app_identity_matches(name: &str, bundle_id: Option<&str>, filter: &str) -> bool {
    app_name_matches(name, filter)
        || bundle_id.is_some_and(|bundle_id| app_name_matches(bundle_id, filter))
}

fn app_name_equals(name: &str, other: &str) -> bool {
    name.eq_ignore_ascii_case(other)
}

fn resolve_app_filter_for_script(app_filter: Option<&str>) -> Option<String> {
    let app_filter = non_empty_string(app_filter?)?;
    if looks_like_bundle_id(&app_filter) {
        application_name_from_bundle_id(&app_filter).or(Some(app_filter))
    } else {
        Some(app_filter)
    }
}

fn wait_for_frontmost_app(app: &str, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        if frontmost_app_info(Some(app)).is_some() || window_server_frontmost_app_matches(app) {
            return true;
        }
        if started.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn window_server_frontmost_app_matches(app: &str) -> bool {
    window_server_app_list_result(None)
        .ok()
        .and_then(|apps| apps.into_iter().find(|app| app.is_frontmost))
        .is_some_and(|frontmost| {
            app_identity_matches(&frontmost.name, frontmost.bundle_id.as_deref(), app)
        })
}

fn app_activate_script(app_reference: &str) -> String {
    let app_json = serde_json::to_string(app_reference).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"import AppKit
let wanted = {app_json}
let normalized = wanted.lowercased()
let matches = NSWorkspace.shared.runningApplications.filter {{
    $0.activationPolicy == .regular &&
    ((($0.localizedName ?? "").lowercased().contains(normalized)) ||
     (($0.bundleIdentifier ?? "").lowercased().contains(normalized)))
}}
let exact = matches.first(where: {{
    (($0.localizedName ?? "").lowercased() == normalized) ||
    (($0.bundleIdentifier ?? "").lowercased() == normalized)
}})
if let app = exact ?? matches.first {{
    app.activate()
    print(app.localizedName ?? app.bundleIdentifier ?? "")
}}"#
    )
}

fn activate_app_with_appkit(app_reference: &str) -> SootieResult<Option<String>> {
    run_swift(
        &app_activate_script(app_reference),
        Duration::from_millis(APP_STATE_SCRIPT_TIMEOUT_MS),
    )
    .map(|output| non_empty_string(output.trim()))
}

fn app_focus_script(app: &str) -> String {
    format!(
        r#"tell application "System Events"
  if exists process "{app}" then
    set frontmost of process "{app}" to true
    try
      set value of attribute "AXFrontmost" of process "{app}" to true
    end try
  end if
end tell"#,
        app = esc(app)
    )
}

fn minimal_app_info(name: String, is_frontmost: bool) -> AppInfo {
    AppInfo {
        app_id: Some(name.clone()),
        platform_app_id: None,
        name,
        pid: None,
        bundle_id: None,
        is_frontmost,
        windows: Vec::new(),
    }
}

fn app_state_timeout_ms(app: Option<&str>) -> u64 {
    if app.is_some() {
        APP_STATE_SCRIPT_TIMEOUT_MS
    } else {
        APP_LIST_SCRIPT_TIMEOUT_MS
    }
}

fn screen_capture_display_unavailable(message: &str) -> bool {
    let message = message.to_lowercase();
    message.contains("could not create image from display")
        || message.contains("does not intersect any displays")
}

fn app_snapshot_script(app: Option<&str>) -> String {
    let selector = match app {
        Some(app) => format!(
            r#"repeat with candidateProcess in (every process whose background only is false)
  try
    set candidateName to name of candidateProcess as text
    set candidateBundleId to ""
    try
      set candidateBundleId to bundle identifier of candidateProcess as text
    end try
    if candidateName contains "{}" or candidateBundleId contains "{}" then
      set targetProcess to candidateProcess
      exit repeat
    end if
  end try
end repeat"#,
            esc(app),
            esc(app)
        ),
        None => r#"try
  set targetProcess to first process whose frontmost is true
end try"#
            .to_string(),
    };
    format!(
        r#"tell application "System Events"
set oldDelimiters to AppleScript's text item delimiters
set rowSep to character id 30
set fieldSep to character id 31
set winSep to character id 29
set winFieldSep to character id 28
set appRows to {{}}
set targetProcess to missing value
{}
if targetProcess is missing value then return ""
set appName to name of targetProcess
set fronted to frontmost of targetProcess
set appPid to ""
set appBundleId to ""
try
  set appPid to unix id of targetProcess as text
end try
try
  set appBundleId to bundle identifier of targetProcess as text
end try
	set winRows to {{}}
	repeat with w in windows of targetProcess
	  try
	    set winName to ""
	    set winX to ""
	    set winY to ""
	    set winWidth to ""
	    set winHeight to ""
	    set winName to name of w
	    try
	      set winPosition to position of w
	      set winX to item 1 of winPosition as text
	      set winY to item 2 of winPosition as text
	    end try
	    try
	      set winSize to size of w
	      set winWidth to item 1 of winSize as text
	      set winHeight to item 2 of winSize as text
	    end try
	    set AppleScript's text item delimiters to winFieldSep
	    set end of winRows to {{winName, winX, winY, winWidth, winHeight}} as text
	  end try
	end repeat
set AppleScript's text item delimiters to winSep
set winText to winRows as text
set AppleScript's text item delimiters to fieldSep
set end of appRows to {{appName, (fronted as text), appPid, appBundleId, winText}} as text
set AppleScript's text item delimiters to rowSep
set resultText to appRows as text
set AppleScript's text item delimiters to oldDelimiters
return resultText
end tell"#,
        selector
    )
}

fn app_list_script() -> &'static str {
    r#"tell application "System Events"
	set oldDelimiters to AppleScript's text item delimiters
	set rowSep to character id 30
	set fieldSep to character id 31
	set appRows to {}
	repeat with candidateProcess in processes
	  try
	    if not (background only of candidateProcess) then
	      set appName to name of candidateProcess as text
	      set fronted to frontmost of candidateProcess as text
	      set appPid to ""
	      set appBundleId to ""
	      try
	        set appPid to unix id of candidateProcess as text
	      end try
	      try
	        set appBundleId to bundle identifier of candidateProcess as text
	      end try
	      set AppleScript's text item delimiters to fieldSep
	      set end of appRows to {appName, fronted, appPid, appBundleId, ""} as text
	    end if
	  end try
	end repeat
	set AppleScript's text item delimiters to rowSep
	set resultText to appRows as text
	set AppleScript's text item delimiters to oldDelimiters
	return resultText
	end tell"#
}

fn window_title_snapshot_script(app_name: &str) -> String {
    format!(
        r#"tell application "System Events"
	set oldDelimiters to AppleScript's text item delimiters
	set fieldSep to character id 31
	set winSep to character id 29
	set winFieldSep to character id 28
	if not (exists process "{}") then return ""
	set winRows to {{}}
	tell process "{}"
	  set appName to name
	  set fronted to frontmost
	  set appPid to ""
	  set appBundleId to ""
	  try
	    set appPid to unix id as text
	  end try
	  try
	    set appBundleId to bundle identifier as text
	  end try
	  repeat with w in windows
	    try
	      set winName to name of w
	      if winName is not "" then
	        set winX to ""
	        set winY to ""
	        set winWidth to ""
	        set winHeight to ""
	        try
	          set winPosition to position of w
	          set winX to item 1 of winPosition as text
	          set winY to item 2 of winPosition as text
	        end try
	        try
	          set winSize to size of w
	          set winWidth to item 1 of winSize as text
	          set winHeight to item 2 of winSize as text
	        end try
	        set AppleScript's text item delimiters to winFieldSep
	        set end of winRows to {{winName, winX, winY, winWidth, winHeight}} as text
	      end if
	    end try
	  end repeat
	end tell
	set AppleScript's text item delimiters to winSep
	set winText to winRows as text
	set AppleScript's text item delimiters to fieldSep
	set resultText to {{appName, (fronted as text), appPid, appBundleId, winText}} as text
	set AppleScript's text item delimiters to oldDelimiters
	return resultText
end tell"#,
        esc(app_name),
        esc(app_name)
    )
}

fn application_window_snapshot_script(app_name: &str) -> String {
    format!(
        r#"set oldDelimiters to AppleScript's text item delimiters
set rowSep to character id 30
set fieldSep to character id 31
set winSep to character id 29
set winFieldSep to character id 28
set appPid to ""
set appBundleId to ""
set fronted to "false"
tell application "System Events"
  if not (exists process "{}") then return ""
  tell process "{}"
    set fronted to frontmost as text
    try
      set appPid to unix id as text
    end try
    try
      set appBundleId to bundle identifier as text
    end try
  end tell
end tell
set winRows to {{}}
tell application "{}"
  if (count of windows) is 0 then return ""
  set windowIndex to 1
  repeat with w in windows
    set winName to ""
    set winX to ""
    set winY to ""
    set winWidth to ""
    set winHeight to ""
    try
      set winName to name of w as text
    end try
    if winName is "" then
      try
        set winName to title of w as text
      end try
    end if
    if winName is "" then
      set winName to "{} window " & (windowIndex as text)
    end if
    try
      set boundsValue to bounds of w
      set leftValue to item 1 of boundsValue
      set topValue to item 2 of boundsValue
      set rightValue to item 3 of boundsValue
      set bottomValue to item 4 of boundsValue
      set winX to leftValue as text
      set winY to topValue as text
      set winWidth to (rightValue - leftValue) as text
      set winHeight to (bottomValue - topValue) as text
    end try
    set AppleScript's text item delimiters to winFieldSep
    set end of winRows to {{winName, winX, winY, winWidth, winHeight}} as text
    set windowIndex to windowIndex + 1
  end repeat
end tell
set AppleScript's text item delimiters to winSep
set winText to winRows as text
set AppleScript's text item delimiters to fieldSep
set resultText to {{"{}", fronted, appPid, appBundleId, winText}} as text
set AppleScript's text item delimiters to oldDelimiters
return resultText"#,
        esc(app_name),
        esc(app_name),
        esc(app_name),
        esc(app_name),
        esc(app_name)
    )
}

fn window_server_snapshot_script(app_name: &str) -> String {
    let app_name_json = serde_json::to_string(app_name).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"ObjC.import('CoreGraphics');
ObjC.import('Foundation');
const wanted = {app_name_json};
const rowSep = String.fromCharCode(30);
const fieldSep = String.fromCharCode(31);
const winSep = String.fromCharCode(29);
const winFieldSep = String.fromCharCode(28);
function unwrap(value) {{
  if (!value) return '';
  try {{ return ObjC.unwrap(value); }} catch (_) {{ return ''; }}
}}
function raw(item, key) {{
  return unwrap(item.objectForKey(key));
}}
const windows = ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, $.kCGNullWindowID));
let rows = [];
let appPid = '';
let ownerName = wanted;
for (let i = 0; i < windows.count; i++) {{
  const item = windows.objectAtIndex(i);
  const owner = String(raw(item, 'kCGWindowOwnerName'));
  if (owner.toLowerCase() !== wanted.toLowerCase()) continue;
  const layer = Number(raw(item, 'kCGWindowLayer') || 0);
  if (layer !== 0) continue;
  const bounds = ObjC.deepUnwrap(item.objectForKey('kCGWindowBounds')) || {{}};
  const width = Number(bounds.Width || 0);
  const height = Number(bounds.Height || 0);
  if (width <= 0 || height <= 0) continue;
  appPid = String(raw(item, 'kCGWindowOwnerPID') || appPid);
  ownerName = owner || ownerName;
  const fallbackName = ownerName + ' window ' + String(rows.length + 1);
  const winName = String(raw(item, 'kCGWindowName') || fallbackName);
  rows.push([winName, String(Number(bounds.X || 0)), String(Number(bounds.Y || 0)), String(width), String(height)].join(winFieldSep));
}}
rows.length === 0 ? '' : [ownerName, 'false', appPid, '', rows.join(winSep)].join(fieldSep);"#,
        app_name_json = app_name_json
    )
}

fn window_server_app_list(app_filter: Option<&str>) -> Vec<AppInfo> {
    window_server_app_list_result(app_filter).unwrap_or_default()
}

fn window_server_app_list_result(app_filter: Option<&str>) -> SootieResult<Vec<AppInfo>> {
    let output = run_jxa(
        &window_server_app_list_script(app_filter),
        Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
    )?;
    Ok(parse_app_state_rows(&output, app_filter))
}

fn window_server_app_list_script(app_filter: Option<&str>) -> String {
    let wanted_json = app_filter
        .and_then(non_empty_string)
        .map(|value| serde_json::to_string(&value).unwrap_or_else(|_| "\"\"".to_string()))
        .unwrap_or_else(|| "null".to_string());
    format!(
        r#"ObjC.import('CoreGraphics');
ObjC.import('Foundation');
const wanted = {wanted_json};
const rowSep = String.fromCharCode(30);
const fieldSep = String.fromCharCode(31);
const winSep = String.fromCharCode(29);
const winFieldSep = String.fromCharCode(28);
function unwrap(value) {{
  if (!value) return '';
  try {{ return ObjC.unwrap(value); }} catch (_) {{ return ''; }}
}}
function raw(item, key) {{
  return unwrap(item.objectForKey(key));
}}
function matches(owner) {{
  if (!wanted) return true;
  return owner.toLowerCase().indexOf(String(wanted).toLowerCase()) !== -1;
}}
const windows = ObjC.castRefToObject($.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, $.kCGNullWindowID));
let rows = [];
let byOwner = {{}};
for (let i = 0; i < windows.count; i++) {{
  const item = windows.objectAtIndex(i);
  const owner = String(raw(item, 'kCGWindowOwnerName'));
  if (!owner || !matches(owner)) continue;
  const layer = Number(raw(item, 'kCGWindowLayer') || 0);
  if (layer !== 0) continue;
  const bounds = ObjC.deepUnwrap(item.objectForKey('kCGWindowBounds')) || {{}};
  const width = Number(bounds.Width || 0);
  const height = Number(bounds.Height || 0);
  if (width <= 0 || height <= 0) continue;
  const ownerPid = String(raw(item, 'kCGWindowOwnerPID') || '');
  if (!byOwner[owner]) byOwner[owner] = {{ pid: ownerPid, windows: [] }};
  const fallbackName = owner + ' window ' + String(byOwner[owner].windows.length + 1);
  const winName = String(raw(item, 'kCGWindowName') || fallbackName);
  byOwner[owner].windows.push([winName, String(Number(bounds.X || 0)), String(Number(bounds.Y || 0)), String(width), String(height)].join(winFieldSep));
  if (rows.indexOf(owner) === -1) rows.push(owner);
}}
let appRows = [];
for (let i = 0; i < rows.length; i++) {{
  const owner = rows[i];
  const record = byOwner[owner];
  appRows.push([owner, i === 0 ? 'true' : 'false', record.pid, '', record.windows.join(winSep)].join(fieldSep));
}}
appRows.join(rowSep);"#,
        wanted_json = wanted_json
    )
}

fn ax_window_action_script(
    app_reference: &str,
    window: Option<&str>,
    action: &str,
    bounds: Option<Bounds>,
) -> String {
    let app_json = serde_json::to_string(app_reference).unwrap_or_else(|_| "\"\"".to_string());
    let window_json = window
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()))
        .unwrap_or_else(|| "nil".to_string());
    let bounds_code = bounds
        .map(|bounds| {
            format!(
                "CGRect(x: {}, y: {}, width: {}, height: {})",
                bounds.x.round(),
                bounds.y.round(),
                bounds.width.round().max(1.0),
                bounds.height.round().max(1.0)
            )
        })
        .unwrap_or_else(|| "nil".to_string());
    format!(
        r#"import AppKit
import ApplicationServices
import Darwin
import Foundation

func fail(_ message: String, _ code: Int32 = 2) -> Never {{
    FileHandle.standardError.write(Data(message.utf8))
    exit(code)
}}

func attrString(_ element: AXUIElement, _ attr: CFString) -> String {{
    var value: CFTypeRef?
    let err = AXUIElementCopyAttributeValue(element, attr, &value)
    if err != .success {{ return "" }}
    return value as? String ?? ""
}}

let wanted = {app_json}
let normalized = wanted.lowercased()
let matches = NSWorkspace.shared.runningApplications.filter {{
    $0.activationPolicy == .regular &&
    ((($0.localizedName ?? "").lowercased().contains(normalized)) ||
     (($0.bundleIdentifier ?? "").lowercased().contains(normalized)))
}}
let exact = matches.first(where: {{
    (($0.localizedName ?? "").lowercased() == normalized) ||
    (($0.bundleIdentifier ?? "").lowercased() == normalized)
}})
guard let app = exact ?? matches.first else {{
    fail("application not found: \(wanted)")
}}

app.activate()
let axApp = AXUIElementCreateApplication(app.processIdentifier)
var rawWindows: CFTypeRef?
let windowsError = AXUIElementCopyAttributeValue(axApp, kAXWindowsAttribute as CFString, &rawWindows)
guard windowsError == .success, let windows = rawWindows as? [AXUIElement], !windows.isEmpty else {{
    fail("window not found: \(app.localizedName ?? wanted)")
}}

let wantedWindow: String? = {window_json}
let target = wantedWindow.flatMap {{ name in
    let normalizedName = name.lowercased()
    return windows.first {{ attrString($0, kAXTitleAttribute as CFString).lowercased().contains(normalizedName) }}
}} ?? windows[0]
let title = attrString(target, kAXTitleAttribute as CFString)
let action = "{action}"
let rect: CGRect? = {bounds_code}

switch action {{
case "focus":
    _ = AXUIElementSetAttributeValue(target, kAXMinimizedAttribute as CFString, false as CFTypeRef)
    _ = AXUIElementSetAttributeValue(target, kAXMainAttribute as CFString, true as CFTypeRef)
    _ = AXUIElementPerformAction(target, kAXRaiseAction as CFString)
case "restore":
    _ = AXUIElementSetAttributeValue(target, kAXMinimizedAttribute as CFString, false as CFTypeRef)
    _ = AXUIElementSetAttributeValue(target, kAXMainAttribute as CFString, true as CFTypeRef)
    _ = AXUIElementPerformAction(target, kAXRaiseAction as CFString)
case "minimize":
    let err = AXUIElementSetAttributeValue(target, kAXMinimizedAttribute as CFString, true as CFTypeRef)
    if err != .success {{ fail("minimize failed: \(err.rawValue)") }}
case "maximize":
    let err = AXUIElementPerformAction(target, "AXZoomWindow" as CFString)
    if err != .success {{ fail("maximize failed: \(err.rawValue)") }}
case "close":
    var rawButton: CFTypeRef?
    let buttonError = AXUIElementCopyAttributeValue(target, kAXCloseButtonAttribute as CFString, &rawButton)
    guard buttonError == .success, let closeButton = rawButton else {{
        fail("close button not found")
    }}
    let err = AXUIElementPerformAction(closeButton as! AXUIElement, kAXPressAction as CFString)
    if err != .success {{ fail("close failed: \(err.rawValue)") }}
case "move", "resize":
    guard var rect = rect else {{ fail("bounds required") }}
    var point = rect.origin
    var size = rect.size
    guard let pointValue = AXValueCreate(.cgPoint, &point),
          let sizeValue = AXValueCreate(.cgSize, &size) else {{
        fail("bounds value creation failed")
    }}
    let posErr = AXUIElementSetAttributeValue(target, kAXPositionAttribute as CFString, pointValue)
    let sizeErr = AXUIElementSetAttributeValue(target, kAXSizeAttribute as CFString, sizeValue)
    if posErr != .success || sizeErr != .success {{
        fail("bounds update failed: \(posErr.rawValue)/\(sizeErr.rawValue)")
    }}
default:
    fail("unknown window action: \(action)")
}}

print("\(app.localizedName ?? wanted)\u{{1f}}\(action)\u{{1f}}\(title)")
"#
    )
}

fn run_ax_window_action(
    app_reference: &str,
    window: Option<&str>,
    action: &str,
    bounds: Option<Bounds>,
) -> SootieResult<ActionResult> {
    let output = run_swift(
        &ax_window_action_script(app_reference, window, action, bounds.clone()),
        Duration::from_millis(APP_STATE_SCRIPT_TIMEOUT_MS),
    )?;
    let mut fields = output.trim().split('\u{1f}');
    let app_name = fields
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(app_reference);
    let reported_action = fields
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(action);
    let reported_window = fields.next().filter(|value| !value.is_empty()).or(window);
    let mut result = macos_window_result(reported_action, app_name, reported_window, bounds);
    result.method = "accessibility".to_string();
    Ok(result)
}

fn macos_window_result(
    action: &str,
    app: &str,
    window: Option<&str>,
    bounds: Option<Bounds>,
) -> ActionResult {
    let mut details = json!({ "app": app, "action": action });
    if let Some(window) = window {
        details["window"] = json!(window);
    }
    if let Some(bounds) = bounds {
        details["bounds"] = json!(bounds);
    }
    ActionResult {
        method: "osascript".to_string(),
        details,
    }
}

fn mouse_move_script(x: f64, y: f64) -> String {
    format!(
        r#"ObjC.import("CoreGraphics");
const point = $.CGPointMake({}, {});
const event = $.CGEventCreateMouseEvent(null, $.kCGEventMouseMoved, point, $.kCGMouseButtonLeft);
$.CGEventPost($.kCGHIDEventTap, event);
"ok";"#,
        x.round(),
        y.round()
    )
}

fn mouse_click_script(x: f64, y: f64, button: &str, count: u32) -> SootieResult<String> {
    let (button_constant, down_event, up_event) = mouse_button_constants(button)?;
    Ok(format!(
        r#"ObjC.import("CoreGraphics");
const point = $.CGPointMake({}, {});
const button = $.{};
for (let i = 0; i < {}; i++) {{
  const down = $.CGEventCreateMouseEvent(null, $.{}, point, button);
  $.CGEventPost($.kCGHIDEventTap, down);
  const up = $.CGEventCreateMouseEvent(null, $.{}, point, button);
  $.CGEventPost($.kCGHIDEventTap, up);
}}
"ok";"#,
        x.round(),
        y.round(),
        button_constant,
        count.max(1),
        down_event,
        up_event
    ))
}

fn mouse_button_constants(
    button: &str,
) -> SootieResult<(&'static str, &'static str, &'static str)> {
    match button {
        "left" => Ok((
            "kCGMouseButtonLeft",
            "kCGEventLeftMouseDown",
            "kCGEventLeftMouseUp",
        )),
        "right" => Ok((
            "kCGMouseButtonRight",
            "kCGEventRightMouseDown",
            "kCGEventRightMouseUp",
        )),
        "middle" | "center" => Ok((
            "kCGMouseButtonCenter",
            "kCGEventOtherMouseDown",
            "kCGEventOtherMouseUp",
        )),
        other => Err(SootieError::InvalidArguments(format!(
            "unsupported mouse button '{other}'"
        ))),
    }
}

fn mouse_button_event_script(x: f64, y: f64, button: &str, pressed: bool) -> SootieResult<String> {
    let (button_constant, down_event, up_event) = mouse_button_constants(button)?;
    let event = if pressed { down_event } else { up_event };
    Ok(format!(
        r#"ObjC.import("CoreGraphics");
const point = $.CGPointMake({}, {});
const button = $.{};
const event = $.CGEventCreateMouseEvent(null, $.{}, point, button);
$.CGEventPost($.kCGHIDEventTap, event);
"ok";"#,
        x.round(),
        y.round(),
        button_constant,
        event
    ))
}

fn mouse_scroll_script(direction: &str, amount: i32) -> SootieResult<String> {
    let amount = amount.abs().max(1);
    let (delta_x, delta_y) = match direction {
        "up" => (0, amount),
        "down" => (0, -amount),
        "left" => (amount, 0),
        "right" => (-amount, 0),
        other => {
            return Err(SootieError::InvalidArguments(format!(
                "unsupported scroll direction '{other}'"
            )));
        }
    };
    Ok(format!(
        r#"ObjC.import("CoreGraphics");
const event = $.CGEventCreateScrollWheelEvent(null, $.kCGScrollEventUnitLine, 2, {}, {});
$.CGEventPost($.kCGHIDEventTap, event);
"ok";"#,
        delta_y, delta_x
    ))
}

fn mouse_drag_script(from_x: f64, from_y: f64, to_x: f64, to_y: f64) -> String {
    let steps = 12;
    format!(
        r#"ObjC.import("CoreGraphics");
const button = $.kCGMouseButtonLeft;
const fromX = {};
const fromY = {};
const toX = {};
const toY = {};
function point(x, y) {{
  return $.CGPointMake(Math.round(x), Math.round(y));
}}
function post(type, x, y) {{
  const event = $.CGEventCreateMouseEvent(null, type, point(x, y), button);
  $.CGEventPost($.kCGHIDEventTap, event);
}}
post($.kCGEventMouseMoved, fromX, fromY);
post($.kCGEventLeftMouseDown, fromX, fromY);
for (let i = 1; i <= {}; i++) {{
  const t = i / {};
  post($.kCGEventLeftMouseDragged, fromX + ((toX - fromX) * t), fromY + ((toY - fromY) * t));
}}
post($.kCGEventLeftMouseUp, toX, toY);
"ok";"#,
        from_x.round(),
        from_y.round(),
        to_x.round(),
        to_y.round(),
        steps,
        steps
    )
}

fn keyboard_event_script(key_code: u16, modifiers: &[String]) -> SootieResult<String> {
    let flags = macos_modifier_flag_expression(modifiers)?;
    Ok(format!(
        r#"ObjC.import("CoreGraphics");
const keyCode = {};
const flags = {};
function post(down) {{
  const event = $.CGEventCreateKeyboardEvent(null, keyCode, down);
  $.CGEventSetFlags(event, flags);
  $.CGEventPost($.kCGHIDEventTap, event);
}}
post(true);
post(false);
"ok";"#,
        key_code, flags
    ))
}

fn post_keyboard_event(key_code: u16, modifiers: &[String]) -> SootieResult<()> {
    run_jxa(
        &keyboard_event_script(key_code, modifiers)?,
        Duration::from_millis(KEYBOARD_EVENT_TIMEOUT_MS),
    )?;
    Ok(())
}

fn paste_text_with_coregraphics(text: &str, clear: bool) -> SootieResult<()> {
    command_output_stdin_timeout(
        "pbcopy",
        &[],
        Some(text.as_bytes()),
        Duration::from_millis(KEYBOARD_EVENT_TIMEOUT_MS),
    )?;
    if clear {
        post_keyboard_event(
            virtual_key_code("a").expect("a has a virtual key code"),
            &["command".to_string()],
        )?;
    }
    post_keyboard_event(
        virtual_key_code("v").expect("v has a virtual key code"),
        &["command".to_string()],
    )
}

fn accessibility_elements_script(app_name: &str, max_elements: usize) -> String {
    format!(
        r#"tell application "System Events"
set oldDelimiters to AppleScript's text item delimiters
set rowSep to character id 30
set fieldSep to character id 31
set outputRows to {{}}
if not (exists process "{}") then return ""
tell process "{}"
  if (count of windows) is 0 then return ""
  set targetWindow to front window
  try
    set elementList to entire contents of targetWindow
  on error
    set elementList to UI elements of targetWindow
  end try
  repeat with e in elementList
    if (count of outputRows) is greater than or equal to {} then exit repeat
    set roleText to ""
    set nameText to ""
    set valueText to ""
    set idText to ""
    set xText to ""
    set yText to ""
    set widthText to ""
    set heightText to ""
    set enabledText to ""
    set focusedText to ""
    try
      set roleText to role of e as text
    end try
    try
      set nameText to name of e as text
    end try
    if nameText is "" then
      try
        set nameText to description of e as text
      end try
    end if
    try
      set valueText to value of e as text
    end try
    try
      set idText to value of attribute "AXIdentifier" of e as text
    end try
    try
      set posValue to position of e
      set xText to item 1 of posValue as text
      set yText to item 2 of posValue as text
    end try
    try
      set sizeValue to size of e
      set widthText to item 1 of sizeValue as text
      set heightText to item 2 of sizeValue as text
    end try
    try
      set enabledText to enabled of e as text
    end try
    try
      set focusedText to focused of e as text
    end try
    set AppleScript's text item delimiters to fieldSep
    set end of outputRows to {{roleText, nameText, valueText, idText, xText, yText, widthText, heightText, enabledText, focusedText}} as text
  end repeat
end tell
set AppleScript's text item delimiters to rowSep
set resultText to outputRows as text
set AppleScript's text item delimiters to oldDelimiters
return resultText
end tell"#,
        esc(app_name),
        esc(app_name),
        max_elements.max(1)
    )
}

fn ax_accessibility_elements_script(app_reference: &str, max_elements: usize) -> String {
    let app_json = serde_json::to_string(app_reference).unwrap_or_else(|_| "\"\"".to_string());
    let max_elements = max_elements.max(1);
    format!(
        r#"import AppKit
import ApplicationServices
import Darwin
import Foundation

let wanted = {app_json}
let maxElements = {max_elements}
let rowSep = "\u{{1e}}"
let fieldSep = "\u{{1f}}"

func clean(_ value: String) -> String {{
    value
        .replacingOccurrences(of: rowSep, with: " ")
        .replacingOccurrences(of: fieldSep, with: " ")
        .replacingOccurrences(of: "\n", with: " ")
}}

func attr(_ element: AXUIElement, _ name: CFString) -> CFTypeRef? {{
    var value: CFTypeRef?
    let err = AXUIElementCopyAttributeValue(element, name, &value)
    if err != .success {{ return nil }}
    return value
}}

func attrString(_ element: AXUIElement, _ name: CFString) -> String {{
    guard let value = attr(element, name) else {{ return "" }}
    if let string = value as? String {{ return string }}
    if let number = value as? NSNumber {{ return number.stringValue }}
    return String(describing: value)
}}

func attrBool(_ element: AXUIElement, _ name: CFString) -> String {{
    guard let value = attr(element, name) else {{ return "" }}
    if let bool = value as? Bool {{ return bool ? "true" : "false" }}
    if let number = value as? NSNumber {{ return number.boolValue ? "true" : "false" }}
    return ""
}}

func pointValue(_ element: AXUIElement) -> CGPoint? {{
    guard let value = attr(element, kAXPositionAttribute as CFString) else {{ return nil }}
    var point = CGPoint.zero
    if AXValueGetValue(value as! AXValue, .cgPoint, &point) {{ return point }}
    return nil
}}

func sizeValue(_ element: AXUIElement) -> CGSize? {{
    guard let value = attr(element, kAXSizeAttribute as CFString) else {{ return nil }}
    var size = CGSize.zero
    if AXValueGetValue(value as! AXValue, .cgSize, &size) {{ return size }}
    return nil
}}

func children(_ element: AXUIElement, _ name: CFString) -> [AXUIElement] {{
    guard let value = attr(element, name), let children = value as? [AXUIElement] else {{ return [] }}
    return children
}}

let normalized = wanted.lowercased()
let matches = NSWorkspace.shared.runningApplications.filter {{
    $0.activationPolicy == .regular &&
    ((($0.localizedName ?? "").lowercased().contains(normalized)) ||
     (($0.bundleIdentifier ?? "").lowercased().contains(normalized)))
}}
let exact = matches.first(where: {{
    (($0.localizedName ?? "").lowercased() == normalized) ||
    (($0.bundleIdentifier ?? "").lowercased() == normalized)
}})
guard let app = exact ?? matches.first else {{
    exit(0)
}}

let axApp = AXUIElementCreateApplication(app.processIdentifier)
var rawWindow: CFTypeRef?
var targetWindow: AXUIElement?
if AXUIElementCopyAttributeValue(axApp, kAXFocusedWindowAttribute as CFString, &rawWindow) == .success,
   let window = rawWindow {{
    targetWindow = (window as! AXUIElement)
}}
if targetWindow == nil,
   let windows = attr(axApp, kAXWindowsAttribute as CFString) as? [AXUIElement],
   !windows.isEmpty {{
    targetWindow = windows[0]
}}
guard let root = targetWindow else {{
    exit(0)
}}

let childAttrs = [kAXChildrenAttribute as CFString, "AXContents" as CFString]
var queue = [root]
var rows: [String] = []
var seen = Set<CFHashCode>()
var visited = 0
while !queue.isEmpty && rows.count < maxElements && visited < maxElements * 25 {{
    let element = queue.removeFirst()
    let elementKey = CFHash(element)
    if seen.contains(elementKey) {{ continue }}
    seen.insert(elementKey)
    visited += 1
    let role = attrString(element, kAXRoleAttribute as CFString)
    if !role.isEmpty {{
        let title = attrString(element, kAXTitleAttribute as CFString)
        let description = attrString(element, kAXDescriptionAttribute as CFString)
        let name = title.isEmpty ? description : title
        let value = attrString(element, kAXValueAttribute as CFString)
        let identifier = attrString(element, "AXIdentifier" as CFString)
        let point = pointValue(element)
        let size = sizeValue(element)
        let fields = [
            role,
            name,
            value,
            identifier,
            point.map {{ String(format: "%.0f", $0.x) }} ?? "",
            point.map {{ String(format: "%.0f", $0.y) }} ?? "",
            size.map {{ String(format: "%.0f", $0.width) }} ?? "",
            size.map {{ String(format: "%.0f", $0.height) }} ?? "",
            attrBool(element, kAXEnabledAttribute as CFString),
            attrBool(element, kAXFocusedAttribute as CFString)
        ].map(clean)
        rows.append(fields.joined(separator: fieldSep))
    }}
    for childAttr in childAttrs {{
        queue.append(contentsOf: children(element, childAttr))
    }}
}}
print(rows.joined(separator: rowSep))
"#
    )
}

fn accessibility_elements(app_name: &str) -> Vec<AccessibilityElement> {
    match run_swift(
        &ax_accessibility_elements_script(app_name, ACCESSIBILITY_ELEMENT_LIMIT),
        Duration::from_millis(ACCESSIBILITY_SCRIPT_TIMEOUT_MS),
    ) {
        Ok(output) => output
            .split('\u{1e}')
            .filter_map(parse_accessibility_element_row)
            .collect(),
        Err(swift_error) => match osascript_with_timeout(
            &accessibility_elements_script(app_name, ACCESSIBILITY_ELEMENT_LIMIT),
            Duration::from_millis(ACCESSIBILITY_SCRIPT_TIMEOUT_MS),
        ) {
            Ok(output) => output
                .split('\u{1e}')
                .filter_map(parse_accessibility_element_row)
                .collect(),
            Err(error) => {
                tracing::debug!(
                    %swift_error,
                    %error,
                    app = app_name,
                    "macOS accessibility element enumeration failed"
                );
                Vec::new()
            }
        },
    }
}

fn parse_accessibility_element_row(row: &str) -> Option<AccessibilityElement> {
    let fields = row.split('\u{1f}').collect::<Vec<_>>();
    if fields.len() < 10 {
        return None;
    }
    let role = fields[0].trim();
    if role.is_empty() {
        return None;
    }
    let name = non_empty_string(fields[1]);
    let text = non_empty_string(fields[2]);
    let id = non_empty_string(fields[3]);
    let bounds = match (
        parse_optional_f64(fields[4]),
        parse_optional_f64(fields[5]),
        parse_optional_f64(fields[6]),
        parse_optional_f64(fields[7]),
    ) {
        (Some(x), Some(y), Some(width), Some(height)) if width > 0.0 && height > 0.0 => {
            Some(Bounds {
                x,
                y,
                width,
                height,
            })
        }
        _ => None,
    };
    let enabled = parse_optional_bool(fields[8]);
    let focused = parse_optional_bool(fields[9]).unwrap_or(false);
    let editable = editable_role(role);
    let title = name.clone().or_else(|| text.clone());
    Some(AccessibilityElement {
        focused,
        element: ElementInfo {
            id,
            role: role.to_string(),
            title,
            name,
            text,
            bounds,
            actions: actions_for_accessibility_role(role, editable),
            editable: Some(editable),
            enabled,
        },
    })
}

fn parse_app_state_rows(output: &str, app_filter: Option<&str>) -> Vec<AppInfo> {
    let mut apps = Vec::new();
    for (index, row) in output.split('\u{1e}').enumerate() {
        let mut parts = row.split('\u{1f}');
        let name = parts.next().unwrap_or_default().trim().to_string();
        if name.is_empty() {
            continue;
        }
        let is_frontmost = parts.next().map(|v| v == "true").unwrap_or(false);
        let third_field = parts.next().unwrap_or("").trim();
        let fourth_field = parts.next();
        let fifth_field = parts.next();
        let (pid, bundle_id, window_text) = match (fourth_field, fifth_field) {
            (Some(bundle_id), Some(window_text)) => (
                third_field.parse::<u32>().ok(),
                non_empty_string(bundle_id),
                window_text.trim(),
            ),
            (Some(window_text), None) => {
                (third_field.parse::<u32>().ok(), None, window_text.trim())
            }
            _ => (None, None, third_field),
        };
        if let Some(filter) = app_filter {
            if !app_identity_matches(&name, bundle_id.as_deref(), filter) {
                continue;
            }
        }
        let windows = window_text
            .split('\u{1d}')
            .enumerate()
            .filter_map(|(window_index, window_row)| {
                if window_row.trim().is_empty() {
                    return None;
                }
                let fields = window_row.split('\u{1c}').collect::<Vec<_>>();
                let title = fields.first().copied().unwrap_or_default().trim();
                if title.is_empty() {
                    return None;
                }
                let bounds = match (
                    fields.get(1).and_then(|value| value.parse::<f64>().ok()),
                    fields.get(2).and_then(|value| value.parse::<f64>().ok()),
                    fields.get(3).and_then(|value| value.parse::<f64>().ok()),
                    fields.get(4).and_then(|value| value.parse::<f64>().ok()),
                ) {
                    (Some(x), Some(y), Some(width), Some(height)) => Some(Bounds {
                        x,
                        y,
                        width,
                        height,
                    }),
                    _ => None,
                };
                Some(WindowInfo {
                    id: Some(format!("win_{index}_{window_index}")),
                    title: title.to_string(),
                    bounds,
                    focused: is_frontmost && window_index == 0,
                })
            })
            .collect();
        apps.push(AppInfo {
            app_id: Some(name.clone()),
            platform_app_id: bundle_id.clone(),
            name,
            pid,
            bundle_id,
            is_frontmost,
            windows,
        });
    }
    apps
}

fn fill_missing_windows(apps: &mut [AppInfo], app_filter: Option<&str>) {
    let Some(app_filter) = app_filter else {
        return;
    };
    for app in apps.iter_mut().filter(|app| app.windows.is_empty()) {
        fill_app_windows(app, app_filter);
    }
}

fn fill_selected_app_windows(apps: &mut [AppInfo], app_filter: Option<&str>) {
    let Some(selected_index) = apps
        .iter()
        .position(|app| app.is_frontmost)
        .or_else(|| (!apps.is_empty()).then_some(0))
    else {
        return;
    };
    if !apps[selected_index].windows.is_empty() {
        return;
    }
    let app_name = app_filter
        .and_then(non_empty_string)
        .unwrap_or_else(|| apps[selected_index].name.clone());
    fill_app_windows(&mut apps[selected_index], &app_name);
}

fn fill_app_windows(app: &mut AppInfo, app_name: &str) {
    if try_fill_app_windows(app, &window_title_snapshot_script(app_name), app_name) {
        return;
    }
    if try_fill_app_windows_from_jxa(app, &window_server_snapshot_script(app_name), app_name) {
        return;
    }
    let _ = try_fill_app_windows(app, &application_window_snapshot_script(app_name), app_name);
}

fn try_fill_app_windows(app: &mut AppInfo, script: &str, app_name: &str) -> bool {
    let Ok(output) = osascript_with_timeout(
        script,
        Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
    ) else {
        return false;
    };
    fill_app_windows_from_output(app, &output, app_name)
}

fn try_fill_app_windows_from_jxa(app: &mut AppInfo, script: &str, app_name: &str) -> bool {
    let Ok(output) = run_jxa(
        script,
        Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
    ) else {
        return false;
    };
    fill_app_windows_from_output(app, &output, app_name)
}

fn fill_app_windows_from_output(app: &mut AppInfo, output: &str, app_name: &str) -> bool {
    let Some(mut enriched) = parse_app_state_rows(output, Some(app_name))
        .into_iter()
        .next()
    else {
        return false;
    };
    if enriched.windows.is_empty() {
        return false;
    }
    if app.pid.is_none() {
        app.pid = enriched.pid;
    }
    if app.bundle_id.is_none() {
        app.bundle_id = enriched.bundle_id.clone();
    }
    if app.platform_app_id.is_none() {
        app.platform_app_id = enriched.platform_app_id;
    }
    app.windows.append(&mut enriched.windows);
    true
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "missing value").then(|| value.to_string())
}

fn parse_optional_f64(value: &str) -> Option<f64> {
    non_empty_string(value)?.parse().ok()
}

fn parse_optional_bool(value: &str) -> Option<bool> {
    match value.trim().to_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn editable_role(role: &str) -> bool {
    let role = role.to_lowercase();
    role.contains("textfield")
        || role.contains("textarea")
        || role.contains("combobox")
        || role.contains("searchfield")
}

fn actions_for_accessibility_role(role: &str, editable: bool) -> Vec<String> {
    let role = role.to_lowercase();
    let mut actions = Vec::new();
    if editable {
        actions.push("setValue".to_string());
    }
    if role.contains("button")
        || role.contains("checkbox")
        || role.contains("radio")
        || role.contains("menuitem")
        || role.contains("link")
    {
        actions.push("click".to_string());
    }
    actions
}

fn element_matches_query(element: &ElementInfo, query: &FindQuery) -> bool {
    let matches_query = query
        .query
        .as_ref()
        .map(|needle| {
            let needle = needle.to_lowercase();
            element
                .name
                .as_ref()
                .is_some_and(|name| name.to_lowercase().contains(&needle))
                || element
                    .text
                    .as_ref()
                    .is_some_and(|text| text.to_lowercase().contains(&needle))
        })
        .unwrap_or(true);
    let matches_role = query
        .role
        .as_ref()
        .map(|wanted| {
            element.role.eq_ignore_ascii_case(wanted)
                || element.role.strip_prefix("AX").is_some_and(|stripped| {
                    stripped.eq_ignore_ascii_case(wanted)
                        || wanted
                            .strip_prefix("AX")
                            .is_some_and(|wanted| stripped.eq_ignore_ascii_case(wanted))
                })
        })
        .unwrap_or(true);
    let matches_id = query
        .identifier
        .as_ref()
        .or(query.dom_id.as_ref())
        .map(|wanted| element.id.as_ref().is_some_and(|id| id == wanted))
        .unwrap_or(true);
    matches_query && matches_role && matches_id
}

impl MacosBackend {
    fn selected_app(&self, app_filter: Option<&str>) -> Option<AppInfo> {
        let apps = self.state(app_filter).ok()?;
        apps.iter()
            .find(|app| app.is_frontmost)
            .cloned()
            .or_else(|| apps.into_iter().next())
    }

    fn cdp_port_for_query(&self, query: &FindQuery) -> Option<u16> {
        self.cdp_port_for_app_filter(query.app.as_deref())
    }

    fn cdp_port_for_app_filter(&self, app_filter: Option<&str>) -> Option<u16> {
        if let Some(port) = cdp::configured_port() {
            return Some(port);
        }
        if app_filter.is_none() {
            return first_browser_cdp_port();
        }
        let app = self.selected_app(app_filter)?;
        browser_cdp_port(&app.name)
    }

    fn screenshot_window(&self, app: Option<&str>) -> Option<WindowInfo> {
        let app = app?;
        let windows = self
            .state(Some(app))
            .ok()?
            .into_iter()
            .flat_map(|app| app.windows.into_iter())
            .collect::<Vec<_>>();
        windows
            .iter()
            .find(|window| window.focused)
            .cloned()
            .or_else(|| windows.into_iter().next())
    }

    fn capture_screenshot(&self, args: &[&str]) -> SootieResult<()> {
        let result = command_output_timeout(
            "screencapture",
            args,
            Duration::from_millis(SCREENSHOT_TIMEOUT_MS),
        );
        match result {
            Ok(_) => Ok(()),
            Err(error) if screen_capture_display_unavailable(&error.to_string()) => {
                let _ = command_output_timeout(
                    "caffeinate",
                    &["-u", "-t", "1"],
                    Duration::from_millis(1_500),
                );
                thread::sleep(Duration::from_millis(200));
                command_output_timeout(
                    "screencapture",
                    args,
                    Duration::from_millis(SCREENSHOT_TIMEOUT_MS),
                )
                .map(|_| ())
            }
            Err(error) => Err(error),
        }
    }

    fn resolve_point(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> SootieResult<(f64, f64)> {
        match (x, y) {
            (Some(x), Some(y)) => Ok((x, y)),
            _ => self
                .find(query)?
                .into_iter()
                .find_map(|element| element.bounds.map(|bounds| bounds.center()))
                .map(|point| (point.x, point.y))
                .ok_or_else(|| {
                    SootieError::NotFound("no coordinate or matching element".to_string())
                }),
        }
    }
}

impl DesktopBackend for MacosBackend {
    fn platform(&self) -> &'static str {
        "macos"
    }

    fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
        vec![
            macos_accessibility_diagnostic(),
            macos_window_server_diagnostic(),
        ]
    }

    fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot> {
        let script_app = resolve_app_filter_for_script(app);
        let mut apps = if app.is_none() {
            frontmost_app_info(None)
                .map(|app| vec![app])
                .unwrap_or_else(|| fallback_app_list_without_frontmost_retry(None))
        } else {
            match osascript_with_timeout(
                &app_snapshot_script(script_app.as_deref()),
                Duration::from_millis(APP_SNAPSHOT_SCRIPT_TIMEOUT_MS),
            ) {
                Ok(output) => {
                    let mut parsed = parse_app_state_rows(&output, app);
                    if parsed.is_empty() && script_app.as_deref() != app {
                        parsed = parse_app_state_rows(&output, script_app.as_deref());
                    }
                    if parsed.is_empty() {
                        self.state(app)?
                    } else {
                        parsed
                    }
                }
                Err(error) => {
                    tracing::debug!(
                        %error,
                        app = app.unwrap_or(""),
                        "macOS app snapshot failed; falling back to state"
                    );
                    self.state(app)?
                }
            }
        };
        fill_selected_app_windows(&mut apps, script_app.as_deref().or(app));
        let selected_app = apps
            .iter()
            .find(|app| app.is_frontmost)
            .or_else(|| apps.first());
        let selected_window = selected_app.and_then(|app| {
            app.windows
                .iter()
                .find(|window| window.focused)
                .or_else(|| app.windows.first())
        });
        let accessibility_records = selected_app
            .map(|app| accessibility_elements(&app.name))
            .unwrap_or_default();
        let cdp_elements = self
            .cdp_port_for_app_filter(app)
            .and_then(cdp::page_elements)
            .unwrap_or_default();
        let interactive_elements = if accessibility_records.is_empty() && cdp_elements.is_empty() {
            apps.iter()
                .flat_map(|app| {
                    app.windows
                        .iter()
                        .map(move |window| crate::backend::element_from_window(app, window))
                })
                .collect()
        } else {
            cdp_elements
                .into_iter()
                .chain(
                    accessibility_records
                        .iter()
                        .map(|record| record.element.clone()),
                )
                .collect()
        };
        let focused_element = accessibility_records
            .iter()
            .find(|record| record.focused)
            .map(|record| record.element.clone())
            .or_else(|| {
                selected_app
                    .zip(selected_window)
                    .map(|(app, window)| element_from_window(app, window))
            });
        Ok(ContextSnapshot {
            app: selected_app.map(|app| app.name.clone()),
            app_id: selected_app.and_then(|app| app.app_id.clone()),
            platform_app_id: selected_app.and_then(|app| app.platform_app_id.clone()),
            bundle_id: selected_app.and_then(|app| app.bundle_id.clone()),
            pid: selected_app.and_then(|app| app.pid),
            window: selected_window.map(|window| window.title.clone()),
            url: selected_app
                .and_then(|app| current_browser_url(&app.name))
                .or_else(|| {
                    self.cdp_port_for_app_filter(app)
                        .and_then(cdp::current_page_url)
                }),
            focused_element,
            interactive_elements,
        })
    }

    fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
        let script_app = resolve_app_filter_for_script(app);
        let script = script_app
            .as_deref()
            .map(window_title_snapshot_script)
            .unwrap_or_else(|| app_list_script().to_string());
        let timeout_ms = app_state_timeout_ms(app);
        match osascript_with_timeout(&script, Duration::from_millis(timeout_ms)) {
            Ok(output) => {
                let mut apps = parse_app_state_rows(&output, app);
                if apps.is_empty() && script_app.as_deref() != app {
                    apps = parse_app_state_rows(&output, script_app.as_deref());
                }
                fill_missing_windows(&mut apps, script_app.as_deref().or(app));
                if apps.is_empty() {
                    Ok(fallback_state_app_list(app))
                } else {
                    Ok(apps)
                }
            }
            Err(error) => {
                tracing::debug!(
                    %error,
                    app = app.unwrap_or(""),
                    "macOS app state failed; falling back to process list"
                );
                Ok(fallback_state_app_list_without_frontmost_retry(app))
            }
        }
    }

    fn find(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
        if has_element_target(query) {
            if let Some(cdp_elements) = self.cdp_port_for_query(query).and_then(cdp::page_elements)
            {
                let matches = cdp_elements
                    .into_iter()
                    .filter(|element| element_matches_query(element, query))
                    .collect::<Vec<_>>();
                if !matches.is_empty() {
                    return Ok(matches);
                }
            }
        }
        if query.app.is_none() {
            return Ok(self
                .context(None)?
                .interactive_elements
                .into_iter()
                .filter(|element| element_matches_query(element, query))
                .collect());
        }
        let mut apps = self.state(query.app.as_deref())?;
        fill_selected_app_windows(&mut apps, query.app.as_deref());
        let selected_app = apps
            .iter()
            .find(|app| app.is_frontmost)
            .or_else(|| apps.first());
        let mut elements = filter_elements(&apps, &FindQuery::default());
        if let Some(app) = selected_app {
            if let Some(cdp_elements) = self.cdp_port_for_query(query).and_then(cdp::page_elements)
            {
                elements.extend(cdp_elements);
            }
            elements.extend(
                accessibility_elements(&app.name)
                    .into_iter()
                    .map(|record| record.element),
            );
        }
        Ok(elements
            .into_iter()
            .filter(|element| element_matches_query(element, query))
            .collect())
    }

    fn read(
        &self,
        app: Option<&str>,
        query: Option<&str>,
        depth: Option<u32>,
    ) -> SootieResult<String> {
        if let Some(text) = self
            .cdp_port_for_app_filter(app)
            .and_then(|port| cdp::page_text(port, query, depth))
        {
            return Ok(text);
        }
        let query = FindQuery {
            app: app.map(str::to_string),
            query: query.map(str::to_string),
            depth,
            ..Default::default()
        };
        Ok(self
            .find(&query)?
            .into_iter()
            .filter_map(|element| element.name.or(element.text))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn inspect(&self, query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
        Ok(self.find(query)?.into_iter().next())
    }

    fn element_at(&self, x: f64, y: f64) -> SootieResult<Option<ElementInfo>> {
        Ok(element_at_from_elements(
            self.context(None)?.interactive_elements,
            x,
            y,
        ))
    }

    fn screenshot(&self, app: Option<&str>, _full_resolution: bool) -> SootieResult<Screenshot> {
        if let Some(screenshot) = self
            .cdp_port_for_app_filter(app)
            .and_then(cdp::page_screenshot)
        {
            return Ok(screenshot);
        }
        let window = self.screenshot_window(app);
        let path = tmp_screenshot_path("png");
        let path_str = path.to_string_lossy().to_string();
        if let Some(bounds) = window.as_ref().and_then(|window| window.bounds.as_ref()) {
            let region = format!(
                "{},{},{},{}",
                bounds.x.round() as i64,
                bounds.y.round() as i64,
                bounds.width.round().max(1.0) as i64,
                bounds.height.round().max(1.0) as i64
            );
            if self
                .capture_screenshot(&["-x", "-R", &region, &path_str])
                .is_err()
            {
                self.capture_screenshot(&["-x", &path_str])?;
            }
        } else {
            self.capture_screenshot(&["-x", &path_str])?;
        }
        let bytes = std::fs::read(&path)?;
        let _ = std::fs::remove_file(&path);
        let (width, height) = png_dimensions(&bytes);
        Ok(Screenshot {
            mime_type: "image/png".to_string(),
            data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            width,
            height,
            window_title: window.as_ref().map(|window| window.title.clone()),
            window_frame: window.and_then(|window| window.bounds),
        })
    }

    fn click(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        button: &str,
        count: u32,
    ) -> SootieResult<ActionResult> {
        if x.is_none() && y.is_none() && button == "left" && count <= 1 && has_element_target(query)
        {
            if let Some(result) = self
                .cdp_port_for_query(query)
                .and_then(|port| cdp::click_element(port, query))
            {
                return Ok(result);
            }
        }
        let (x, y) = self.resolve_point(x, y, query)?;
        run_jxa(
            &mouse_click_script(x, y, button, count)?,
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "x": x, "y": y, "button": button, "count": count.max(1) }),
        })
    }

    fn hover(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> SootieResult<ActionResult> {
        if x.is_none() && y.is_none() && has_element_target(query) {
            if let Some(result) = self
                .cdp_port_for_query(query)
                .and_then(|port| cdp::hover_element(port, query))
            {
                return Ok(result);
            }
        }
        let (x, y) = self.resolve_point(x, y, query)?;
        run_jxa(
            &mouse_move_script(x, y),
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "x": x, "y": y }),
        })
    }

    fn long_press(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        duration_secs: f64,
        button: &str,
    ) -> SootieResult<ActionResult> {
        if x.is_none() && y.is_none() && has_element_target(query) {
            if let Some(result) = self
                .cdp_port_for_query(query)
                .and_then(|port| cdp::long_press_element(port, query, duration_secs, button))
            {
                return Ok(result);
            }
        }
        let (x, y) = self.resolve_point(x, y, query)?;
        run_jxa(
            &mouse_button_event_script(x, y, button, true)?,
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        std::thread::sleep(std::time::Duration::from_secs_f64(duration_secs.max(0.0)));
        run_jxa(
            &mouse_button_event_script(x, y, button, false)?,
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "x": x, "y": y, "button": button, "duration": duration_secs }),
        })
    }

    fn drag(
        &self,
        from: Option<(f64, f64)>,
        to: (f64, f64),
        query: &FindQuery,
        duration_secs: f64,
        hold_duration_secs: f64,
    ) -> SootieResult<ActionResult> {
        if from.is_none() && has_element_target(query) {
            if let Some(result) = self.cdp_port_for_query(query).and_then(|port| {
                cdp::drag_element(port, query, to, duration_secs, hold_duration_secs)
            }) {
                return Ok(result);
            }
        }
        let (from_x, from_y) = match from {
            Some(p) => p,
            None => self.resolve_point(None, None, query)?,
        };
        std::thread::sleep(std::time::Duration::from_secs_f64(
            hold_duration_secs.max(0.0),
        ));
        run_jxa(
            &mouse_drag_script(from_x, from_y, to.0, to.1),
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({
                "from": { "x": from_x, "y": from_y },
                "to": { "x": to.0, "y": to.1 },
                "duration": duration_secs,
                "hold_duration": hold_duration_secs
            }),
        })
    }

    fn type_text(&self, text: &str, target: &FindQuery, clear: bool) -> SootieResult<ActionResult> {
        if has_element_target(target) {
            if let Some(result) = self
                .cdp_port_for_query(target)
                .and_then(|port| cdp::type_text_element(port, target, text, clear))
            {
                return Ok(result);
            }
            self.click(None, None, target, "left", 1)?;
        } else if let Some(app) = &target.app {
            self.focus(app, None, None)?;
        }
        paste_text_with_coregraphics(text, clear)?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "bytes": text.len(), "clear": clear }),
        })
    }

    fn press(
        &self,
        key: &str,
        modifiers: &[String],
        app: Option<&str>,
    ) -> SootieResult<ActionResult> {
        if let Some(result) = self
            .cdp_port_for_app_filter(app)
            .and_then(|port| cdp::press_key(port, key, modifiers))
        {
            return Ok(result);
        }
        if let Some(app) = app {
            self.focus(app, None, None)?;
        }
        let key_code = virtual_key_code(key)
            .ok_or_else(|| SootieError::InvalidArguments(format!("unknown macOS key '{key}'")))?;
        post_keyboard_event(key_code, modifiers)?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "key": key, "modifiers": modifiers }),
        })
    }

    fn hotkey(&self, keys: &[String], app: Option<&str>) -> SootieResult<ActionResult> {
        let Some((key, modifiers)) = keys.split_last() else {
            return Err(SootieError::InvalidArguments(
                "keys must not be empty".to_string(),
            ));
        };
        self.press(key, modifiers, app)
    }

    fn scroll(
        &self,
        direction: &str,
        amount: i32,
        app: Option<&str>,
        at: Option<(f64, f64)>,
    ) -> SootieResult<ActionResult> {
        if at.is_none() {
            if let Some(result) = self
                .cdp_port_for_app_filter(app)
                .and_then(|port| cdp::scroll_page(port, direction, amount))
            {
                return Ok(result);
            }
        }
        if let Some(app) = app {
            self.focus(app, None, None)?;
        }
        run_jxa(
            &mouse_scroll_script(direction, amount)?,
            Duration::from_millis(POINTER_EVENT_TIMEOUT_MS),
        )?;
        Ok(ActionResult {
            method: "coregraphics".to_string(),
            details: json!({ "direction": direction, "amount": amount }),
        })
    }

    fn focus(
        &self,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
    ) -> SootieResult<ActionResult> {
        let app_reference = platform_app_id.unwrap_or(app);
        let bundle_id = platform_app_id.or_else(|| looks_like_bundle_id(app).then_some(app));
        let mut foreground_error = None;
        let mut activated_with_appkit = false;
        let app_name = match activate_app_with_appkit(app_reference) {
            Ok(Some(name)) => {
                activated_with_appkit = true;
                name
            }
            Ok(None) | Err(_) => {
                let app_name = bundle_id
                    .and_then(application_name_from_bundle_id)
                    .unwrap_or_else(|| app.to_string());
                let activate_result = if let Some(bundle_id) = bundle_id {
                    osascript(&format!(
                        "tell application id \"{}\" to activate",
                        esc(bundle_id)
                    ))
                } else {
                    osascript(&format!("tell application \"{}\" to activate", esc(app)))
                };
                if let Err(error) = activate_result {
                    foreground_error = Some(error);
                }
                app_name
            }
        };
        if !activated_with_appkit && foreground_error.is_none() {
            foreground_error = osascript(&app_focus_script(&app_name)).err();
        }
        if let Some(window) = window {
            return run_ax_window_action(&app_name, Some(window), "focus", None);
        }
        if !wait_for_frontmost_app(&app_name, Duration::from_millis(FOCUS_CONFIRM_TIMEOUT_MS)) {
            let foreground_note = foreground_error
                .map(|error| format!(" foreground fallback failed: {error}."))
                .unwrap_or_default();
            return Err(SootieError::Platform(format!(
                "macOS did not make '{}' frontmost after activation;{} grant Accessibility and Automation permissions to the calling app, or bring the app forward manually and retry",
                app_name, foreground_note
            )));
        }
        let mut result = macos_window_result("focus", &app_name, window, None);
        if activated_with_appkit && window.is_none() {
            result.method = "appkit".to_string();
        }
        Ok(result)
    }

    fn window(
        &self,
        command: WindowCommand,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
        bounds: Option<Bounds>,
    ) -> SootieResult<ActionResult> {
        let app_reference = platform_app_id.unwrap_or(app);
        match command {
            WindowCommand::List => Ok(ActionResult {
                method: "backend".to_string(),
                details: json!({ "windows": self.state(Some(platform_app_id.unwrap_or(app)))? }),
            }),
            WindowCommand::Focus => self.focus(app, platform_app_id, window),
            WindowCommand::Restore => run_ax_window_action(app_reference, window, "restore", None),
            WindowCommand::Minimize => {
                run_ax_window_action(app_reference, window, "minimize", None)
            }
            WindowCommand::Maximize => {
                run_ax_window_action(app_reference, window, "maximize", None)
            }
            WindowCommand::Close => run_ax_window_action(app_reference, window, "close", None),
            WindowCommand::Move | WindowCommand::Resize => {
                let Some(bounds) = bounds else {
                    return Err(SootieError::InvalidArguments(
                        "move/resize requires x/y/width/height".to_string(),
                    ));
                };
                let action = match command {
                    WindowCommand::Move => "move",
                    WindowCommand::Resize => "resize",
                    _ => unreachable!(),
                };
                run_ax_window_action(app_reference, window, action, Some(bounds))
            }
        }
    }
}

fn virtual_key_code(key: &str) -> Option<u16> {
    match key.to_lowercase().as_str() {
        "a" => Some(0),
        "s" => Some(1),
        "d" => Some(2),
        "f" => Some(3),
        "h" => Some(4),
        "g" => Some(5),
        "z" => Some(6),
        "x" => Some(7),
        "c" => Some(8),
        "v" => Some(9),
        "b" => Some(11),
        "q" => Some(12),
        "w" => Some(13),
        "e" => Some(14),
        "r" => Some(15),
        "y" => Some(16),
        "t" => Some(17),
        "1" => Some(18),
        "2" => Some(19),
        "3" => Some(20),
        "4" => Some(21),
        "6" => Some(22),
        "5" => Some(23),
        "=" | "+" => Some(24),
        "9" => Some(25),
        "7" => Some(26),
        "-" | "_" => Some(27),
        "8" => Some(28),
        "0" => Some(29),
        "]" | "}" => Some(30),
        "o" => Some(31),
        "u" => Some(32),
        "[" | "{" => Some(33),
        "i" => Some(34),
        "p" => Some(35),
        "return" | "enter" => Some(36),
        "l" => Some(37),
        "j" => Some(38),
        "'" | "\"" => Some(39),
        "k" => Some(40),
        ";" | ":" => Some(41),
        "\\" | "|" => Some(42),
        "," | "<" => Some(43),
        "/" | "?" => Some(44),
        "n" => Some(45),
        "m" => Some(46),
        "." | ">" => Some(47),
        "tab" => Some(48),
        "space" => Some(49),
        "`" | "~" => Some(50),
        "delete" | "backspace" => Some(51),
        "escape" | "esc" => Some(53),
        "left" => Some(123),
        "right" => Some(124),
        "down" => Some(125),
        "up" => Some(126),
        _ => None,
    }
}

fn macos_modifier_flag_expression(modifiers: &[String]) -> SootieResult<String> {
    if modifiers.is_empty() {
        return Ok("0".to_string());
    }
    modifiers
        .iter()
        .map(|modifier| {
            let flag = match modifier.to_lowercase().as_str() {
                "cmd" | "command" | "meta" => "$.kCGEventFlagMaskCommand",
                "ctrl" | "control" => "$.kCGEventFlagMaskControl",
                "alt" | "option" => "$.kCGEventFlagMaskAlternate",
                "shift" => "$.kCGEventFlagMaskShift",
                other => {
                    return Err(SootieError::InvalidArguments(format!(
                        "unknown macOS modifier '{other}'"
                    )));
                }
            };
            Ok(flag)
        })
        .collect::<SootieResult<Vec<_>>>()
        .map(|tokens| tokens.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_browser_names_to_macos_application_names() {
        assert_eq!(browser_application_name("Safari"), Some("Safari"));
        assert_eq!(browser_application_name("chrome"), Some("Google Chrome"));
        assert_eq!(
            browser_application_name("Microsoft Edge"),
            Some("Microsoft Edge")
        );
        assert_eq!(browser_application_name("brave"), Some("Brave Browser"));
        assert_eq!(browser_application_name("Notes"), None);
    }

    #[test]
    fn maps_macos_modifier_aliases_to_coregraphics_flags() {
        let modifiers = vec![
            "cmd".to_string(),
            "ctrl".to_string(),
            "alt".to_string(),
            "shift".to_string(),
        ];
        assert_eq!(
            macos_modifier_flag_expression(&modifiers).unwrap(),
            "$.kCGEventFlagMaskCommand | $.kCGEventFlagMaskControl | $.kCGEventFlagMaskAlternate | $.kCGEventFlagMaskShift"
        );
    }

    #[test]
    fn rejects_unknown_macos_modifiers() {
        let error = macos_modifier_flag_expression(&["hyper".to_string()]).unwrap_err();
        assert!(error.to_string().contains("unknown macOS modifier"));
    }

    #[test]
    fn keyboard_event_script_uses_coregraphics_for_plain_characters() {
        let script = keyboard_event_script(virtual_key_code("a").unwrap(), &[]).unwrap();
        assert!(script.contains("CoreGraphics"));
        assert!(script.contains("CGEventCreateKeyboardEvent"));
        assert!(script.contains("const keyCode = 0;"));
        assert!(script.contains("const flags = 0;"));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn keyboard_event_script_uses_coregraphics_for_special_keys_with_modifiers() {
        let script =
            keyboard_event_script(virtual_key_code("enter").unwrap(), &["cmd".to_string()])
                .unwrap();
        assert!(script.contains("const keyCode = 36;"));
        assert!(script.contains("$.kCGEventFlagMaskCommand"));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn virtual_key_code_rejects_unknown_named_keys() {
        assert_eq!(virtual_key_code("not-a-key"), None);
    }

    #[test]
    fn browser_url_scripts_use_tab_url_accessors() {
        let safari = browser_url_script("Safari");
        assert!(safari.contains("URL of current tab of front window"));
        let chrome = browser_url_script("Google Chrome");
        assert!(chrome.contains("URL of active tab of front window"));
    }

    #[test]
    fn detects_bundle_id_like_app_references() {
        assert!(looks_like_bundle_id("com.apple.TextEdit"));
        assert!(looks_like_bundle_id("org.chromium.Chromium"));
        assert!(!looks_like_bundle_id("Google Chrome"));
        assert!(!looks_like_bundle_id("/Applications/TextEdit.app"));
    }

    #[test]
    fn parses_cdp_port_from_browser_process_commands() {
        let output = "\
/Applications/Google Chrome.app/Contents/MacOS/Google Chrome --profile-directory=Default --remote-debugging-port=9222
/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper --type=renderer
/Applications/Brave Browser.app/Contents/MacOS/Brave Browser --remote-debugging-port 9333
";
        assert_eq!(
            parse_cdp_port_from_process_commands(output, "Google Chrome"),
            Some(9222)
        );
        assert_eq!(
            parse_cdp_port_from_process_commands(output, "Brave Browser"),
            Some(9333)
        );
        assert_eq!(parse_cdp_port_from_process_commands(output, "Safari"), None);
    }

    #[test]
    fn app_snapshot_script_selects_frontmost_or_named_app() {
        let frontmost = app_snapshot_script(None);
        assert!(frontmost.contains("first process whose frontmost is true"));
        let named = app_snapshot_script(Some("Google Chrome"));
        assert!(named.contains("repeat with candidateProcess"));
        assert!(named.contains("contains \"Google Chrome\""));
        assert!(named.contains("candidateBundleId contains \"Google Chrome\""));
        assert!(named.contains("set winX to \"\""));
        assert!(named.contains("set appPid to \"\""));
        assert!(named.contains("set appPid to unix id of targetProcess as text"));
        assert!(named
            .contains("set end of winRows to {winName, winX, winY, winWidth, winHeight} as text"));
    }

    #[test]
    fn app_list_script_lists_apps_without_reading_windows() {
        let script = app_list_script();
        assert!(script.contains("repeat with candidateProcess in processes"));
        assert!(script.contains("if not (background only of candidateProcess)"));
        assert!(script.contains("set appPid to unix id of candidateProcess as text"));
        assert!(script.contains("set appBundleId to bundle identifier of candidateProcess as text"));
        assert!(script.contains(
            "set end of appRows to {appName, fronted, appPid, appBundleId, \"\"} as text"
        ));
        assert!(!script.contains("windows of p"));
    }

    #[test]
    fn window_title_snapshot_script_reads_window_names_only() {
        let script = window_title_snapshot_script("Clock");
        assert!(script.contains("exists process \"Clock\""));
        assert!(script.contains("tell process \"Clock\""));
        assert!(script.contains("set appPid to \"\""));
        assert!(script.contains("set appPid to unix id as text"));
        assert!(script.contains("set appBundleId to bundle identifier as text"));
        assert!(script.contains("set winX to \"\""));
        assert!(script
            .contains("set end of winRows to {winName, winX, winY, winWidth, winHeight} as text"));
    }

    #[test]
    fn application_window_snapshot_script_reads_scriptable_window_bounds() {
        let script = application_window_snapshot_script("Google Chrome");
        assert!(script.contains("exists process \"Google Chrome\""));
        assert!(script.contains("tell application \"Google Chrome\""));
        assert!(script.contains("set boundsValue to bounds of w"));
        assert!(script.contains("set winWidth to (rightValue - leftValue) as text"));
        assert!(script.contains("set winHeight to (bottomValue - topValue) as text"));
        assert!(script
            .contains("set end of winRows to {winName, winX, winY, winWidth, winHeight} as text"));
    }

    #[test]
    fn window_server_snapshot_script_reads_onscreen_window_bounds() {
        let script = window_server_snapshot_script("Codex");
        assert!(script.contains("CGWindowListCopyWindowInfo"));
        assert!(script.contains("kCGWindowOwnerName"));
        assert!(script.contains("kCGWindowLayer"));
        assert!(script.contains("kCGWindowBounds"));
        assert!(script.contains("const wanted = \"Codex\";"));
    }

    #[test]
    fn window_server_app_list_script_collects_visible_apps_without_system_events() {
        let script = window_server_app_list_script(None);
        assert!(script.contains("CGWindowListCopyWindowInfo"));
        assert!(script.contains("i === 0 ? 'true' : 'false'"));
        assert!(script.contains("appRows.join(rowSep)"));
        assert!(!script.contains("System Events"));

        let filtered = window_server_app_list_script(Some("Chrome"));
        assert!(filtered.contains("const wanted = \"Chrome\";"));
        assert!(filtered.contains("owner.toLowerCase().indexOf"));
    }

    #[test]
    fn parses_window_server_app_list_rows_as_front_to_back_context_candidates() {
        let output = concat!(
            "Notes\u{1f}true\u{1f}1234\u{1f}\u{1f}Todo\u{1c}10\u{1c}20\u{1c}300\u{1c}200",
            "\u{1e}",
            "Calendar\u{1f}false\u{1f}5678\u{1f}\u{1f}May\u{1c}30\u{1c}40\u{1c}500\u{1c}400",
        );
        let apps = parse_app_state_rows(output, None);

        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0].name, "Notes");
        assert_eq!(apps[0].pid, Some(1234));
        assert!(apps[0].is_frontmost);
        assert_eq!(apps[0].windows[0].title, "Todo");
        assert!(apps[0].windows[0].focused);
        assert_eq!(apps[1].name, "Calendar");
        assert!(!apps[1].is_frontmost);
        assert!(!apps[1].windows[0].focused);
    }

    #[test]
    fn mouse_move_script_uses_coregraphics_event() {
        let script = mouse_move_script(10.2, 20.7);
        assert!(script.contains("CoreGraphics"));
        assert!(script.contains("CGEventCreateMouseEvent"));
        assert!(script.contains("kCGEventMouseMoved"));
        assert!(script.contains("CGPointMake(10, 21)"));
        assert!(!script.contains("mouse move"));
    }

    #[test]
    fn mouse_click_script_uses_coregraphics_events() {
        let script = mouse_click_script(10.2, 20.7, "left", 2).expect("script");
        assert!(script.contains("CoreGraphics"));
        assert!(script.contains("CGEventCreateMouseEvent"));
        assert!(script.contains("kCGEventLeftMouseDown"));
        assert!(script.contains("kCGEventLeftMouseUp"));
        assert!(script.contains("kCGMouseButtonLeft"));
        assert!(script.contains("CGPointMake(10, 21)"));
        assert!(script.contains("i < 2"));
    }

    #[test]
    fn mouse_click_script_rejects_unknown_buttons() {
        let error = mouse_click_script(10.0, 20.0, "primary", 1).expect_err("invalid button");
        assert!(error.to_string().contains("unsupported mouse button"));
    }

    #[test]
    fn mouse_button_event_script_uses_coregraphics_down_and_up() {
        let down = mouse_button_event_script(10.2, 20.7, "left", true).expect("down");
        assert!(down.contains("CoreGraphics"));
        assert!(down.contains("kCGEventLeftMouseDown"));
        assert!(down.contains("kCGMouseButtonLeft"));
        assert!(down.contains("CGPointMake(10, 21)"));

        let up = mouse_button_event_script(10.2, 20.7, "left", false).expect("up");
        assert!(up.contains("kCGEventLeftMouseUp"));
    }

    #[test]
    fn mouse_scroll_script_uses_coregraphics_scroll_event() {
        let script = mouse_scroll_script("down", 3).expect("script");
        assert!(script.contains("CoreGraphics"));
        assert!(script.contains("CGEventCreateScrollWheelEvent"));
        assert!(script.contains("kCGScrollEventUnitLine"));
        assert!(script.contains(", -3, 0"));
        assert!(!script.contains("scroll wheel"));
    }

    #[test]
    fn mouse_scroll_script_rejects_unknown_directions() {
        let error = mouse_scroll_script("forward", 1).expect_err("invalid direction");
        assert!(error.to_string().contains("unsupported scroll direction"));
    }

    #[test]
    fn mouse_drag_script_uses_coregraphics_drag_events() {
        let script = mouse_drag_script(10.2, 20.7, 30.1, 40.9);
        assert!(script.contains("CoreGraphics"));
        assert!(script.contains("kCGEventLeftMouseDown"));
        assert!(script.contains("kCGEventLeftMouseDragged"));
        assert!(script.contains("kCGEventLeftMouseUp"));
        assert!(script.contains("fromX = 10"));
        assert!(script.contains("toY = 41"));
        assert!(!script.contains("drag from"));
    }

    #[test]
    fn fill_app_windows_from_output_merges_fallback_bounds() {
        let mut app = minimal_app_info("Codex".to_string(), true);
        let output = "Codex\u{1f}true\u{1f}42\u{1f}com.openai.codex\u{1f}Codex\u{1c}0\u{1c}32\u{1c}1728\u{1c}1085";

        assert!(fill_app_windows_from_output(&mut app, output, "Codex"));
        assert_eq!(app.pid, Some(42));
        assert_eq!(app.bundle_id.as_deref(), Some("com.openai.codex"));
        assert_eq!(app.windows.len(), 1);
        assert_eq!(app.windows[0].title, "Codex");
        assert_eq!(
            app.windows[0].bounds,
            Some(Bounds {
                x: 0.0,
                y: 32.0,
                width: 1728.0,
                height: 1085.0,
            })
        );
    }

    #[test]
    fn app_focus_script_sets_process_frontmost() {
        let script = app_focus_script("TextEdit");
        assert!(script.contains("exists process \"TextEdit\""));
        assert!(script.contains("set frontmost of process \"TextEdit\" to true"));
        assert!(script.contains("AXFrontmost"));
    }

    #[test]
    fn app_activate_script_uses_appkit_without_system_events() {
        let script = app_activate_script("TextEdit");
        assert!(script.contains("AppKit"));
        assert!(script.contains("NSWorkspace.shared.runningApplications"));
        assert!(script.contains("app.activate()"));
        assert!(script.contains("let wanted = \"TextEdit\""));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn ax_window_action_script_uses_accessibility_without_system_events() {
        let script = ax_window_action_script(
            "TextEdit",
            Some("Untitled"),
            "move",
            Some(Bounds {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
            }),
        );
        assert!(script.contains("ApplicationServices"));
        assert!(script.contains("AXUIElementCreateApplication"));
        assert!(script.contains("kAXWindowsAttribute"));
        assert!(script.contains("CGRect(x: 10"));
        assert!(script.contains("let action = \"move\""));
        assert!(script.contains("let wantedWindow: String? = \"Untitled\""));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn extracts_app_name_from_process_command() {
        assert_eq!(
            app_name_from_process_command(
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
            )
            .as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(
            app_name_from_process_command(
                "/Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper",
            )
            .as_deref(),
            Some("Google Chrome")
        );
        assert_eq!(app_name_from_process_command("/usr/libexec/sandboxd"), None);
    }

    #[test]
    fn parses_ps_app_rows_as_state_fallback() {
        let output = "\
2286 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
2305 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper
3916 /System/Applications/Calendar.app/Contents/MacOS/Calendar
92408 ./Codex Computer Use.app/Contents/SharedSupport/SkyComputerUseClient.app/Contents/MacOS/SkyComputerUseClient
92406 node
";
        let apps = parse_ps_app_rows(output, None, Some("Google Chrome"));
        assert_eq!(apps.len(), 3);
        assert_eq!(apps[0].name, "Google Chrome");
        assert_eq!(apps[0].pid, Some(2286));
        assert!(apps[0].is_frontmost);
        assert_eq!(apps[1].name, "Calendar");
        assert!(!apps[1].is_frontmost);

        let codex_apps = parse_ps_app_rows(output, Some("codex"), Some("Codex"));
        assert_eq!(codex_apps.len(), 1);
        assert_eq!(codex_apps[0].name, "Codex Computer Use");
        assert!(!codex_apps[0].is_frontmost);

        let filtered = parse_ps_app_rows(output, Some("calendar"), None);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "Calendar");
    }

    #[test]
    fn parses_first_browser_cdp_port_without_app_state() {
        let output = "\
100 /System/Applications/Calendar.app/Contents/MacOS/Calendar
2286 /Applications/Google Chrome.app/Contents/MacOS/Google Chrome --remote-debugging-port=9222
2305 /Applications/Google Chrome.app/Contents/Frameworks/Google Chrome Framework.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/Google Chrome Helper --remote-debugging-port=9333
3916 /Applications/Brave Browser.app/Contents/MacOS/Brave Browser --remote-debugging-port 9444
";
        assert_eq!(parse_first_browser_cdp_port(output), Some(9222));
        assert_eq!(
            parse_first_browser_cdp_port(
                "/System/Applications/Calendar.app/Contents/MacOS/Calendar"
            ),
            None
        );
    }

    #[test]
    fn parses_app_state_rows_with_window_bounds() {
        let output = "Notes\u{1f}true\u{1f}1234\u{1f}com.apple.Notes\u{1f}Todo\u{1c}10\u{1c}20\u{1c}300\u{1c}200";
        let apps = parse_app_state_rows(output, None);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Notes");
        assert_eq!(apps[0].pid, Some(1234));
        assert_eq!(apps[0].platform_app_id.as_deref(), Some("com.apple.Notes"));
        assert_eq!(apps[0].bundle_id.as_deref(), Some("com.apple.Notes"));
        assert!(apps[0].is_frontmost);
        assert_eq!(apps[0].windows[0].title, "Todo");
        assert!(apps[0].windows[0].focused);
        assert_eq!(apps[0].windows[0].bounds.as_ref().unwrap().width, 300.0);
    }

    #[test]
    fn app_state_filters_accept_bundle_identity() {
        let output = "Codex\u{1f}true\u{1f}39521\u{1f}com.openai.codex\u{1f}Main\u{1c}10\u{1c}20\u{1c}300\u{1c}200";
        let apps = parse_app_state_rows(output, Some("com.openai.codex"));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Codex");
        assert_eq!(apps[0].bundle_id.as_deref(), Some("com.openai.codex"));
        assert!(parse_app_state_rows(output, Some("com.apple.Safari")).is_empty());
    }

    #[test]
    fn gives_full_app_list_a_dedicated_timeout() {
        let full_list_timeout = app_state_timeout_ms(None);

        assert_eq!(full_list_timeout, APP_LIST_SCRIPT_TIMEOUT_MS);
        assert_eq!(
            app_state_timeout_ms(Some("Codex")),
            APP_STATE_SCRIPT_TIMEOUT_MS
        );
        assert!(full_list_timeout > APP_SNAPSHOT_SCRIPT_TIMEOUT_MS);
    }

    #[test]
    fn parses_legacy_app_state_rows_without_pid() {
        let output = "Notes\u{1f}true\u{1f}Todo\u{1c}10\u{1c}20\u{1c}300\u{1c}200";
        let apps = parse_app_state_rows(output, None);
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Notes");
        assert_eq!(apps[0].pid, None);
        assert_eq!(apps[0].platform_app_id, None);
        assert_eq!(apps[0].bundle_id, None);
        assert_eq!(apps[0].windows[0].title, "Todo");
    }

    #[test]
    fn parses_frontmost_app_info_with_identity_fields() {
        let app = parse_frontmost_app_info("Codex\u{1f}39521\u{1f}com.openai.codex", None).unwrap();
        assert_eq!(app.name, "Codex");
        assert_eq!(app.pid, Some(39521));
        assert_eq!(app.platform_app_id.as_deref(), Some("com.openai.codex"));
        assert_eq!(app.bundle_id.as_deref(), Some("com.openai.codex"));
        assert!(app.is_frontmost);
        assert!(
            parse_frontmost_app_info("Codex\u{1f}39521\u{1f}com.openai.codex", Some("Safari"))
                .is_none()
        );
        assert!(parse_frontmost_app_info(
            "Codex\u{1f}39521\u{1f}com.openai.codex",
            Some("com.openai.codex")
        )
        .is_some());
    }

    #[test]
    fn frontmost_name_probe_uses_minimal_system_events_query() {
        let script = frontmost_app_name_script();
        assert!(script.contains("name of first process whose frontmost is true"));
        assert!(!script.contains("unix id"));
        assert!(!script.contains("bundle identifier"));
    }

    #[test]
    fn accessibility_diagnostic_reports_launch_path_denial() {
        let diagnostic = macos_accessibility_diagnostic_from_result(Err(SootieError::Platform(
            "swift failed: AX window probe failed: -25211".to_string(),
        )));
        assert_eq!(diagnostic.name, "macos_accessibility");
        assert!(!diagnostic.success);
        assert!(diagnostic.message.contains("launch path"));
        assert!(diagnostic
            .details
            .unwrap()
            .get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("-25211"));
    }

    #[test]
    fn window_server_diagnostic_reports_visible_window_probe_results() {
        let success = macos_window_server_diagnostic_from_result(Ok(vec![minimal_app_info(
            "Notes".to_string(),
            true,
        )]));
        assert_eq!(success.name, "macos_window_server");
        assert!(success.success);
        assert!(success.message.contains("succeeded"));

        let empty = macos_window_server_diagnostic_from_result(Ok(Vec::new()));
        assert!(!empty.success);
        assert!(empty.message.contains("found no windows"));
        assert!(empty
            .details
            .unwrap()
            .get("recovery")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("active Aqua desktop"));

        let failed = macos_window_server_diagnostic_from_result(Err(SootieError::Platform(
            "osascript failed".to_string(),
        )));
        assert!(!failed.success);
        assert!(failed.message.contains("failed"));
        assert!(failed
            .details
            .unwrap()
            .get("error")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("osascript failed"));
    }

    #[test]
    fn builds_minimal_frontmost_app_info_when_deep_state_is_unavailable() {
        let app = app_info_from_frontmost_name("cmux", Some("cm")).unwrap();
        assert_eq!(app.name, "cmux");
        assert!(app.is_frontmost);
        assert!(app.windows.is_empty());
        assert!(app_info_from_frontmost_name("cmux", Some("Safari")).is_none());

        let fallback = fallback_app_info(Some("SootieMissingAppForFallback"));
        assert_eq!(fallback.name, "SootieMissingAppForFallback");
        assert!(!fallback.is_frontmost);
        assert!(fallback.windows.is_empty());

        let fast_fallback =
            fallback_app_info_without_frontmost_retry(Some("SootieMissingAppForFallback"));
        assert_eq!(fast_fallback.name, "SootieMissingAppForFallback");
        assert!(!fast_fallback.is_frontmost);
        assert!(fast_fallback.windows.is_empty());
    }

    #[test]
    fn filtered_state_fallback_does_not_synthesize_missing_apps() {
        let missing = "SootieMissingAppForFilteredState";
        assert!(fallback_state_app_list(Some(missing)).is_empty());
        assert!(fallback_state_app_list_without_frontmost_retry(Some(missing)).is_empty());
        assert!(!fallback_state_app_list(None).is_empty());
        assert!(!fallback_state_app_list_without_frontmost_retry(None).is_empty());
    }

    #[test]
    fn app_name_match_is_case_insensitive_substring() {
        assert!(app_name_matches("Google Chrome", "chrome"));
        assert!(app_name_matches("Cursor", "CUR"));
        assert!(!app_name_matches("Clock", "Finder"));
        assert!(app_identity_matches(
            "Codex",
            Some("com.openai.codex"),
            "openai"
        ));
    }

    #[test]
    fn detects_display_unavailable_screenshot_errors() {
        assert!(screen_capture_display_unavailable(
            "screencapture failed: could not create image from display"
        ));
        assert!(screen_capture_display_unavailable(
            "rect (0.0, 0.0, 100.0, 100.0) does not intersect any displays"
        ));
        assert!(!screen_capture_display_unavailable("permission denied"));
    }

    #[test]
    fn window_action_result_keeps_action_details() {
        let result = macos_window_result(
            "resize",
            "Notes",
            Some("Todo"),
            Some(Bounds {
                x: 10.0,
                y: 20.0,
                width: 300.0,
                height: 200.0,
            }),
        );
        assert_eq!(result.details["app"], "Notes");
        assert_eq!(result.details["action"], "resize");
        assert_eq!(result.details["window"], "Todo");
        assert_eq!(result.details["bounds"]["width"], 300.0);
    }

    #[test]
    fn ax_accessibility_script_collects_window_contents_without_system_events() {
        let script = ax_accessibility_elements_script("Finder", 25);
        assert!(script.contains("AXUIElementCopyAttributeValue"));
        assert!(script.contains("AXIdentifier"));
        assert!(script.contains("rows.count < maxElements"));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn ax_window_probe_script_uses_accessibility_without_system_events() {
        let script = ax_window_probe_script("Finder");
        assert!(script.contains("kAXWindowsAttribute"));
        assert!(script.contains("AXUIElementCreateApplication"));
        assert!(!script.contains("System Events"));
    }

    #[test]
    fn parses_accessibility_element_rows() {
        let row =
            "AXButton\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}true\u{1f}false";
        let record = parse_accessibility_element_row(row).unwrap();
        assert!(!record.focused);
        assert_eq!(record.element.role, "AXButton");
        assert_eq!(record.element.name.as_deref(), Some("Submit"));
        assert_eq!(record.element.id.as_deref(), Some("submit-button"));
        assert_eq!(record.element.bounds.unwrap().width, 100.0);
        assert_eq!(record.element.enabled, Some(true));
        assert_eq!(record.element.editable, Some(false));
        assert_eq!(record.element.actions, vec!["click"]);
    }

    #[test]
    fn parses_focused_editable_accessibility_rows() {
        let row =
            "AXTextField\u{1f}Search\u{1f}query\u{1f}search-input\u{1f}4\u{1f}5\u{1f}200\u{1f}24\u{1f}true\u{1f}true";
        let record = parse_accessibility_element_row(row).unwrap();
        assert!(record.focused);
        assert_eq!(record.element.text.as_deref(), Some("query"));
        assert_eq!(record.element.editable, Some(true));
        assert_eq!(record.element.actions, vec!["setValue"]);
    }

    #[test]
    fn accessibility_query_matches_name_role_and_identifier() {
        let record = parse_accessibility_element_row(
            "AXButton\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}true\u{1f}false",
        )
        .unwrap();
        assert!(element_matches_query(
            &record.element,
            &FindQuery {
                query: Some("sub".to_string()),
                role: Some("button".to_string()),
                identifier: Some("submit-button".to_string()),
                ..Default::default()
            }
        ));
        assert!(!element_matches_query(
            &record.element,
            &FindQuery {
                query: Some("cancel".to_string()),
                ..Default::default()
            }
        ));
    }
}
