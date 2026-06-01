use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::types::{
    ActionResult, AppInfo, Bounds, ContextSnapshot, ElementInfo, FindQuery, RuntimeDiagnostic,
    Screenshot, SootieError, SootieResult, WindowCommand,
};

pub(crate) mod cdp;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod null;
#[cfg(target_os = "windows")]
mod windows;

pub trait DesktopBackend: Send + Sync {
    fn platform(&self) -> &'static str;
    fn diagnostics(&self) -> Vec<RuntimeDiagnostic> {
        Vec::new()
    }
    fn context(&self, app: Option<&str>) -> SootieResult<ContextSnapshot>;
    fn browser_url(&self, app: Option<&str>) -> SootieResult<Option<String>> {
        Ok(self.context(app)?.url)
    }
    fn screen_locked(&self) -> SootieResult<Option<bool>> {
        Ok(None)
    }
    fn state(&self, app: Option<&str>) -> SootieResult<Vec<AppInfo>>;
    fn find(&self, query: &FindQuery) -> SootieResult<Vec<ElementInfo>>;
    fn read(
        &self,
        app: Option<&str>,
        query: Option<&str>,
        depth: Option<u32>,
    ) -> SootieResult<String>;
    fn inspect(&self, query: &FindQuery) -> SootieResult<Option<ElementInfo>>;
    fn element_at(&self, x: f64, y: f64) -> SootieResult<Option<ElementInfo>>;
    fn screenshot(
        &self,
        app: Option<&str>,
        window: Option<&str>,
        full_resolution: bool,
    ) -> SootieResult<Screenshot>;
    fn click(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        button: &str,
        count: u32,
    ) -> SootieResult<ActionResult>;
    fn hover(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
    ) -> SootieResult<ActionResult>;
    fn long_press(
        &self,
        x: Option<f64>,
        y: Option<f64>,
        query: &FindQuery,
        duration_secs: f64,
        button: &str,
    ) -> SootieResult<ActionResult>;
    fn drag(
        &self,
        from: Option<(f64, f64)>,
        to: (f64, f64),
        query: &FindQuery,
        duration_secs: f64,
        hold_duration_secs: f64,
    ) -> SootieResult<ActionResult>;
    fn type_text(&self, text: &str, target: &FindQuery, clear: bool) -> SootieResult<ActionResult>;
    fn set_clipboard_text(&self, _text: &str) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "set_clipboard_text"))
    }
    fn clipboard_text(&self) -> SootieResult<String> {
        Err(unsupported(self.platform(), "clipboard_text"))
    }
    fn press(
        &self,
        key: &str,
        modifiers: &[String],
        app: Option<&str>,
    ) -> SootieResult<ActionResult>;
    fn hotkey(&self, keys: &[String], app: Option<&str>) -> SootieResult<ActionResult>;
    fn scroll(
        &self,
        direction: &str,
        amount: i32,
        app: Option<&str>,
        at: Option<(f64, f64)>,
    ) -> SootieResult<ActionResult>;
    fn focus(
        &self,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
    ) -> SootieResult<ActionResult>;
    fn window(
        &self,
        command: WindowCommand,
        app: &str,
        platform_app_id: Option<&str>,
        window: Option<&str>,
        bounds: Option<Bounds>,
    ) -> SootieResult<ActionResult>;
}

pub fn create_backend() -> Box<dyn DesktopBackend> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxBackend)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosBackend)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsBackend)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Box::new(null::NullBackend)
    }
}

#[allow(dead_code)]
fn unsupported(platform: &str, feature: &str) -> SootieError {
    SootieError::Unsupported(format!(
        "{feature} is not available on the {platform} backend"
    ))
}

fn command_output(program: &str, args: &[&str]) -> SootieResult<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(SootieError::Platform(format!(
            "{} failed: {}",
            program,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[allow(dead_code)]
fn command_output_timeout(program: &str, args: &[&str], timeout: Duration) -> SootieResult<String> {
    command_output_stdin_timeout(program, args, None, timeout)
}

#[allow(dead_code)]
fn command_output_stdin_timeout(
    program: &str,
    args: &[&str],
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> SootieResult<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn()?;
    if let Some(input) = stdin {
        if let Some(mut child_stdin) = child.stdin.take() {
            child_stdin.write_all(input)?;
        }
    }
    drop(child.stdin.take());
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| SootieError::Platform(format!("{program} stdout pipe unavailable")))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| SootieError::Platform(format!("{program} stderr pipe unavailable")))?;
    let stdout_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let stderr_thread = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            let status = child.wait()?;
            let stdout = join_output(stdout_thread);
            let stderr = join_output(stderr_thread);
            if !status.success() {
                return Err(SootieError::Platform(format!(
                    "{} failed: {}",
                    program,
                    String::from_utf8_lossy(&stderr).trim()
                )));
            }
            return Ok(String::from_utf8_lossy(&stdout).to_string());
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_output(stdout_thread);
            let _ = join_output(stderr_thread);
            return Err(SootieError::Platform(format!(
                "{} timed out after {}ms",
                program,
                timeout.as_millis()
            )));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn join_output(handle: thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    handle.join().unwrap_or_default()
}

#[allow(dead_code)]
fn run_command(program: &str, args: &[&str]) -> SootieResult<ActionResult> {
    command_output(program, args)?;
    Ok(ActionResult {
        method: program.to_string(),
        details: serde_json::json!({ "args": args }),
    })
}

fn tmp_screenshot_path(extension: &str) -> PathBuf {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    std::env::temp_dir().join(format!("sootie-screenshot-{millis}.{extension}"))
}

fn png_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return (None, None);
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (Some(width), Some(height))
}

fn element_from_window(_app: &AppInfo, window: &crate::types::WindowInfo) -> ElementInfo {
    ElementInfo {
        id: window.id.clone(),
        role: "window".to_string(),
        title: Some(window.title.clone()),
        name: Some(window.title.clone()),
        text: None,
        bounds: window.bounds.clone(),
        actions: vec!["focus".to_string()],
        editable: Some(false),
        enabled: Some(true),
    }
}

fn filter_elements(apps: &[AppInfo], query: &FindQuery) -> Vec<ElementInfo> {
    let needle = query.query.as_ref().map(|q| q.to_lowercase());
    apps.iter()
        .filter(|app| {
            query
                .app
                .as_ref()
                .map(|wanted| app.name.to_lowercase().contains(&wanted.to_lowercase()))
                .unwrap_or(true)
        })
        .flat_map(|app| {
            app.windows
                .iter()
                .map(move |window| element_from_window(app, window))
        })
        .filter(|element| {
            query
                .role
                .as_ref()
                .map(|role| element.role.eq_ignore_ascii_case(role))
                .unwrap_or(true)
        })
        .filter(|element| {
            needle
                .as_ref()
                .map(|needle| {
                    element
                        .name
                        .as_ref()
                        .map(|name| name.to_lowercase().contains(needle))
                        .unwrap_or(false)
                })
                .unwrap_or(true)
        })
        .collect()
}

fn element_at_from_elements(
    elements: impl IntoIterator<Item = ElementInfo>,
    x: f64,
    y: f64,
) -> Option<ElementInfo> {
    elements.into_iter().find(|element| {
        element.bounds.as_ref().is_some_and(|bounds| {
            x >= bounds.x
                && y >= bounds.y
                && x <= bounds.x + bounds.width
                && y <= bounds.y + bounds.height
        })
    })
}

fn has_element_target(query: &FindQuery) -> bool {
    query.query.is_some()
        || query.role.is_some()
        || query.dom_id.is_some()
        || query.dom_class.is_some()
        || query.identifier.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_png_dimensions_from_header() {
        let mut bytes = vec![0; 24];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&800_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&600_u32.to_be_bytes());
        assert_eq!(png_dimensions(&bytes), (Some(800), Some(600)));
    }

    #[test]
    fn rejects_non_png_dimensions() {
        assert_eq!(png_dimensions(b"not-png"), (None, None));
    }

    #[test]
    fn finds_element_containing_point() {
        let element = ElementInfo {
            id: None,
            role: "button".to_string(),
            title: Some("Save".to_string()),
            name: Some("Save".to_string()),
            text: None,
            bounds: Some(Bounds {
                x: 10.0,
                y: 20.0,
                width: 80.0,
                height: 40.0,
            }),
            actions: vec![],
            editable: None,
            enabled: None,
        };
        assert_eq!(
            element_at_from_elements(vec![element], 30.0, 30.0).and_then(|element| element.name),
            Some("Save".to_string())
        );
    }

    #[test]
    fn detects_explicit_element_targets() {
        assert!(!has_element_target(&FindQuery::default()));
        assert!(has_element_target(&FindQuery {
            query: Some("Email".to_string()),
            ..Default::default()
        }));
    }

    #[cfg(unix)]
    #[test]
    fn command_output_timeout_drains_large_stdout() {
        let output = command_output_timeout(
            "sh",
            &[
                "-c",
                "i=0; while [ $i -lt 20000 ]; do printf 0123456789; i=$((i+1)); done",
            ],
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(output.len(), 200_000);
        assert!(output.starts_with("0123456789"));
    }
}
