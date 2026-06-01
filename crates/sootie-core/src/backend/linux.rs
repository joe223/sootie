use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine;
use serde_json::json;

use crate::backend::{
    cdp, command_output, command_output_stdin_timeout, command_output_timeout,
    element_at_from_elements, element_from_window, filter_elements, has_element_target,
    png_dimensions, run_command, tmp_screenshot_path, DesktopBackend,
};
use crate::types::{
    ActionResult, AppInfo, Bounds, ContextSnapshot, ElementInfo, FindQuery, RuntimeDiagnostic,
    Screenshot, SootieError, SootieResult, WindowCommand, WindowInfo,
};

pub struct LinuxBackend;

const ACCESSIBILITY_SCRIPT_TIMEOUT_MS: u64 = 1_500;
const SCREENSHOT_TIMEOUT_MS: u64 = 5_000;

#[derive(Debug, Clone)]
struct AccessibilityElementRecord {
    element: ElementInfo,
    focused: bool,
}

fn probe_command(program: &str, args: &[&str]) -> Result<(), String> {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("{program} exited with {status}")),
        Err(error) => Err(format!("{program}: {error}")),
    }
}

fn linux_diagnostic_from_probe(
    name: &str,
    result: Result<(), String>,
    success_message: &str,
    failure_message: &str,
    recovery: &str,
) -> RuntimeDiagnostic {
    match result {
        Ok(()) => RuntimeDiagnostic {
            name: name.to_string(),
            success: true,
            message: success_message.to_string(),
            details: None,
        },
        Err(error) => RuntimeDiagnostic {
            name: name.to_string(),
            success: false,
            message: failure_message.to_string(),
            details: Some(json!({
                "error": error,
                "recovery": recovery,
            })),
        },
    }
}

fn linux_xprop_diagnostic() -> RuntimeDiagnostic {
    linux_diagnostic_from_probe(
        "linux_xprop",
        probe_command("xprop", &["-root", "_NET_ACTIVE_WINDOW"]),
        "Linux xprop/X11 probe succeeded",
        "Linux xprop/X11 probe failed for the Sootie launch path",
        "Install xprop and run Sootie from an interactive X11 desktop session.",
    )
}

fn linux_wmctrl_diagnostic() -> RuntimeDiagnostic {
    linux_diagnostic_from_probe(
        "linux_wmctrl",
        probe_command("wmctrl", &["-m"]),
        "Linux wmctrl probe succeeded",
        "Linux wmctrl probe failed for the Sootie launch path",
        "Install wmctrl and run Sootie from an interactive X11 desktop session.",
    )
}

fn linux_xdotool_diagnostic() -> RuntimeDiagnostic {
    linux_diagnostic_from_probe(
        "linux_xdotool",
        probe_command("xdotool", &["getactivewindow"]),
        "Linux xdotool/X11 probe succeeded",
        "Linux xdotool/X11 probe failed for the Sootie launch path",
        "Install xdotool and run Sootie from an interactive X11 desktop session with an active window.",
    )
}

fn linux_pyatspi_diagnostic() -> RuntimeDiagnostic {
    linux_diagnostic_from_probe(
        "linux_pyatspi",
        probe_command("python3", &["-c", "import pyatspi"]),
        "Linux pyatspi probe succeeded",
        "Linux pyatspi probe failed for the Sootie launch path",
        "Install Python 3 AT-SPI bindings, such as python3-pyatspi or python3-gi plus gir1.2-atspi-2.0.",
    )
}

fn screenshot_probe_result() -> Result<(), String> {
    let candidates: [(&str, &[&str]); 3] = [
        ("gnome-screenshot", &["--version"]),
        ("import", &["-version"]),
        ("scrot", &["--version"]),
    ];
    let mut errors = Vec::new();
    for (program, args) in candidates {
        match probe_command(program, args) {
            Ok(()) => return Ok(()),
            Err(error) => errors.push(error),
        }
    }
    Err(errors.join("; "))
}

fn linux_screenshot_diagnostic() -> RuntimeDiagnostic {
    linux_diagnostic_from_probe(
        "linux_screenshot",
        screenshot_probe_result(),
        "Linux screenshot utility probe succeeded",
        "No Linux screenshot utility was available for the Sootie launch path",
        "Install gnome-screenshot, ImageMagick import, or scrot.",
    )
}

fn normalize_x11_window_id(value: &str) -> Option<String> {
    let raw = value
        .trim()
        .trim_end_matches(',')
        .split_whitespace()
        .last()
        .unwrap_or(value)
        .trim();
    let raw = raw.strip_prefix("0x").unwrap_or(raw);
    u64::from_str_radix(raw, 16)
        .or_else(|_| raw.parse::<u64>())
        .ok()
        .map(|id| format!("0x{id:x}"))
}

fn parse_active_window_id(output: &str) -> Option<String> {
    output.split('#').nth(1).and_then(normalize_x11_window_id)
}

fn active_window_id() -> Option<String> {
    command_output("xprop", &["-root", "_NET_ACTIVE_WINDOW"])
        .ok()
        .and_then(|output| parse_active_window_id(&output))
}

fn browser_process_name(app_name: &str) -> Option<&'static str> {
    let normalized = app_name.to_lowercase();
    match normalized.as_str() {
        "chrome" | "google-chrome" | "google-chrome-stable" | "google chrome" => {
            Some("google-chrome")
        }
        "chromium" | "chromium-browser" => Some("chromium"),
        "brave" | "brave-browser" | "brave browser" => Some("brave-browser"),
        "edge" | "microsoft-edge" | "microsoft-edge-stable" | "microsoft edge" | "msedge" => {
            Some("microsoft-edge")
        }
        "firefox" | "mozilla firefox" => Some("firefox"),
        _ => None,
    }
}

fn browser_process_name_from_cmdline(cmdline: &str) -> Option<&'static str> {
    let executable = cmdline
        .split_whitespace()
        .next()
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or(cmdline);
    browser_process_name(executable)
}

fn read_process_cmdline(pid: u32) -> Option<String> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let cmdline = String::from_utf8_lossy(&bytes).replace('\0', " ");
    Some(cmdline)
}

fn current_browser_url(app: &AppInfo) -> Option<String> {
    let _process_name = browser_process_name(&app.name)?;
    browser_cdp_port(app).and_then(cdp::current_page_url)
}

fn browser_cdp_port(app: &AppInfo) -> Option<u16> {
    let pid = app.pid?;
    let cmdline = read_process_cmdline(pid)?;
    cdp::parse_remote_debugging_port(&cmdline)
}

fn first_browser_cdp_port() -> Option<u16> {
    let entries = std::fs::read_dir("/proc").ok()?;
    let cmdlines = entries.flatten().filter_map(|entry| {
        let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
        read_process_cmdline(pid)
    });
    parse_first_browser_cdp_port_from_cmdlines(cmdlines)
}

fn parse_first_browser_cdp_port_from_cmdlines<I, S>(cmdlines: I) -> Option<u16>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    cmdlines.into_iter().find_map(|cmdline| {
        let cmdline = cmdline.as_ref();
        browser_process_name_from_cmdline(cmdline)?;
        cdp::parse_remote_debugging_port(cmdline)
    })
}

fn accessibility_elements_script() -> &'static str {
    r#"
import sys

app_name = sys.argv[1].lower()
pid_arg = sys.argv[2]
limit = int(sys.argv[3])
field_sep = chr(31)
row_sep = chr(30)

try:
    import pyatspi
except Exception:
    sys.exit(0)

def app_pid(accessible):
    for attr in ("get_process_id", "getProcessID"):
        method = getattr(accessible, attr, None)
        if method:
            try:
                return int(method())
            except Exception:
                pass
    try:
        app = accessible.getApplication()
        method = getattr(app, "get_process_id", None) or getattr(app, "getProcessID", None)
        if method:
            return int(method())
    except Exception:
        pass
    return None

def target_app(desktop):
    wanted_pid = int(pid_arg) if pid_arg.isdigit() else None
    fallback = None
    for index in range(desktop.childCount):
        try:
            app = desktop.getChildAtIndex(index)
        except Exception:
            continue
        name = (getattr(app, "name", "") or "").lower()
        pid = app_pid(app)
        if wanted_pid is not None and pid == wanted_pid:
            return app
        if app_name and (app_name in name or name in app_name):
            fallback = fallback or app
    return fallback

def text_value(accessible):
    try:
        text = accessible.queryText()
        value = text.getText(0, -1)
        return value or ""
    except Exception:
        return ""

def attributes(accessible):
    result = {}
    try:
        for item in accessible.getAttributes():
            key, _, value = item.partition(":")
            result[key] = value
    except Exception:
        pass
    return result

def extents(accessible):
    try:
        component = accessible.queryComponent()
        box = component.getExtents(pyatspi.DESKTOP_COORDS)
        return (box.x, box.y, box.width, box.height)
    except Exception:
        return ("", "", "", "")

def state_flags(accessible):
    try:
        state = accessible.getState()
        enabled = state.contains(pyatspi.STATE_ENABLED)
        focused = state.contains(pyatspi.STATE_FOCUSED)
        return (enabled, focused)
    except Exception:
        return ("", "")

rows = []

def walk(accessible, depth=0):
    if len(rows) >= limit or depth > 8:
        return
    try:
        role = accessible.getRoleName() or ""
    except Exception:
        role = ""
    name = getattr(accessible, "name", "") or ""
    value = text_value(accessible)
    attrs = attributes(accessible)
    identifier = attrs.get("id") or attrs.get("accessible-name") or ""
    x, y, width, height = extents(accessible)
    enabled, focused = state_flags(accessible)
    if role and (name or value or identifier or (width not in ("", 0) and height not in ("", 0))):
        rows.append(field_sep.join(str(item) for item in [
            role, name, value, identifier, x, y, width, height, enabled, focused
        ]))
    try:
        count = accessible.childCount
    except Exception:
        count = 0
    for index in range(count):
        if len(rows) >= limit:
            break
        try:
            child = accessible.getChildAtIndex(index)
        except Exception:
            continue
        walk(child, depth + 1)

try:
    desktop = pyatspi.Registry.getDesktop(0)
    app = target_app(desktop)
    if app is not None:
        walk(app)
except Exception:
    pass

sys.stdout.write(row_sep.join(rows))
"#
}

fn python_script_args<'a>(
    script: &'a str,
    app_name: &'a str,
    pid: &'a str,
    limit: &'a str,
) -> [&'a str; 5] {
    ["-c", script, app_name, pid, limit]
}

fn accessibility_elements(app: &AppInfo) -> Vec<AccessibilityElementRecord> {
    let pid = app.pid.map(|pid| pid.to_string()).unwrap_or_default();
    let max = "400".to_string();
    let script = accessibility_elements_script();
    let args = python_script_args(script, &app.name, &pid, &max);
    command_output_timeout(
        "python3",
        &args,
        Duration::from_millis(ACCESSIBILITY_SCRIPT_TIMEOUT_MS),
    )
    .ok()
    .map(|output| {
        output
            .split('\u{1e}')
            .filter_map(parse_accessibility_element_row)
            .collect()
    })
    .unwrap_or_default()
}

fn window_target<'a>(app: &'a str, window: Option<&'a str>) -> &'a str {
    window
        .map(str::trim)
        .filter(|window| !window.is_empty())
        .unwrap_or(app)
}

fn wmctrl_focus_args(
    app: &str,
    platform_app_id: Option<&str>,
    window: Option<&str>,
) -> Vec<String> {
    let has_window = window
        .map(str::trim)
        .is_some_and(|window| !window.is_empty());
    let platform_target = platform_app_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let target = if has_window {
        window_target(app, window)
    } else {
        platform_target.unwrap_or(app)
    };
    if platform_target.is_some() && !has_window {
        vec!["-x".to_string(), "-a".to_string(), target.to_string()]
    } else {
        vec!["-a".to_string(), target.to_string()]
    }
}

fn wmctrl_window_args(
    app: &str,
    platform_app_id: Option<&str>,
    window: Option<&str>,
) -> Vec<String> {
    let has_window = window
        .map(str::trim)
        .is_some_and(|window| !window.is_empty());
    let platform_target = platform_app_id
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let target = if has_window {
        window_target(app, window)
    } else {
        platform_target.unwrap_or(app)
    };
    if platform_target.is_some() && !has_window {
        vec!["-x".to_string(), "-r".to_string(), target.to_string()]
    } else {
        vec!["-r".to_string(), target.to_string()]
    }
}

fn parse_accessibility_element_row(row: &str) -> Option<AccessibilityElementRecord> {
    let fields = row.split('\u{1f}').collect::<Vec<_>>();
    if fields.len() < 10 {
        return None;
    }
    let role = non_empty_string(fields[0])?;
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
    let editable = editable_role(&role);
    let title = name.clone().or_else(|| text.clone());
    Some(AccessibilityElementRecord {
        focused,
        element: ElementInfo {
            id,
            role: role.clone(),
            title,
            name,
            text,
            bounds,
            actions: actions_for_accessibility_role(&role, editable),
            editable: Some(editable),
            enabled,
        },
    })
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value != "None").then(|| value.to_string())
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

fn canonical_role(role: &str) -> String {
    role.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn editable_role(role: &str) -> bool {
    let role = canonical_role(role);
    role.contains("text")
        || role.contains("entry")
        || role.contains("edit")
        || role.contains("combo")
        || role.contains("document")
}

fn actions_for_accessibility_role(role: &str, editable: bool) -> Vec<String> {
    let role = canonical_role(role);
    let mut actions = Vec::new();
    if editable {
        actions.push("setValue".to_string());
    }
    if role.contains("button")
        || role.contains("check")
        || role.contains("radio")
        || role.contains("menuitem")
        || role.contains("link")
        || role.contains("listitem")
    {
        actions.push("click".to_string());
    }
    actions
}

fn app_filter_matches(app: &AppInfo, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    app.name.to_lowercase().contains(&filter)
        || app
            .app_id
            .as_deref()
            .is_some_and(|id| id.to_lowercase().contains(&filter))
        || app
            .platform_app_id
            .as_deref()
            .is_some_and(|id| id.to_lowercase().contains(&filter))
        || app
            .bundle_id
            .as_deref()
            .is_some_and(|id| id.to_lowercase().contains(&filter))
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
        .map(|wanted| canonical_role(&element.role) == canonical_role(wanted))
        .unwrap_or(true);
    let matches_id = query
        .identifier
        .as_ref()
        .or(query.dom_id.as_ref())
        .map(|wanted| element.id.as_ref().is_some_and(|id| id == wanted))
        .unwrap_or(true);
    matches_query && matches_role && matches_id
}

impl LinuxBackend {
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
        browser_cdp_port(&app)
    }

    fn screenshot_window(&self, app: Option<&str>, window: Option<&str>) -> Option<WindowInfo> {
        let app = app?;
        let windows = self
            .state(Some(app))
            .ok()?
            .into_iter()
            .flat_map(|app| app.windows.into_iter())
            .collect::<Vec<_>>();
        if let Some(needle) = window {
            return windows
                .into_iter()
                .find(|candidate| candidate.title.contains(needle));
        }
        windows
            .iter()
            .find(|window| window.focused)
            .cloned()
            .or_else(|| windows.into_iter().next())
    }

    fn capture_screenshot(&self, program: &str, args: &[&str]) -> SootieResult<()> {
        command_output_timeout(program, args, Duration::from_millis(SCREENSHOT_TIMEOUT_MS))
            .map(|_| ())
    }

    fn wmctrl_state(&self) -> SootieResult<Vec<AppInfo>> {
        let active_window_id = active_window_id();
        let output = command_output("wmctrl", &["-lpGx"])?;
        let mut apps = Vec::<AppInfo>::new();
        for line in output.lines() {
            let parts = line.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 9 {
                continue;
            }
            let id = parts[0].to_string();
            let pid = parts[2].parse::<u32>().ok();
            let x = parts[3].parse::<f64>().ok();
            let y = parts[4].parse::<f64>().ok();
            let width = parts[5].parse::<f64>().ok();
            let height = parts[6].parse::<f64>().ok();
            let class = parts[7]
                .split('.')
                .next_back()
                .unwrap_or(parts[7])
                .to_string();
            let focused = active_window_id
                .as_deref()
                .is_some_and(|active| normalize_x11_window_id(&id).as_deref() == Some(active));
            let title = parts[8..].join(" ");
            let bounds = match (x, y, width, height) {
                (Some(x), Some(y), Some(width), Some(height)) => Some(Bounds {
                    x,
                    y,
                    width,
                    height,
                }),
                _ => None,
            };
            if let Some(app) = apps.iter_mut().find(|app| app.name == class) {
                app.is_frontmost |= focused;
                app.windows.push(WindowInfo {
                    id: Some(id),
                    title,
                    bounds,
                    focused,
                });
            } else {
                apps.push(AppInfo {
                    name: class,
                    app_id: Some(parts[7].to_string()),
                    platform_app_id: Some(parts[7].to_string()),
                    pid,
                    bundle_id: None,
                    is_frontmost: focused,
                    windows: vec![WindowInfo {
                        id: Some(id),
                        title,
                        bounds,
                        focused,
                    }],
                });
            }
        }
        Ok(apps)
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

    fn focus_query_app(&self, query: &FindQuery) -> SootieResult<()> {
        if let Some(app) = query.app.as_deref() {
            self.focus(app, None, None)?;
        }
        Ok(())
    }
}

impl DesktopBackend for LinuxBackend {
    fn platform(&self) -> &'static str {
        "linux"
    }

    fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
        vec![
            linux_xprop_diagnostic(),
            linux_wmctrl_diagnostic(),
            linux_xdotool_diagnostic(),
            linux_pyatspi_diagnostic(),
            linux_screenshot_diagnostic(),
        ]
    }

    fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot> {
        let apps = self.state(app)?;
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
        let accessibility_records = selected_app.map(accessibility_elements).unwrap_or_default();
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
            url: selected_app.and_then(current_browser_url).or_else(|| {
                self.cdp_port_for_app_filter(app)
                    .and_then(cdp::current_page_url)
            }),
            focused_element,
            interactive_elements,
        })
    }

    fn browser_url(&self, app: Option<&str>) -> SootieResult<Option<String>> {
        let apps = self.state(app)?;
        let selected_app = apps
            .iter()
            .find(|app| app.is_frontmost)
            .or_else(|| apps.first());
        Ok(selected_app.and_then(current_browser_url).or_else(|| {
            if app.is_none() {
                first_browser_cdp_port().and_then(cdp::current_page_url)
            } else {
                self.cdp_port_for_app_filter(app)
                    .and_then(cdp::current_page_url)
            }
        }))
    }

    fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
        let mut apps = self.wmctrl_state()?;
        if let Some(needle) = app {
            apps.retain(|app| app_filter_matches(app, needle));
        }
        Ok(apps)
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
        let apps = self.state(query.app.as_deref())?;
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
                accessibility_elements(app)
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
            self.find(&FindQuery::default())?,
            x,
            y,
        ))
    }

    fn screenshot(
        &self,
        app: Option<&str>,
        window: Option<&str>,
        _full_resolution: bool,
    ) -> SootieResult<Screenshot> {
        if window.is_none() {
            if let Some(screenshot) = self
                .cdp_port_for_app_filter(app)
                .and_then(cdp::page_screenshot)
            {
                return Ok(screenshot);
            }
        }
        let window = self.screenshot_window(app, window);
        let path = tmp_screenshot_path("png");
        let path_str = path.to_string_lossy().to_string();
        let captured =
            if let Some(window_id) = window.as_ref().and_then(|window| window.id.as_ref()) {
                self.capture_screenshot("import", &["-window", window_id, &path_str])
            } else if let Some(bounds) = window.as_ref().and_then(|window| window.bounds.as_ref()) {
                let crop = format!(
                    "{}x{}+{}+{}",
                    bounds.width.round().max(1.0) as i64,
                    bounds.height.round().max(1.0) as i64,
                    bounds.x.round() as i64,
                    bounds.y.round() as i64
                );
                self.capture_screenshot("import", &["-window", "root", "-crop", &crop, &path_str])
            } else {
                self.capture_screenshot("gnome-screenshot", &["-f", &path_str])
                    .or_else(|_| self.capture_screenshot("import", &["-window", "root", &path_str]))
            };
        captured?;
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
        self.focus_query_app(query)?;
        let (x, y) = self.resolve_point(x, y, query)?;
        let button_arg = match button {
            "right" => "3",
            "middle" => "2",
            _ => "1",
        };
        for _ in 0..count.max(1) {
            run_command(
                "xdotool",
                &[
                    "mousemove",
                    &x.to_string(),
                    &y.to_string(),
                    "click",
                    button_arg,
                ],
            )?;
        }
        Ok(ActionResult {
            method: "xdotool".to_string(),
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
        self.focus_query_app(query)?;
        let (x, y) = self.resolve_point(x, y, query)?;
        run_command("xdotool", &["mousemove", &x.to_string(), &y.to_string()])
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
        self.focus_query_app(query)?;
        let (x, y) = self.resolve_point(x, y, query)?;
        let button_arg = match button {
            "right" => "3",
            "middle" => "2",
            _ => "1",
        };
        run_command(
            "xdotool",
            &[
                "mousemove",
                &x.to_string(),
                &y.to_string(),
                "mousedown",
                button_arg,
            ],
        )?;
        std::thread::sleep(std::time::Duration::from_secs_f64(duration_secs.max(0.0)));
        run_command("xdotool", &["mouseup", button_arg])
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
        self.focus_query_app(query)?;
        let (from_x, from_y) = match from {
            Some(p) => p,
            None => self.resolve_point(None, None, query)?,
        };
        run_command(
            "xdotool",
            &[
                "mousemove",
                &from_x.to_string(),
                &from_y.to_string(),
                "mousedown",
                "1",
            ],
        )?;
        std::thread::sleep(std::time::Duration::from_secs_f64(
            hold_duration_secs.max(0.0),
        ));
        if duration_secs > 0.0 {
            std::thread::sleep(std::time::Duration::from_secs_f64(duration_secs));
        }
        run_command(
            "xdotool",
            &[
                "mousemove",
                &to.0.to_string(),
                &to.1.to_string(),
                "mouseup",
                "1",
            ],
        )?;
        Ok(ActionResult {
            method: "xdotool".to_string(),
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
            let _ = self.focus(app, None, None);
        }
        if clear {
            run_command("xdotool", &["key", "ctrl+a"])?;
        }
        run_command("xdotool", &["type", "--", text])
    }

    fn set_clipboard_text(&self, text: &str) -> SootieResult<ActionResult> {
        let candidates: [(&str, &[&str]); 3] = [
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        let mut errors = Vec::new();
        for (program, args) in candidates {
            match command_output_stdin_timeout(
                program,
                args,
                Some(text.as_bytes()),
                Duration::from_millis(1_000),
            ) {
                Ok(_) => {
                    return Ok(ActionResult {
                        method: program.to_string(),
                        details: json!({ "bytes": text.len() }),
                    });
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(SootieError::Platform(format!(
            "no Linux clipboard writer succeeded: {}",
            errors.join("; ")
        )))
    }

    fn clipboard_text(&self) -> SootieResult<String> {
        let candidates: [(&str, &[&str]); 3] = [
            ("wl-paste", &["-n"]),
            ("xclip", &["-selection", "clipboard", "-out"]),
            ("xsel", &["--clipboard", "--output"]),
        ];
        let mut errors = Vec::new();
        for (program, args) in candidates {
            match command_output_timeout(program, args, Duration::from_millis(1_000)) {
                Ok(output) => return Ok(output),
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(SootieError::Platform(format!(
            "no Linux clipboard reader succeeded: {}",
            errors.join("; ")
        )))
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
            let _ = self.focus(app, None, None);
        }
        let combo = xdotool_key_combo(key, modifiers)?;
        run_command("xdotool", &["key", &combo])
    }

    fn hotkey(&self, keys: &[String], app: Option<&str>) -> SootieResult<ActionResult> {
        let Some((key, modifiers)) = keys.split_last() else {
            return Err(SootieError::InvalidArguments(
                "keys must not be empty".to_string(),
            ));
        };
        if let Some(result) = self
            .cdp_port_for_app_filter(app)
            .and_then(|port| cdp::press_key(port, key, modifiers))
        {
            return Ok(result);
        }
        if let Some(app) = app {
            let _ = self.focus(app, None, None);
        }
        let combo = xdotool_key_combo(key, modifiers)?;
        run_command("xdotool", &["key", &combo])
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
            let _ = self.focus(app, None, None);
        }
        if let Some((x, y)) = at {
            let _ = self.hover(Some(x), Some(y), &FindQuery::default());
        }
        let button = match direction {
            "down" => "5",
            "left" => "6",
            "right" => "7",
            _ => "4",
        };
        for _ in 0..amount.abs().max(1) {
            run_command("xdotool", &["click", button])?;
        }
        Ok(ActionResult {
            method: "xdotool".to_string(),
            details: json!({ "direction": direction, "amount": amount }),
        })
    }

    fn focus(
        &self,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
    ) -> SootieResult<ActionResult> {
        let args = wmctrl_focus_args(app, platform_app_id, window);
        let args = args.iter().map(String::as_str).collect::<Vec<_>>();
        run_command("wmctrl", &args)
    }

    fn window(
        &self,
        command: WindowCommand,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
        bounds: Option<Bounds>,
    ) -> SootieResult<ActionResult> {
        let target_args = wmctrl_window_args(app, platform_app_id, window);
        match command {
            WindowCommand::List => Ok(ActionResult {
                method: "wmctrl".to_string(),
                details: json!({ "windows": self.state(Some(platform_app_id.unwrap_or(app)))? }),
            }),
            WindowCommand::Focus | WindowCommand::Restore => {
                self.focus(app, platform_app_id, window)
            }
            WindowCommand::Move | WindowCommand::Resize => {
                let Some(bounds) = bounds else {
                    return Err(SootieError::InvalidArguments(
                        "move/resize requires x/y/width/height".to_string(),
                    ));
                };
                let mut args = target_args.clone();
                args.push("-e".to_string());
                args.push(format!(
                    "0,{},{},{},{}",
                    bounds.x, bounds.y, bounds.width, bounds.height
                ));
                let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                run_command("wmctrl", &args)
            }
            WindowCommand::Minimize => {
                self.focus(app, platform_app_id, window)?;
                run_command("xdotool", &["getactivewindow", "windowminimize"])
            }
            WindowCommand::Maximize => {
                let mut args = target_args.clone();
                args.push("-b".to_string());
                args.push("add,maximized_vert,maximized_horz".to_string());
                let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                run_command("wmctrl", &args)
            }
            WindowCommand::Close => {
                let args = if target_args.first().map(String::as_str) == Some("-x") {
                    vec!["-x".to_string(), "-c".to_string(), target_args[2].clone()]
                } else {
                    vec!["-c".to_string(), target_args[1].clone()]
                };
                let args = args.iter().map(String::as_str).collect::<Vec<_>>();
                run_command("wmctrl", &args)
            }
        }
    }
}

fn xdotool_key_combo(key: &str, modifiers: &[String]) -> SootieResult<String> {
    if modifiers.is_empty() {
        return Ok(key.to_string());
    }
    let mut tokens = modifiers
        .iter()
        .map(|modifier| {
            let token = match modifier.to_lowercase().as_str() {
                "cmd" | "command" | "meta" | "ctrl" | "control" => "ctrl",
                "alt" | "option" => "alt",
                "shift" => "shift",
                "super" | "win" | "windows" => "super",
                other => {
                    return Err(SootieError::InvalidArguments(format!(
                        "unknown Linux modifier '{other}'"
                    )));
                }
            };
            Ok(token.to_string())
        })
        .collect::<SootieResult<Vec<_>>>()?;
    tokens.push(key.to_string());
    Ok(tokens.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x11_active_window_id() {
        assert_eq!(
            parse_active_window_id("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x05c00007"),
            Some("0x5c00007".to_string())
        );
        assert_eq!(parse_active_window_id("missing"), None);
    }

    #[test]
    fn normalizes_x11_window_ids() {
        assert_eq!(
            normalize_x11_window_id("0x05c00007"),
            Some("0x5c00007".to_string())
        );
        assert_eq!(
            normalize_x11_window_id("96468999"),
            Some("0x5c00007".to_string())
        );
    }

    #[test]
    fn maps_linux_browser_process_names() {
        assert_eq!(browser_process_name("Google-chrome"), Some("google-chrome"));
        assert_eq!(
            browser_process_name("google-chrome-stable"),
            Some("google-chrome")
        );
        assert_eq!(browser_process_name("chromium-browser"), Some("chromium"));
        assert_eq!(browser_process_name("Brave-browser"), Some("brave-browser"));
        assert_eq!(
            browser_process_name("Microsoft-edge"),
            Some("microsoft-edge")
        );
        assert_eq!(browser_process_name("Firefox"), Some("firefox"));
        assert_eq!(browser_process_name("Terminal"), None);
    }

    #[test]
    fn app_filter_matches_linux_identity_fields() {
        let app = AppInfo {
            name: "Firefox".to_string(),
            app_id: Some("Navigator.firefox".to_string()),
            platform_app_id: Some("Navigator.firefox".to_string()),
            pid: Some(42),
            bundle_id: None,
            is_frontmost: true,
            windows: Vec::new(),
        };
        assert!(app_filter_matches(&app, "firefox"));
        assert!(app_filter_matches(&app, "Navigator"));
        assert!(!app_filter_matches(&app, "terminal"));
    }

    #[test]
    fn maps_linux_modifier_aliases_to_xdotool_combo() {
        let combo = xdotool_key_combo(
            "l",
            &["cmd".to_string(), "alt".to_string(), "shift".to_string()],
        )
        .unwrap();
        assert_eq!(combo, "ctrl+alt+shift+l");
        assert_eq!(
            xdotool_key_combo("space", &["win".to_string()]).unwrap(),
            "super+space"
        );
    }

    #[test]
    fn rejects_unknown_linux_modifiers() {
        let error = xdotool_key_combo("l", &["hyper".to_string()]).unwrap_err();
        assert!(error.to_string().contains("unknown Linux modifier"));
    }

    #[test]
    fn parses_remote_debugging_port_from_cmdline() {
        assert_eq!(
            cdp::parse_remote_debugging_port(
                "google-chrome --profile-directory=Default --remote-debugging-port=9222"
            ),
            Some(9222)
        );
        assert_eq!(
            cdp::parse_remote_debugging_port("chromium --remote-debugging-port 9333"),
            Some(9333)
        );
        assert_eq!(
            cdp::parse_remote_debugging_port("firefox --new-window"),
            None
        );
    }

    #[test]
    fn parses_first_browser_cdp_port_from_process_commands() {
        let cmdlines = [
            "/usr/bin/terminal --remote-debugging-port=1111",
            "/usr/bin/google-chrome-stable --profile-directory=Default --remote-debugging-port=9222",
            "/usr/bin/chromium --remote-debugging-port 9333",
        ];
        assert_eq!(
            parse_first_browser_cdp_port_from_cmdlines(cmdlines),
            Some(9222)
        );
        assert_eq!(
            browser_process_name_from_cmdline("/opt/google/chrome/chrome --flag"),
            Some("google-chrome")
        );
        assert_eq!(
            parse_first_browser_cdp_port_from_cmdlines(["/usr/bin/firefox --new-window"]),
            None
        );
    }

    #[test]
    fn accessibility_script_uses_pyatspi_tree_walk() {
        let script = accessibility_elements_script();
        assert!(script.contains("import pyatspi"));
        assert!(script.contains("Registry.getDesktop"));
        assert!(script.contains("queryComponent"));
        assert!(script.contains("childCount"));
    }

    #[test]
    fn python_accessibility_script_is_executed_inline() {
        let script = accessibility_elements_script();
        let args = python_script_args(script, "Firefox", "123", "50");
        assert_eq!(args[0], "-c");
        assert_eq!(args[1], script);
        assert_eq!(args[2], "Firefox");
        assert_eq!(args[3], "123");
        assert_eq!(args[4], "50");
    }

    #[test]
    fn linux_diagnostic_from_probe_reports_recovery_on_failure() {
        let diagnostic = linux_diagnostic_from_probe(
            "linux_pyatspi",
            Err("python3: missing module".to_string()),
            "ok",
            "failed",
            "install pyatspi",
        );
        assert_eq!(diagnostic.name, "linux_pyatspi");
        assert!(!diagnostic.success);
        assert_eq!(diagnostic.message, "failed");
        assert_eq!(
            diagnostic.details.unwrap()["recovery"].as_str(),
            Some("install pyatspi")
        );
    }

    #[test]
    fn linux_backend_diagnostics_cover_runtime_prerequisites() {
        let diagnostics = LinuxBackend.diagnostics();
        let names = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.name.as_str())
            .collect::<Vec<_>>();

        for expected in [
            "linux_xprop",
            "linux_wmctrl",
            "linux_xdotool",
            "linux_pyatspi",
            "linux_screenshot",
        ] {
            assert!(names.contains(&expected), "missing diagnostic {expected}");
        }
    }

    #[test]
    fn window_target_prefers_non_empty_window_title() {
        assert_eq!(window_target("Firefox", Some("Downloads")), "Downloads");
        assert_eq!(window_target("Firefox", Some("  ")), "Firefox");
        assert_eq!(window_target("Firefox", None), "Firefox");
    }

    #[test]
    fn wmctrl_focus_args_use_platform_app_id_when_available() {
        assert_eq!(
            wmctrl_focus_args("Firefox", Some("Navigator.firefox"), None),
            vec!["-x", "-a", "Navigator.firefox"]
        );
        assert_eq!(
            wmctrl_focus_args("Firefox", Some("Navigator.firefox"), Some("Downloads")),
            vec!["-a", "Downloads"]
        );
        assert_eq!(
            wmctrl_focus_args("Firefox", None, None),
            vec!["-a", "Firefox"]
        );
    }

    #[test]
    fn wmctrl_window_args_use_platform_app_id_when_available() {
        assert_eq!(
            wmctrl_window_args("Firefox", Some("Navigator.firefox"), None),
            vec!["-x", "-r", "Navigator.firefox"]
        );
        assert_eq!(
            wmctrl_window_args("Firefox", Some("Navigator.firefox"), Some("Downloads")),
            vec!["-r", "Downloads"]
        );
        assert_eq!(
            wmctrl_window_args("Firefox", None, None),
            vec!["-r", "Firefox"]
        );
    }

    #[test]
    fn parses_accessibility_element_rows() {
        let row =
            "push button\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}True\u{1f}False";
        let record = parse_accessibility_element_row(row).unwrap();
        assert!(!record.focused);
        assert_eq!(record.element.role, "push button");
        assert_eq!(record.element.name.as_deref(), Some("Submit"));
        assert_eq!(record.element.id.as_deref(), Some("submit-button"));
        assert_eq!(record.element.bounds.unwrap().x, 10.0);
        assert_eq!(record.element.enabled, Some(true));
        assert_eq!(record.element.editable, Some(false));
        assert_eq!(record.element.actions, vec!["click"]);
    }

    #[test]
    fn parses_focused_editable_accessibility_rows() {
        let row =
            "text\u{1f}Search\u{1f}query\u{1f}search-input\u{1f}4\u{1f}5\u{1f}200\u{1f}24\u{1f}True\u{1f}True";
        let record = parse_accessibility_element_row(row).unwrap();
        assert!(record.focused);
        assert_eq!(record.element.text.as_deref(), Some("query"));
        assert_eq!(record.element.editable, Some(true));
        assert_eq!(record.element.actions, vec!["setValue"]);
    }

    #[test]
    fn accessibility_query_matches_name_role_and_identifier() {
        let record = parse_accessibility_element_row(
            "push button\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}True\u{1f}False",
        )
        .unwrap();
        assert!(element_matches_query(
            &record.element,
            &FindQuery {
                query: Some("sub".to_string()),
                role: Some("pushbutton".to_string()),
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
