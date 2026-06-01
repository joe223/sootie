use std::time::Duration;

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

pub struct WindowsBackend;

const BROWSER_URL_TIMEOUT_MS: u64 = 1_000;
const AUTOMATION_ELEMENTS_TIMEOUT_MS: u64 = 1_500;
const SCREENSHOT_TIMEOUT_MS: u64 = 5_000;
const WINDOWS_DIAGNOSTIC_TIMEOUT_MS: u64 = 1_000;

#[derive(Debug, Clone)]
struct AutomationElementRecord {
    element: ElementInfo,
    focused: bool,
}

fn powershell(script: &str) -> SootieResult<String> {
    command_output("powershell", &["-NoProfile", "-Command", script])
}

fn powershell_with_timeout(script: &str, timeout: Duration) -> SootieResult<String> {
    command_output_timeout("powershell", &["-NoProfile", "-Command", script], timeout)
}

fn windows_diagnostic_from_probe(
    name: &str,
    result: SootieResult<String>,
    success_message: &str,
    failure_message: &str,
    recovery: &str,
) -> RuntimeDiagnostic {
    match result {
        Ok(output) => RuntimeDiagnostic {
            name: name.to_string(),
            success: true,
            message: success_message.to_string(),
            details: Some(json!({ "output": output.trim() })),
        },
        Err(error) => RuntimeDiagnostic {
            name: name.to_string(),
            success: false,
            message: failure_message.to_string(),
            details: Some(json!({
                "error": error.to_string(),
                "recovery": recovery,
            })),
        },
    }
}

fn windows_powershell_diagnostic() -> RuntimeDiagnostic {
    windows_diagnostic_from_probe(
        "windows_powershell",
        powershell_with_timeout(
            "$PSVersionTable.PSVersion.ToString()",
            Duration::from_millis(WINDOWS_DIAGNOSTIC_TIMEOUT_MS),
        ),
        "Windows PowerShell probe succeeded",
        "Windows PowerShell probe failed for the Sootie launch path",
        "Run Sootie from a shell that can execute powershell -NoProfile -Command.",
    )
}

fn windows_uiautomation_diagnostic() -> RuntimeDiagnostic {
    windows_diagnostic_from_probe(
        "windows_uiautomation",
        powershell_with_timeout(
            "Add-Type -AssemblyName UIAutomationClient; [System.Windows.Automation.AutomationElement]::RootElement | Out-Null; 'ok'",
            Duration::from_millis(WINDOWS_DIAGNOSTIC_TIMEOUT_MS),
        ),
        "Windows UI Automation probe succeeded",
        "Windows UI Automation probe failed for the Sootie launch path",
        "Run Sootie from an interactive Windows desktop session with UI Automation access.",
    )
}

fn windows_forms_drawing_diagnostic() -> RuntimeDiagnostic {
    windows_diagnostic_from_probe(
        "windows_forms_drawing",
        powershell_with_timeout(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; 'ok'",
            Duration::from_millis(WINDOWS_DIAGNOSTIC_TIMEOUT_MS),
        ),
        "Windows Forms/System.Drawing probe succeeded",
        "Windows Forms/System.Drawing probe failed for the Sootie launch path",
        "Install or enable the Windows desktop assemblies needed for keyboard input and screenshots.",
    )
}

fn windows_visible_window_diagnostic() -> RuntimeDiagnostic {
    windows_diagnostic_from_probe(
        "windows_visible_window",
        powershell_with_timeout(
            r#"$window = Get-Process | Where-Object { $_.MainWindowTitle -and $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if (-not $window) { Write-Error "No visible top-level window"; exit 2 }
$window.ProcessName"#,
            Duration::from_millis(WINDOWS_DIAGNOSTIC_TIMEOUT_MS),
        ),
        "Windows visible window probe succeeded",
        "Windows visible window probe failed for the Sootie launch path",
        "Run Sootie from an interactive desktop session with at least one visible application window.",
    )
}

fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn browser_process_name(app_name: &str) -> Option<&'static str> {
    let normalized = app_name.to_lowercase();
    match normalized.as_str() {
        "chrome" | "google-chrome" | "google chrome" => Some("chrome"),
        "edge" | "microsoft edge" | "msedge" => Some("msedge"),
        "brave" | "brave browser" => Some("brave"),
        "chromium" => Some("chromium"),
        "firefox" | "mozilla firefox" => Some("firefox"),
        _ => None,
    }
}

fn browser_process_name_from_cmdline(cmdline: &str) -> Option<&'static str> {
    let lower = cmdline.to_lowercase();
    let exe_end = lower.find(".exe")? + 4;
    let executable = cmdline[..exe_end].trim().trim_matches('"');
    let name = executable
        .rsplit(['\\', '/'])
        .next()?
        .trim_end_matches(".exe");
    browser_process_name(name)
}

fn browser_url_script(process_name: &str) -> String {
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
$process = Get-Process -Name {} -ErrorAction SilentlyContinue | Where-Object {{ $_.MainWindowHandle -ne 0 }} | Select-Object -First 1
if (-not $process) {{ return "" }}
$root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
if (-not $root) {{ return "" }}
$condition = New-Object System.Windows.Automation.PropertyCondition(
  [System.Windows.Automation.AutomationElement]::ControlTypeProperty,
  [System.Windows.Automation.ControlType]::Edit
)
$edits = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, $condition)
foreach ($edit in $edits) {{
  $name = $edit.Current.Name
  $automationId = $edit.Current.AutomationId
  $pattern = $null
  if ($edit.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {{
    $value = $pattern.Current.Value
    if ($value -and ($value -match '^(https?://|file://|about:|chrome://|edge://|brave://|localhost|127\.0\.0\.1)' -or $name -match '(?i)(address|search|url)' -or $automationId -match '(?i)(address|url)')) {{
      return $value
    }}
  }}
}}
return ""
"#,
        ps_quote(process_name)
    )
}

fn browser_command_line_script(process_name: &str) -> String {
    let filter = format!("Name = '{}.exe'", process_name);
    format!(
        r#"
$process = Get-CimInstance Win32_Process -Filter {} | Select-Object -First 1
if (-not $process) {{ return "" }}
return $process.CommandLine
"#,
        ps_quote(&filter)
    )
}

fn browser_command_lines_script() -> &'static str {
    r#"
$names = @('chrome.exe', 'msedge.exe', 'brave.exe', 'chromium.exe', 'firefox.exe')
Get-CimInstance Win32_Process |
  Where-Object { $names -contains $_.Name } |
  ForEach-Object { $_.CommandLine }
"#
}

fn cdp_browser_url(process_name: &str) -> Option<String> {
    browser_cdp_port(process_name).and_then(cdp::current_page_url)
}

fn browser_cdp_port(process_name: &str) -> Option<u16> {
    let cmdline = powershell_with_timeout(
        &browser_command_line_script(process_name),
        Duration::from_millis(BROWSER_URL_TIMEOUT_MS),
    )
    .ok()?;
    cdp::parse_remote_debugging_port(cmdline.trim())
}

fn first_browser_cdp_port() -> Option<u16> {
    let cmdlines = powershell_with_timeout(
        browser_command_lines_script(),
        Duration::from_millis(BROWSER_URL_TIMEOUT_MS),
    )
    .ok()?;
    parse_first_browser_cdp_port_from_cmdlines(cmdlines.lines())
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

fn current_browser_url(app_name: &str) -> Option<String> {
    let process_name = browser_process_name(app_name)?;
    if let Some(url) = cdp_browser_url(process_name) {
        return Some(url);
    }
    powershell_with_timeout(
        &browser_url_script(process_name),
        Duration::from_millis(BROWSER_URL_TIMEOUT_MS),
    )
    .ok()
    .map(|output| output.trim().to_string())
    .filter(|url| !url.is_empty())
}

fn automation_elements_script(process_name: &str, max_elements: usize) -> String {
    format!(
        r#"
Add-Type -AssemblyName UIAutomationClient
$fieldSep = [char]31
$rowSep = [char]30
$rows = New-Object System.Collections.Generic.List[string]
$process = Get-Process -Name {} -ErrorAction SilentlyContinue | Where-Object {{ $_.MainWindowHandle -ne 0 }} | Select-Object -First 1
if (-not $process) {{ return "" }}
$root = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
if (-not $root) {{ return "" }}
$items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
foreach ($item in $items) {{
  if ($rows.Count -ge {}) {{ break }}
  $current = $item.Current
  $rect = $current.BoundingRectangle
  $role = ""
  if ($current.ControlType) {{ $role = $current.ControlType.ProgrammaticName -replace '^ControlType\.', '' }}
  $name = $current.Name
  $automationId = $current.AutomationId
  $value = ""
  $pattern = $null
  if ($item.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$pattern)) {{
    $value = $pattern.Current.Value
  }}
  if (-not $role) {{ continue }}
  if (-not $name -and -not $value -and -not $automationId -and ($rect.Width -le 0 -or $rect.Height -le 0)) {{ continue }}
  $fields = @($role, $name, $value, $automationId, $rect.X, $rect.Y, $rect.Width, $rect.Height, $current.IsEnabled, $current.HasKeyboardFocus)
  $rows.Add(($fields -join $fieldSep))
}}
return ($rows -join $rowSep)
"#,
        ps_quote(process_name),
        max_elements.max(1)
    )
}

fn automation_elements(app_name: &str) -> Vec<AutomationElementRecord> {
    powershell_with_timeout(
        &automation_elements_script(app_name, 400),
        Duration::from_millis(AUTOMATION_ELEMENTS_TIMEOUT_MS),
    )
    .ok()
    .map(|output| {
        output
            .split('\u{1e}')
            .filter_map(parse_automation_element_row)
            .collect()
    })
    .unwrap_or_default()
}

fn parse_automation_element_row(row: &str) -> Option<AutomationElementRecord> {
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
    Some(AutomationElementRecord {
        focused,
        element: ElementInfo {
            id,
            role: role.clone(),
            title,
            name,
            text,
            bounds,
            actions: actions_for_automation_role(&role, editable),
            editable: Some(editable),
            enabled,
        },
    })
}

fn non_empty_string(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
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
    role.contains("edit") || role.contains("document") || role.contains("combo")
}

fn actions_for_automation_role(role: &str, editable: bool) -> Vec<String> {
    let role = role.to_lowercase();
    let mut actions = Vec::new();
    if editable {
        actions.push("setValue".to_string());
    }
    if role.contains("button")
        || role.contains("checkbox")
        || role.contains("radio")
        || role.contains("menuitem")
        || role.contains("hyperlink")
        || role.contains("listitem")
    {
        actions.push("click".to_string());
    }
    actions
}

fn app_filter_matches(name: &str, platform_app_id: Option<&str>, filter: &str) -> bool {
    let filter = filter.to_lowercase();
    name.to_lowercase().contains(&filter)
        || platform_app_id.is_some_and(|id| id.to_lowercase().contains(&filter))
}

fn parse_window_state_rows(parsed: serde_json::Value, app: Option<&str>) -> Vec<AppInfo> {
    let rows = match parsed {
        serde_json::Value::Array(rows) => rows,
        serde_json::Value::Object(_) => vec![parsed],
        _ => vec![],
    };
    let mut apps = Vec::new();
    for row in rows {
        let name = row
            .get("ProcessName")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if name.is_empty() {
            continue;
        }
        let title = row
            .get("MainWindowTitle")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let pid = row.get("Id").and_then(|v| v.as_u64()).map(|v| v as u32);
        let platform_app_id = row
            .get("ExecutablePath")
            .and_then(|v| v.as_str())
            .and_then(non_empty_string);
        if let Some(filter) = app {
            if !app_filter_matches(&name, platform_app_id.as_deref(), filter) {
                continue;
            }
        }
        let is_frontmost = row
            .get("IsForeground")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let bounds = match (
            row.get("X").and_then(|value| value.as_f64()),
            row.get("Y").and_then(|value| value.as_f64()),
            row.get("Width").and_then(|value| value.as_f64()),
            row.get("Height").and_then(|value| value.as_f64()),
        ) {
            (Some(x), Some(y), Some(width), Some(height)) => Some(Bounds {
                x,
                y,
                width,
                height,
            }),
            _ => None,
        };
        apps.push(AppInfo {
            app_id: Some(name.clone()),
            platform_app_id,
            name,
            pid,
            bundle_id: None,
            is_frontmost,
            windows: vec![WindowInfo {
                id: pid.map(|pid| pid.to_string()),
                title,
                bounds,
                focused: is_frontmost,
            }],
        });
    }
    apps
}

fn window_state_script() -> &'static str {
    r#"$signature = @'
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
[DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint lpdwProcessId);
public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
'@
Add-Type -MemberDefinition $signature -Name NativeRect -Namespace Sootie
$foreground = [Sootie.NativeRect]::GetForegroundWindow()
$foregroundPid = 0
if ($foreground -ne [IntPtr]::Zero) {
  [Sootie.NativeRect]::GetWindowThreadProcessId($foreground, [ref]$foregroundPid) | Out-Null
}
Get-Process | Where-Object {$_.MainWindowTitle -and $_.MainWindowHandle -ne 0} | ForEach-Object {
  $path = ""
  try {
    if ($_.Path) { $path = $_.Path }
  } catch {}
  $rect = New-Object Sootie.NativeRect+RECT
  [Sootie.NativeRect]::GetWindowRect($_.MainWindowHandle, [ref]$rect) | Out-Null
  [PSCustomObject]@{
    Id=$_.Id
    ProcessName=$_.ProcessName
    ExecutablePath=$path
    MainWindowTitle=$_.MainWindowTitle
    X=$rect.Left
    Y=$rect.Top
    Width=($rect.Right - $rect.Left)
    Height=($rect.Bottom - $rect.Top)
    IsForeground=($_.Id -eq $foregroundPid)
  }
} | ConvertTo-Json -Compress"#
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
        .map(|wanted| element.role.eq_ignore_ascii_case(wanted))
        .unwrap_or(true);
    let matches_id = query
        .identifier
        .as_ref()
        .or(query.dom_id.as_ref())
        .map(|wanted| element.id.as_ref().is_some_and(|id| id == wanted))
        .unwrap_or(true);
    matches_query && matches_role && matches_id
}

fn user32_input_script(body: &str) -> String {
    format!(
        r#"$signature = @'
[DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
[DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, int dwData, UIntPtr dwExtraInfo);
'@
Add-Type -MemberDefinition $signature -Name NativeInput -Namespace Sootie
{body}"#
    )
}

fn windows_process_selector(
    app: &str,
    platform_app_id: Option<&str>,
    window: Option<&str>,
) -> String {
    let app_ref = platform_app_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(app);
    let mut selector = if app_ref.contains('\\') || app_ref.to_lowercase().ends_with(".exe") {
        format!(
            "Get-Process -ErrorAction SilentlyContinue | Where-Object {{ $_.Path -eq {} -and $_.MainWindowHandle -ne 0",
            ps_quote(app_ref)
        )
    } else {
        format!(
            "Get-Process -Name {} -ErrorAction SilentlyContinue | Where-Object {{ $_.MainWindowHandle -ne 0",
            ps_quote(app_ref)
        )
    };
    if let Some(window) = window.map(str::trim).filter(|window| !window.is_empty()) {
        selector.push_str(&format!(
            " -and $_.MainWindowTitle -like {}",
            ps_quote(&format!("*{window}*"))
        ));
    }
    selector.push_str(" } | Select-Object -First 1");
    selector
}

fn user32_window_script(
    app: &str,
    platform_app_id: Option<&str>,
    window: Option<&str>,
    body: &str,
) -> String {
    let selector = windows_process_selector(app, platform_app_id, window);
    let missing_target = window
        .map(str::trim)
        .filter(|window| !window.is_empty())
        .map(|window| format!("{} / {}", app, window))
        .unwrap_or_else(|| app.to_string());
    format!(
        r#"$signature = @'
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
[DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hWnd, int X, int Y, int nWidth, int nHeight, bool bRepaint);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam);
'@
Add-Type -MemberDefinition $signature -Name NativeWindow -Namespace Sootie
$p = {}
if (-not $p) {{ throw "window not found: {}" }}
$h = $p.MainWindowHandle
{body}"#,
        selector,
        missing_target.replace('"', "`\"")
    )
}

fn mouse_button_flags(button: &str) -> (u32, u32) {
    match button {
        "right" => (0x0008, 0x0010),
        "middle" => (0x0020, 0x0040),
        _ => (0x0002, 0x0004),
    }
}

fn send_keys_token(key: &str) -> String {
    match key.to_lowercase().as_str() {
        "return" | "enter" => "{ENTER}".to_string(),
        "tab" => "{TAB}".to_string(),
        "escape" | "esc" => "{ESC}".to_string(),
        "space" => " ".to_string(),
        "delete" => "{DEL}".to_string(),
        "backspace" => "{BACKSPACE}".to_string(),
        "left" => "{LEFT}".to_string(),
        "right" => "{RIGHT}".to_string(),
        "down" => "{DOWN}".to_string(),
        "up" => "{UP}".to_string(),
        "home" => "{HOME}".to_string(),
        "end" => "{END}".to_string(),
        "pageup" => "{PGUP}".to_string(),
        "pagedown" => "{PGDN}".to_string(),
        other if other.chars().count() == 1 => send_keys_text(other),
        other => format!("{{{}}}", other.to_uppercase()),
    }
}

fn send_keys_text(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars() {
        match ch {
            '\r' => {}
            '\n' => output.push_str("{ENTER}"),
            '+' | '^' | '%' | '~' | '(' | ')' | '[' | ']' => {
                output.push_str(&format!("{{{ch}}}"));
            }
            '{' => output.push_str("{{}"),
            '}' => output.push_str("{}}"),
            other => output.push(other),
        }
    }
    output
}

fn send_keys_combo(key: &str, modifiers: &[String]) -> SootieResult<String> {
    let mut prefix = String::new();
    for modifier in modifiers {
        match modifier.to_lowercase().as_str() {
            "cmd" | "command" | "meta" | "control" | "ctrl" => prefix.push('^'),
            "shift" => prefix.push('+'),
            "alt" | "option" => prefix.push('%'),
            "win" | "windows" | "super" => {
                return Err(SootieError::InvalidArguments(
                    "Windows-key modifier is not supported by SendKeys".to_string(),
                ));
            }
            other => {
                return Err(SootieError::InvalidArguments(format!(
                    "unknown Windows modifier '{other}'"
                )));
            }
        }
    }
    Ok(format!("{prefix}{}", send_keys_token(key)))
}

impl WindowsBackend {
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
        let process_name = browser_process_name(&app.name)?;
        browser_cdp_port(process_name)
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

impl DesktopBackend for WindowsBackend {
    fn platform(&self) -> &'static str {
        "windows"
    }

    fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
        vec![
            windows_powershell_diagnostic(),
            windows_uiautomation_diagnostic(),
            windows_forms_drawing_diagnostic(),
            windows_visible_window_diagnostic(),
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
        let automation_records = selected_app
            .map(|app| automation_elements(&app.name))
            .unwrap_or_default();
        let cdp_elements = self
            .cdp_port_for_app_filter(app)
            .and_then(cdp::page_elements)
            .unwrap_or_default();
        let interactive_elements = if automation_records.is_empty() && cdp_elements.is_empty() {
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
                    automation_records
                        .iter()
                        .map(|record| record.element.clone()),
                )
                .collect()
        };
        let focused_element = automation_records
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

    fn browser_url(&self, app: Option<&str>) -> SootieResult<Option<String>> {
        let apps = self.state(app)?;
        let selected_app = apps
            .iter()
            .find(|app| app.is_frontmost)
            .or_else(|| apps.first());
        Ok(selected_app
            .and_then(|app| current_browser_url(&app.name))
            .or_else(|| {
                if app.is_none() {
                    first_browser_cdp_port().and_then(cdp::current_page_url)
                } else {
                    self.cdp_port_for_app_filter(app)
                        .and_then(cdp::current_page_url)
                }
            }))
    }

    fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
        let output = powershell(window_state_script())?;
        let parsed: serde_json::Value =
            serde_json::from_str(output.trim()).unwrap_or(serde_json::Value::Null);
        Ok(parse_window_state_rows(parsed, app))
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
                automation_elements(&app.name)
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
        let path_str = path.to_string_lossy().replace('\\', "\\\\");
        let rect_script = window
            .as_ref()
            .and_then(|window| window.bounds.as_ref())
            .map(|bounds| {
                format!(
                    "$b=[Drawing.Rectangle]::new({},{},{},{});",
                    bounds.x.round() as i32,
                    bounds.y.round() as i32,
                    bounds.width.round().max(1.0) as i32,
                    bounds.height.round().max(1.0) as i32
                )
            })
            .unwrap_or_else(|| {
                "$b=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds;".to_string()
            });
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; {rect_script} $bmp=New-Object Drawing.Bitmap $b.Width,$b.Height; $g=[Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen([Drawing.Point]::new($b.X,$b.Y),[Drawing.Point]::Empty,[Drawing.Size]::new($b.Width,$b.Height)); $bmp.Save('{}',[Drawing.Imaging.ImageFormat]::Png)",
            path_str
        );
        powershell_with_timeout(&script, Duration::from_millis(SCREENSHOT_TIMEOUT_MS))?;
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
        let (down, up) = mouse_button_flags(button);
        let repetitions = count.max(1);
        let script = format!(
            "if (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}\nfor ($i = 0; $i -lt {}; $i++) {{ [Sootie.NativeInput]::mouse_event({}, 0, 0, 0, [UIntPtr]::Zero); [Sootie.NativeInput]::mouse_event({}, 0, 0, 0, [UIntPtr]::Zero) }}",
            x.round() as i32,
            y.round() as i32,
            repetitions,
            down,
            up
        );
        powershell(&user32_input_script(&script))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({ "x": x, "y": y, "button": button, "count": repetitions }),
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
        let script = format!(
            "if (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}",
            x.round() as i32,
            y.round() as i32
        );
        powershell(&user32_input_script(&script))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
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
        self.focus_query_app(query)?;
        let (x, y) = self.resolve_point(x, y, query)?;
        let (down, up) = mouse_button_flags(button);
        let duration_ms = (duration_secs.max(0.0) * 1000.0).round() as u64;
        let script = format!(
            "if (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}\n[Sootie.NativeInput]::mouse_event({}, 0, 0, 0, [UIntPtr]::Zero)\nStart-Sleep -Milliseconds {}\n[Sootie.NativeInput]::mouse_event({}, 0, 0, 0, [UIntPtr]::Zero)",
            x.round() as i32,
            y.round() as i32,
            down,
            duration_ms,
            up
        );
        powershell(&user32_input_script(&script))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
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
        self.focus_query_app(query)?;
        let from = match from {
            Some(p) => p,
            None => self.resolve_point(None, None, query)?,
        };
        let duration_ms = (duration_secs.max(0.0) * 1000.0).round() as u64;
        let hold_ms = (hold_duration_secs.max(0.0) * 1000.0).round() as u64;
        let script = format!(
            "if (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}\n[Sootie.NativeInput]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)\nStart-Sleep -Milliseconds {}\nif (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}\nStart-Sleep -Milliseconds {}\n[Sootie.NativeInput]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)",
            from.0.round() as i32,
            from.1.round() as i32,
            hold_ms,
            to.0.round() as i32,
            to.1.round() as i32,
            duration_ms
        );
        powershell(&user32_input_script(&script))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({
                "from": { "x": from.0, "y": from.1 },
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
            powershell("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait('^a')")?;
        }
        let escaped_text = send_keys_text(text);
        powershell(&format!("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait({})", ps_quote(&escaped_text)))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({ "bytes": text.len(), "clear": clear }),
        })
    }

    fn set_clipboard_text(&self, text: &str) -> SootieResult<ActionResult> {
        command_output_stdin_timeout(
            "powershell",
            &[
                "-NoProfile",
                "-Command",
                "[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false); Set-Clipboard -Value ([Console]::In.ReadToEnd())",
            ],
            Some(text.as_bytes()),
            Duration::from_millis(1_000),
        )?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({ "bytes": text.len() }),
        })
    }

    fn clipboard_text(&self) -> SootieResult<String> {
        command_output_timeout(
            "powershell",
            &["-NoProfile", "-Command", "Get-Clipboard -Raw"],
            Duration::from_millis(1_000),
        )
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
        let combo = send_keys_combo(key, modifiers)?;
        powershell(&format!("Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait({})", ps_quote(&combo)))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
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
            let _ = self.focus(app, None, None);
        }
        let mut script = String::new();
        if let Some((x, y)) = at {
            script.push_str(&format!(
                "if (-not [Sootie.NativeInput]::SetCursorPos({}, {})) {{ throw 'SetCursorPos failed' }}\n",
                x.round() as i32,
                y.round() as i32
            ));
        }
        let units = amount.abs().max(1) * 120;
        let (event, delta) = match direction {
            "down" => (0x0800, -units),
            "left" => (0x1000, -units),
            "right" => (0x1000, units),
            _ => (0x0800, units),
        };
        script.push_str(&format!(
            "[Sootie.NativeInput]::mouse_event({}, 0, 0, {}, [UIntPtr]::Zero)",
            event, delta
        ));
        powershell(&user32_input_script(&script))?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({ "direction": direction, "amount": amount, "at": at }),
        })
    }

    fn focus(
        &self,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
    ) -> SootieResult<ActionResult> {
        let selector = windows_process_selector(app, platform_app_id, window);
        let missing_target = window
            .map(str::trim)
            .filter(|window| !window.is_empty())
            .map(|window| format!("{app} / {window}"))
            .unwrap_or_else(|| app.to_string());
        let script = format!(
            "$p = {}; if (-not $p) {{ throw 'window not found: {}' }}; $wshell = New-Object -ComObject wscript.shell; if ($p.MainWindowTitle) {{ $wshell.AppActivate($p.MainWindowTitle) | Out-Null }} else {{ $wshell.AppActivate($p.Id) | Out-Null }}",
            selector,
            missing_target.replace('\'', "''")
        );
        powershell(&script)?;
        Ok(ActionResult {
            method: "powershell".to_string(),
            details: json!({ "app": app, "platform_app_id": platform_app_id, "window": window }),
        })
    }

    fn window(
        &self,
        command: WindowCommand,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
        bounds: Option<Bounds>,
    ) -> SootieResult<ActionResult> {
        match command {
            WindowCommand::List => Ok(ActionResult {
                method: "powershell".to_string(),
                details: json!({ "windows": self.state(Some(platform_app_id.unwrap_or(app)))? }),
            }),
            WindowCommand::Focus | WindowCommand::Restore => {
                self.focus(app, platform_app_id, window)
            }
            WindowCommand::Minimize => {
                powershell(&user32_window_script(
                    app,
                    platform_app_id,
                    window,
                    "[Sootie.NativeWindow]::ShowWindow($h, 6) | Out-Null",
                ))?;
                Ok(ActionResult {
                    method: "powershell".to_string(),
                    details: json!({ "app": app, "action": "minimize" }),
                })
            }
            WindowCommand::Maximize => {
                powershell(&user32_window_script(
                    app,
                    platform_app_id,
                    window,
                    "[Sootie.NativeWindow]::ShowWindow($h, 3) | Out-Null",
                ))?;
                Ok(ActionResult {
                    method: "powershell".to_string(),
                    details: json!({ "app": app, "action": "maximize" }),
                })
            }
            WindowCommand::Close => {
                powershell(&user32_window_script(
                    app,
                    platform_app_id,
                    window,
                    "[Sootie.NativeWindow]::PostMessage($h, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null",
                ))?;
                Ok(ActionResult {
                    method: "powershell".to_string(),
                    details: json!({ "app": app, "action": "close" }),
                })
            }
            WindowCommand::Move | WindowCommand::Resize => {
                let Some(bounds) = bounds else {
                    return Err(SootieError::InvalidArguments(
                        "move/resize requires x/y/width/height".to_string(),
                    ));
                };
                let body = format!(
                    "[Sootie.NativeWindow]::MoveWindow($h, {}, {}, {}, {}, $true) | Out-Null",
                    bounds.x.round() as i32,
                    bounds.y.round() as i32,
                    bounds.width.round().max(1.0) as i32,
                    bounds.height.round().max(1.0) as i32
                );
                powershell(&user32_window_script(app, platform_app_id, window, &body))?;
                Ok(ActionResult {
                    method: "powershell".to_string(),
                    details: json!({ "app": app, "platform_app_id": platform_app_id, "bounds": bounds }),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_browser_process_names() {
        assert_eq!(browser_process_name("Google Chrome"), Some("chrome"));
        assert_eq!(browser_process_name("google-chrome"), Some("chrome"));
        assert_eq!(browser_process_name("edge"), Some("msedge"));
        assert_eq!(browser_process_name("Brave Browser"), Some("brave"));
        assert_eq!(browser_process_name("Mozilla Firefox"), Some("firefox"));
        assert_eq!(browser_process_name("notepad"), None);
    }

    #[test]
    fn maps_windows_modifier_aliases_to_sendkeys_prefixes() {
        let combo = send_keys_combo(
            "l",
            &["cmd".to_string(), "alt".to_string(), "shift".to_string()],
        )
        .unwrap();
        assert_eq!(combo, "^%+l");
        assert_eq!(
            send_keys_combo("enter", &["ctrl".to_string()]).unwrap(),
            "^{ENTER}"
        );
    }

    #[test]
    fn escapes_windows_sendkeys_text_literals() {
        assert_eq!(
            send_keys_text("a+b^c%d~e(f)[g]{h}\n"),
            "a{+}b{^}c{%}d{~}e{(}f{)}{[}g{]}{{}h{}}{ENTER}"
        );
        assert_eq!(send_keys_token("+"), "{+}");
    }

    #[test]
    fn rejects_unsupported_windows_modifiers() {
        let error = send_keys_combo("l", &["win".to_string()]).unwrap_err();
        assert!(error.to_string().contains("not supported by SendKeys"));
        let error = send_keys_combo("l", &["hyper".to_string()]).unwrap_err();
        assert!(error.to_string().contains("unknown Windows modifier"));
    }

    #[test]
    fn windows_diagnostic_from_probe_reports_recovery_on_failure() {
        let diagnostic = windows_diagnostic_from_probe(
            "windows_uiautomation",
            Err(SootieError::Platform("missing assembly".to_string())),
            "ok",
            "failed",
            "enable UI Automation",
        );
        assert_eq!(diagnostic.name, "windows_uiautomation");
        assert!(!diagnostic.success);
        assert_eq!(diagnostic.message, "failed");
        assert_eq!(
            diagnostic.details.unwrap()["recovery"].as_str(),
            Some("enable UI Automation")
        );
    }

    #[test]
    fn windows_backend_diagnostics_cover_runtime_prerequisites() {
        let diagnostics = WindowsBackend.diagnostics();
        let names = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.name.as_str())
            .collect::<Vec<_>>();

        for expected in [
            "windows_powershell",
            "windows_uiautomation",
            "windows_forms_drawing",
            "windows_visible_window",
        ] {
            assert!(names.contains(&expected), "missing diagnostic {expected}");
        }
    }

    #[test]
    fn browser_url_script_uses_uia_value_pattern() {
        let script = browser_url_script("chrome");
        assert!(script.contains("UIAutomationClient"));
        assert!(script.contains("ControlType]::Edit"));
        assert!(script.contains("ValuePattern"));
        assert!(script.contains("address|search|url"));
    }

    #[test]
    fn browser_command_line_script_reads_debugging_args() {
        let script = browser_command_line_script("chrome");
        assert!(script.contains("Get-CimInstance Win32_Process"));
        assert!(script.contains("Name = ''chrome.exe''"));
        assert!(script.contains("CommandLine"));
    }

    #[test]
    fn browser_command_lines_script_reads_known_browsers() {
        let script = browser_command_lines_script();
        assert!(script.contains("chrome.exe"));
        assert!(script.contains("msedge.exe"));
        assert!(script.contains("CommandLine"));
        assert_eq!(
            parse_first_browser_cdp_port_from_cmdlines([
                r#"C:\Windows\System32\notepad.exe --remote-debugging-port=1111"#,
                r#"C:\Program Files\Google\Chrome\Application\chrome.exe --remote-debugging-port=9222"#,
                r#"C:\Program Files\Microsoft\Edge\Application\msedge.exe --remote-debugging-port 9333"#,
            ]),
            Some(9222)
        );
        assert_eq!(
            browser_process_name_from_cmdline(
                r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --flag"#
            ),
            Some("chrome")
        );
        assert_eq!(
            parse_first_browser_cdp_port_from_cmdlines([
                r#"C:\Program Files\Mozilla Firefox\firefox.exe"#
            ]),
            None
        );
    }

    #[test]
    fn window_state_script_reports_executable_path() {
        let script = window_state_script();
        assert!(script.contains("ExecutablePath=$path"));
        assert!(script.contains("try {"));
    }

    #[test]
    fn parses_window_state_rows_with_platform_app_id() {
        let rows = serde_json::json!([
            {
                "Id": 42,
                "ProcessName": "notepad",
                "ExecutablePath": "C:\\Windows\\System32\\notepad.exe",
                "MainWindowTitle": "notes.txt",
                "X": 10,
                "Y": 20,
                "Width": 300,
                "Height": 200,
                "IsForeground": true
            },
            {
                "Id": 43,
                "ProcessName": "calc",
                "ExecutablePath": "",
                "MainWindowTitle": "Calculator",
                "IsForeground": false
            }
        ]);
        let apps = parse_window_state_rows(rows, Some("note"));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "notepad");
        assert_eq!(apps[0].pid, Some(42));
        assert_eq!(
            apps[0].platform_app_id.as_deref(),
            Some("C:\\Windows\\System32\\notepad.exe")
        );
        assert_eq!(apps[0].bundle_id, None);
        assert!(apps[0].is_frontmost);
        assert_eq!(apps[0].windows[0].title, "notes.txt");
        assert_eq!(apps[0].windows[0].bounds.as_ref().unwrap().width, 300.0);

        assert!(app_filter_matches(
            "notepad",
            Some("C:\\Windows\\System32\\notepad.exe"),
            "system32"
        ));
    }

    #[test]
    fn automation_elements_script_collects_descendants() {
        let script = automation_elements_script("notepad", 25);
        assert!(script.contains("TreeScope]::Descendants"));
        assert!(script.contains("BoundingRectangle"));
        assert!(script.contains("$rows.Count -ge 25"));
        assert!(script.contains("ValuePattern"));
    }

    #[test]
    fn windows_process_selector_can_filter_by_window_title() {
        let app_only = windows_process_selector("notepad", None, None);
        assert!(app_only.contains("Get-Process -Name 'notepad'"));
        assert!(!app_only.contains("MainWindowTitle -like"));

        let with_window = windows_process_selector("notepad", None, Some("README"));
        assert!(with_window.contains("MainWindowTitle -like '*README*'"));
        assert!(with_window.contains("MainWindowHandle -ne 0"));

        let with_path =
            windows_process_selector("notepad", Some("C:\\Windows\\System32\\notepad.exe"), None);
        assert!(with_path.contains("Get-Process -ErrorAction SilentlyContinue"));
        assert!(with_path.contains("$_.Path -eq 'C:\\Windows\\System32\\notepad.exe'"));
    }

    #[test]
    fn user32_window_script_uses_window_selector() {
        let script = user32_window_script(
            "notepad",
            Some("C:\\Windows\\System32\\notepad.exe"),
            Some("README"),
            "body",
        );
        assert!(script.contains("$_.Path -eq 'C:\\Windows\\System32\\notepad.exe'"));
        assert!(script.contains("MainWindowTitle -like '*README*'"));
        assert!(script.contains("window not found: notepad / README"));
        assert!(script.contains("$h = $p.MainWindowHandle"));
        assert!(script.contains("body"));
    }

    #[test]
    fn parses_automation_element_rows() {
        let row =
            "Button\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}True\u{1f}False";
        let record = parse_automation_element_row(row).unwrap();
        assert!(!record.focused);
        assert_eq!(record.element.role, "Button");
        assert_eq!(record.element.name.as_deref(), Some("Submit"));
        assert_eq!(record.element.id.as_deref(), Some("submit-button"));
        assert_eq!(record.element.bounds.unwrap().height, 40.0);
        assert_eq!(record.element.enabled, Some(true));
        assert_eq!(record.element.editable, Some(false));
        assert_eq!(record.element.actions, vec!["click"]);
    }

    #[test]
    fn parses_focused_editable_automation_rows() {
        let row = "Edit\u{1f}Search\u{1f}query\u{1f}search-input\u{1f}4\u{1f}5\u{1f}200\u{1f}24\u{1f}True\u{1f}True";
        let record = parse_automation_element_row(row).unwrap();
        assert!(record.focused);
        assert_eq!(record.element.text.as_deref(), Some("query"));
        assert_eq!(record.element.editable, Some(true));
        assert_eq!(record.element.actions, vec!["setValue"]);
    }

    #[test]
    fn automation_query_matches_name_role_and_identifier() {
        let record = parse_automation_element_row(
            "Button\u{1f}Submit\u{1f}\u{1f}submit-button\u{1f}10\u{1f}20\u{1f}100\u{1f}40\u{1f}True\u{1f}False",
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
