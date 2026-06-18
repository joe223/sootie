use std::collections::HashMap;
use std::env;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};

use crate::backend::cdp;
use crate::types::{ActionResult, Screenshot, SootieError, SootieResult};

#[derive(Debug, Default)]
pub struct BrowserService {
    port: Option<u16>,
    selected_page_id: Option<String>,
    registry: BrowserElementRegistry,
    launches: HashMap<String, ManagedBrowser>,
}

#[derive(Debug)]
struct ManagedBrowser {
    child: Child,
    port: u16,
    browser: String,
    profile: Option<String>,
    headless: bool,
    user_data_dir: PathBuf,
}

#[derive(Debug, Default)]
struct BrowserElementRegistry {
    next_id: u64,
    by_ref: HashMap<String, BrowserElementRecord>,
    by_fingerprint: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct BrowserElementRecord {
    ref_id: String,
    page_id: String,
    fingerprint: String,
    selector_candidates: Vec<String>,
    dom_id: Option<String>,
    role: Option<String>,
    name: Option<String>,
    text: Option<String>,
    bounds: Option<Value>,
    last_seen_at: Instant,
}

impl BrowserService {
    pub fn launch(&mut self, args: &Value) -> SootieResult<Value> {
        let browser = str_arg(args, "browser").unwrap_or_else(|| "chrome".into());
        let profile = str_arg(args, "profile");
        let mode = str_arg(args, "mode");
        let incognito = launch_value_has_token(profile.as_deref(), "incognito")
            || launch_value_has_token(mode.as_deref(), "incognito");
        let headless = browser_launch_headless(args, mode.as_deref(), profile.as_deref());
        let port = u32_arg(args, "port")
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or_else(free_local_port);
        let timeout = timeout_arg(args, 15_000)?;
        let executable = browser_executable(&browser)?;
        let user_data_dir = str_arg(args, "user_data_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| managed_user_data_dir(port));
        std::fs::create_dir_all(&user_data_dir).map_err(|error| {
            SootieError::Platform(format!(
                "BROWSER_LAUNCH_FAILED: failed to create user data dir '{}': {error}",
                user_data_dir.display()
            ))
        })?;

        let mut command = Command::new(&executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for arg in browser_launch_args(port, &user_data_dir, incognito, headless) {
            command.arg(arg);
        }
        if let Some(url) = str_arg(args, "url") {
            command.arg(url);
        }

        let child = command.spawn().map_err(|error| {
            SootieError::Platform(format!(
                "BROWSER_LAUNCH_FAILED: failed to start '{}': {error}",
                executable.display()
            ))
        })?;
        let launch_id = format!("launch:{port}:{}", child.id());
        let pages = match wait_for_browser_pages(port, timeout) {
            Some(pages) => pages,
            None => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(SootieError::Platform(format!(
                    "BROWSER_LAUNCH_TIMEOUT: no reachable CDP endpoint on 127.0.0.1:{port} after {}ms",
                    timeout.as_millis()
                )));
            }
        };

        self.port = Some(port);
        self.selected_page_id = pages
            .iter()
            .find(|page| page.page_type == "page")
            .or_else(|| pages.first())
            .map(|page| page.target_id.clone());
        self.launches.insert(
            launch_id.clone(),
            ManagedBrowser {
                child,
                port,
                browser: browser.clone(),
                profile: profile.clone().or_else(|| mode.clone()),
                headless,
                user_data_dir: user_data_dir.clone(),
            },
        );

        Ok(json!({
            "connected": true,
            "browser_id": browser_id(port),
            "launch_id": launch_id,
            "endpoint": format!("http://127.0.0.1:{port}"),
            "browser": browser,
            "is_incognito": incognito,
            "is_headless": headless,
            "profile": profile.or(mode),
            "user_data_dir": user_data_dir,
            "pages": pages_payload(&pages, self.selected_page_id.as_deref()),
        }))
    }

    pub fn connect(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let timeout = timeout_arg(args, 1_000)?;
        let pages = wait_for_browser_pages(port, timeout)
            .ok_or_else(|| browser_not_connected(port, timeout))?;
        self.port = Some(port);
        if self.selected_page_id.is_none() {
            self.selected_page_id = pages
                .iter()
                .find(|page| page.page_type == "page")
                .or_else(|| pages.first())
                .map(|page| page.target_id.clone());
        }
        Ok(json!({
            "connected": true,
            "browser_id": browser_id(port),
            "endpoint": format!("http://127.0.0.1:{port}"),
            "pages": pages_payload(&pages, self.selected_page_id.as_deref()),
        }))
    }

    pub fn shutdown(&mut self, args: &Value) -> SootieResult<Value> {
        let launch_id = str_arg(args, "launch_id").or_else(|| {
            let port = self.port_from_args(args);
            self.launches
                .iter()
                .find_map(|(launch_id, launch)| (launch.port == port).then_some(launch_id.clone()))
        });
        let launch_id = launch_id.ok_or_else(|| {
            SootieError::InvalidArguments(
                "launch_id or browser_id/port for a managed launch is required".into(),
            )
        })?;
        let mut launch = self.launches.remove(&launch_id).ok_or_else(|| {
            SootieError::NotFound(format!(
                "BROWSER_LAUNCH_NOT_FOUND: no managed browser launch '{launch_id}'"
            ))
        })?;
        let _ = launch.child.kill();
        let status = launch.child.wait().ok();
        if self.port == Some(launch.port) {
            self.port = None;
            self.selected_page_id = None;
        }
        Ok(json!({
            "shutdown": true,
            "launch_id": launch_id,
            "browser_id": browser_id(launch.port),
            "browser": launch.browser,
            "profile": launch.profile,
            "is_headless": launch.headless,
            "user_data_dir": launch.user_data_dir,
            "exit_status": status.map(|status| status.to_string()),
        }))
    }

    pub fn pages(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let pages =
            cdp::browser_pages(port).ok_or_else(|| browser_not_connected(port, Duration::ZERO))?;
        self.port = Some(port);
        Ok(json!({
            "browser_id": browser_id(port),
            "pages": pages_payload(&pages, self.selected_page_id.as_deref()),
        }))
    }

    pub fn select_page(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page_id = required_str(args, "page_id")?;
        let page = cdp::page_by_id(port, Some(&page_id))
            .ok_or_else(|| SootieError::NotFound(format!("PAGE_NOT_FOUND: {page_id}")))?;
        self.port = Some(port);
        self.selected_page_id = Some(page.target_id.clone());
        Ok(json!({
            "browser_id": browser_id(port),
            "selected": true,
            "page": page_payload(&page, true),
        }))
    }

    pub fn open(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let url = required_str(args, "url")?;
        let new_page = bool_arg(args, "new_page").unwrap_or(true);
        let timeout = timeout_arg(args, 10_000)?;
        let page = if new_page {
            cdp::open_page(port, &url, true, timeout)
        } else {
            let page_id = self.page_id_arg(args);
            if let Some(page_id) = page_id.as_deref() {
                self.registry.clear_page(page_id);
            }
            cdp::navigate_page(port, page_id.as_deref(), &url, timeout)
        }
        .ok_or_else(|| cdp_failed("open URL", port, None))?;
        let wait_until = str_arg(args, "wait_until").unwrap_or_else(|| "domcontentloaded".into());
        let navigation_completed =
            cdp::wait_for_page_ready(port, Some(&page.target_id), &wait_until, timeout);
        self.port = Some(port);
        self.selected_page_id = Some(page.target_id.clone());
        Ok(json!({
            "page_id": page.target_id,
            "url": page.url,
            "title": page.title.unwrap_or_default(),
            "navigation_status": if navigation_completed { "completed" } else { "timeout" },
        }))
    }

    pub fn observe(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let max_elements = u32_arg(args, "max_elements").unwrap_or(200).clamp(1, 1000);
        let max_text_chars = u32_arg(args, "max_text_chars")
            .unwrap_or(10_000)
            .clamp(1, 200_000);
        let mode = str_arg(args, "mode").unwrap_or_else(|| "snapshot".into());
        let expression = browser_observe_expression(max_elements, max_text_chars);
        let mut payload = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| cdp_failed("observe", port, Some(&page)))?;
        payload["page"]["page_id"] = json!(page.target_id);
        payload["page"]["url"] = json!(page.url);
        payload["page"]["title"] = json!(page.title.unwrap_or_default());
        let include_elements =
            include_bool(args, "elements").unwrap_or(mode != "text" && mode != "screenshot");
        let include_text = include_bool(args, "text").unwrap_or(mode != "screenshot");
        if bool_arg(args, "viewport_only").unwrap_or(false) {
            filter_elements_to_viewport(&mut payload);
        }
        if !include_elements {
            payload["elements"] = json!([]);
        }
        if include_elements {
            self.registry
                .remember_elements(&page.target_id, &mut payload["elements"]);
        }
        if !include_text {
            if let Some(object) = payload.as_object_mut() {
                object.remove("text");
            }
        }
        self.port = Some(port);
        Ok(payload)
    }

    pub fn find(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let max_results = u32_arg(args, "max_results").unwrap_or(20).clamp(1, 500);
        let target = browser_target(args);
        let expression = format!(
            "(() => {{\nconst target = {};\nconst maxResults = {};\n{}\nreturn findBrowserElements(target, maxResults);\n}})()",
            serde_json::to_string(&target).unwrap_or_else(|_| "{}".into()),
            max_results,
            BROWSER_DOM_HELPERS,
        );
        let elements = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| cdp_failed("find", port, Some(&page)))?;
        let mut elements = elements;
        self.registry
            .remember_elements(&page.target_id, &mut elements);
        let count = elements.as_array().map(Vec::len).unwrap_or(0);
        self.port = Some(port);
        Ok(json!({
            "elements": elements,
            "count": count,
            "total_matches": count,
        }))
    }

    pub fn click(&mut self, args: &Value) -> SootieResult<Value> {
        let button = str_arg(args, "button").unwrap_or_else(|| "left".into());
        let count = u32_arg(args, "count").unwrap_or(1).max(1);
        let script = format!(
            "const buttonName = {};\nconst clickCount = {};\n{}",
            serde_json::to_string(&button).unwrap(),
            count,
            BROWSER_CLICK_BODY
        );
        let mut payload = self.target_action(args, "click", &script)?;
        let wait_after = str_arg(args, "wait_after").unwrap_or_else(|| "none".into());
        if wait_after != "none" {
            let port = self.port_from_args(args);
            let timeout = timeout_arg(args, 10_000)?;
            let page_id = payload
                .get("page")
                .and_then(|page| page.get("page_id"))
                .and_then(Value::as_str);
            let completed = cdp::wait_for_page_ready(port, page_id, &wait_after, timeout);
            payload["wait_after"] = json!({
                "condition": wait_after,
                "navigation_status": if completed { "completed" } else { "timeout" },
            });
        }
        Ok(payload)
    }

    pub fn type_text(&mut self, args: &Value) -> SootieResult<Value> {
        let text = required_str(args, "text")?;
        let clear = bool_arg(args, "clear").unwrap_or(false);
        let submit = bool_arg(args, "submit").unwrap_or(false);
        let mut target_args = args.clone();
        if let Some(object) = target_args.as_object_mut() {
            object.remove("text");
        }
        let script = format!(
            "const text = {};\nconst clear = {};\nconst submit = {};\n{}",
            serde_json::to_string(&text).unwrap(),
            clear,
            submit,
            BROWSER_TYPE_BODY
        );
        self.target_action(&target_args, "type", &script)
    }

    pub fn press(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let key = required_str(args, "key")?;
        let modifiers = string_array_arg(args, "modifiers");
        let result = cdp::press_key_on_page(port, Some(&page.target_id), &key, &modifiers)
            .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: key press failed".into()))?;
        self.port = Some(port);
        Ok(action_payload("press", &page, result, None))
    }

    pub fn scroll(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let direction = str_arg(args, "direction").unwrap_or_else(|| "down".into());
        let amount = browser_scroll_amount(args);
        let result = if has_target(args) {
            let script = format!(
                "const direction = {};\nconst amount = {};\n{}",
                serde_json::to_string(&direction).unwrap(),
                amount,
                BROWSER_SCROLL_TARGET_BODY
            );
            self.target_action(args, "scroll", &script)?
                .get("action_result")
                .cloned()
                .and_then(|value| serde_json::from_value::<ActionResult>(value).ok())
                .unwrap_or(ActionResult {
                    method: "cdp-scroll-target".into(),
                    details: json!({ "direction": direction, "amount": amount }),
                })
        } else {
            cdp::scroll_page_by_id(port, Some(&page.target_id), &direction, amount)
                .ok_or_else(|| cdp_failed("scroll", port, Some(&page)))?
        };
        self.port = Some(port);
        Ok(action_payload("scroll", &page, result, None))
    }

    pub fn wait(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let condition = required_str(args, "condition")?;
        let timeout = timeout_arg(args, 10_000)?;
        let interval = interval_arg(args, 250)?;
        let started = Instant::now();
        loop {
            if self.wait_condition_matches(port, &page.target_id, args, &condition)? {
                let latest = cdp::page_by_id(port, Some(&page.target_id)).unwrap_or(page);
                self.port = Some(port);
                return Ok(json!({
                    "condition": condition,
                    "matched": true,
                    "page": page_payload(&latest, true),
                    "elapsed_ms": started.elapsed().as_millis() as u64,
                }));
            }
            if started.elapsed() >= timeout {
                return Err(SootieError::Platform(format!(
                    "NAVIGATION_TIMEOUT: browser wait condition '{condition}' timed out"
                )));
            }
            std::thread::sleep(interval);
        }
    }

    pub fn extract(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let format = str_arg(args, "format").unwrap_or_else(|| "text".into());
        let max_chars = u32_arg(args, "max_chars")
            .unwrap_or(20_000)
            .clamp(1, 500_000);
        let target = self.extract_target(&page.target_id, args);
        let expression = format!(
            "(() => {{\nconst format = {};\nconst maxChars = {};\nconst target = {};\n{}\n{}\nreturn extractBrowserContent(format, maxChars, target);\n}})()",
            serde_json::to_string(&format).unwrap(),
            max_chars,
            serde_json::to_string(&target).unwrap_or_else(|_| "{}".into()),
            BROWSER_DOM_HELPERS,
            BROWSER_EXTRACT_BODY,
        );
        let extracted = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| cdp_failed("extract", port, Some(&page)))?;
        if extracted.get("ok").and_then(Value::as_bool) == Some(false) {
            return Err(SootieError::NotFound(format!(
                "TARGET_NOT_FOUND: {}",
                extracted
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("target not found")
            )));
        }
        self.port = Some(port);
        Ok(json!({
            "format": extracted.get("format").cloned().unwrap_or_else(|| json!(format)),
            "content": extracted.get("content").cloned().unwrap_or_else(|| json!("")),
            "truncated": extracted.get("truncated").cloned().unwrap_or(Value::Bool(false)),
            "source": {
                "page_id": page.target_id,
                "url": page.url,
                "title": page.title.unwrap_or_default(),
                "target": extracted.get("target").cloned(),
            }
        }))
    }

    pub fn screenshot(&mut self, args: &Value) -> SootieResult<Screenshot> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        self.port = Some(port);
        cdp::page_screenshot_by_id(
            port,
            Some(&page.target_id),
            bool_arg(args, "full_page").unwrap_or(false),
        )
        .ok_or_else(|| cdp_failed("screenshot", port, Some(&page)))
    }

    pub fn history(&mut self, args: &Value, action: &str) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let timeout = timeout_arg(args, 10_000)?;
        let latest = cdp::browser_history_action(port, Some(&page.target_id), action, timeout)
            .ok_or_else(|| cdp_failed("history action", port, Some(&page)))?;
        self.port = Some(port);
        Ok(json!({ "action": action, "page": page_payload(&latest, true) }))
    }

    pub fn close_page(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page_id = self
            .page_id_arg(args)
            .or_else(|| self.selected_page_id.clone())
            .ok_or_else(|| SootieError::InvalidArguments("page_id is required".into()))?;
        cdp::close_page(port, &page_id)
            .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: close page failed".into()))?;
        if self.selected_page_id.as_deref() == Some(page_id.as_str()) {
            self.selected_page_id = None;
        }
        self.registry.clear_page(&page_id);
        self.port = Some(port);
        Ok(json!({ "closed": true, "page_id": page_id }))
    }

    pub fn network(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        if bool_arg(args, "include_body").unwrap_or(false) {
            require_unsafe(args, "Network response body access")?;
            let request_id = required_str(args, "request_id")?;
            let body = cdp::send_cdp_command(
                port,
                Some(&page.target_id),
                "Network.getResponseBody",
                json!({ "requestId": request_id }),
                timeout_arg(args, 10_000)?,
            )
            .ok_or_else(|| {
                SootieError::Platform("CDP_COMMAND_FAILED: Network.getResponseBody failed".into())
            })?;
            self.port = Some(port);
            return Ok(json!({
                "page": page_payload(&page, true),
                "requests": [],
                "body": body,
            }));
        }
        let max_entries = u32_arg(args, "max_entries").unwrap_or(50).clamp(1, 500);
        let since_ms = u32_arg(args, "since_ms").unwrap_or(0);
        let url_contains = str_arg(args, "url_contains").unwrap_or_default();
        let resource_type = str_arg(args, "resource_type").unwrap_or_default();
        let expression = format!(
            "(() => {{\nconst maxEntries = {};\nconst sinceMs = {};\nconst urlContains = {};\nconst resourceType = {};\n{}\nreturn browserNetworkEntries(maxEntries, sinceMs, urlContains, resourceType);\n}})()",
            max_entries,
            since_ms,
            serde_json::to_string(&url_contains).unwrap(),
            serde_json::to_string(&resource_type).unwrap(),
            BROWSER_NETWORK_BODY
        );
        let requests = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| cdp_failed("network", port, Some(&page)))?;
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "requests": requests }))
    }

    pub fn console(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let max_entries = u32_arg(args, "max_entries").unwrap_or(100).clamp(1, 1000);
        let since_ms = u32_arg(args, "since_ms").unwrap_or(0);
        let level = str_arg(args, "level").unwrap_or_default();
        let expression = format!(
            "(() => {{\nconst maxEntries = {};\nconst sinceMs = {};\nconst level = {};\n{}\nreturn browserConsoleEntries(maxEntries, sinceMs, level);\n}})()",
            max_entries,
            since_ms,
            serde_json::to_string(&level).unwrap(),
            BROWSER_CONSOLE_BODY
        );
        let entries = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| cdp_failed("console", port, Some(&page)))?;
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "entries": entries }))
    }

    pub fn storage(&mut self, args: &Value) -> SootieResult<Value> {
        let area = required_str(args, "area")?;
        if !matches!(area.as_str(), "localStorage" | "sessionStorage") {
            return Err(SootieError::InvalidArguments(
                "area must be localStorage or sessionStorage".into(),
            ));
        }
        let action = required_str(args, "action")?;
        require_unsafe(args, "browser storage access")?;
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let key = str_arg(args, "key").unwrap_or_default();
        let value = str_arg(args, "value").unwrap_or_default();
        let origin = str_arg(args, "origin").unwrap_or_default();
        let expression = format!(
            "(() => {{\nconst areaName = {};\nconst action = {};\nconst key = {};\nconst value = {};\nconst origin = {};\n{}\nreturn browserStorageAction(areaName, action, key, value, origin);\n}})()",
            serde_json::to_string(&area).unwrap(),
            serde_json::to_string(&action).unwrap(),
            serde_json::to_string(&key).unwrap(),
            serde_json::to_string(&value).unwrap(),
            serde_json::to_string(&origin).unwrap(),
            BROWSER_STORAGE_BODY
        );
        let result = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            false,
            Duration::from_secs(3),
        )
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: storage failed".into()))?;
        if result.get("ok").and_then(Value::as_bool) == Some(false) {
            return Err(SootieError::InvalidArguments(
                result
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("storage action failed")
                    .to_string(),
            ));
        }
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "storage": result }))
    }

    pub fn cookies(&mut self, args: &Value) -> SootieResult<Value> {
        let action = required_str(args, "action")?;
        require_unsafe(args, "browser cookie access")?;
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let result = match action.as_str() {
            "list" => cdp::send_cdp_command(
                port,
                Some(&page.target_id),
                "Network.getCookies",
                json!({ "urls": [cookie_url_arg(args, &page.url)] }),
                timeout_arg(args, 10_000)?,
            ),
            "get" => {
                let name = required_str(args, "name")?;
                cdp::send_cdp_command(
                    port,
                    Some(&page.target_id),
                    "Network.getCookies",
                    json!({ "urls": [cookie_url_arg(args, &page.url)] }),
                    timeout_arg(args, 10_000)?,
                )
                .map(|mut value| {
                    let filtered = value["cookies"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|cookie| {
                            cookie.get("name").and_then(Value::as_str) == Some(name.as_str())
                        })
                        .collect::<Vec<_>>();
                    value["cookies"] = json!(filtered);
                    value
                })
            }
            "set" => {
                let mut params = serde_json::Map::new();
                params.insert("name".into(), json!(required_str(args, "name")?));
                params.insert("value".into(), json!(required_str(args, "value")?));
                params.insert("url".into(), json!(cookie_url_arg(args, &page.url)));
                insert_arg(&mut params, args, "domain", "domain");
                insert_arg(&mut params, args, "path", "path");
                insert_arg(&mut params, args, "same_site", "sameSite");
                if let Some(value) = f64_arg(args, "expires") {
                    params.insert("expires".into(), json!(value));
                }
                if let Some(value) = bool_arg(args, "http_only") {
                    params.insert("httpOnly".into(), json!(value));
                }
                if let Some(value) = bool_arg(args, "secure") {
                    params.insert("secure".into(), json!(value));
                }
                cdp::send_cdp_command(
                    port,
                    Some(&page.target_id),
                    "Network.setCookie",
                    Value::Object(params),
                    timeout_arg(args, 10_000)?,
                )
            }
            "remove" => cdp::send_cdp_command(
                port,
                Some(&page.target_id),
                "Network.deleteCookies",
                json!({
                    "name": required_str(args, "name")?,
                    "url": cookie_url_arg(args, &page.url)
                }),
                timeout_arg(args, 10_000)?,
            ),
            "clear" => cdp::send_cdp_command(
                port,
                Some(&page.target_id),
                "Network.clearBrowserCookies",
                json!({}),
                timeout_arg(args, 10_000)?,
            ),
            _ => {
                return Err(SootieError::InvalidArguments(
                    "action must be list/get/set/remove/clear".into(),
                ));
            }
        }
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: cookies failed".into()))?;
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "cookies": result }))
    }

    pub fn downloads(&mut self, args: &Value) -> SootieResult<Value> {
        require_unsafe(args, "changing browser download behavior")?;
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let action = required_str(args, "action")?;
        let behavior = match action.as_str() {
            "deny" | "allow" | "allowAndName" | "default" => action,
            _ => {
                return Err(SootieError::InvalidArguments(
                    "action must be deny/allow/allowAndName/default".into(),
                ));
            }
        };
        let mut params = json!({ "behavior": behavior });
        if let Some(path) = str_arg(args, "download_path") {
            params["downloadPath"] = json!(path);
        }
        let result = cdp::send_cdp_command(
            port,
            Some(&page.target_id),
            "Browser.setDownloadBehavior",
            params,
            timeout_arg(args, 10_000)?,
        )
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: downloads failed".into()))?;
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "downloads": result }))
    }

    pub fn upload(&mut self, args: &Value) -> SootieResult<Value> {
        require_unsafe(args, "setting browser file input paths")?;
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let file_paths = string_array_arg(args, "file_paths");
        if file_paths.is_empty() {
            return Err(SootieError::InvalidArguments(
                "file_paths must contain at least one path".into(),
            ));
        }
        let target = self.resolved_browser_target(&page.target_id, args);
        let result = cdp::set_file_input_files(
            port,
            Some(&page.target_id),
            &target,
            &file_paths,
            timeout_arg(args, 10_000)?,
        )
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: upload failed".into()))?;
        self.port = Some(port);
        Ok(json!({ "page": page_payload(&page, true), "upload": result }))
    }

    pub fn pdf(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let mut params = serde_json::Map::new();
        if let Some(value) = bool_arg(args, "landscape") {
            params.insert("landscape".into(), json!(value));
        }
        if let Some(value) = bool_arg(args, "print_background") {
            params.insert("printBackground".into(), json!(value));
        }
        if let Some(value) = f64_arg(args, "scale") {
            params.insert("scale".into(), json!(value));
        }
        if let Some(value) = f64_arg(args, "paper_width") {
            params.insert("paperWidth".into(), json!(value));
        }
        if let Some(value) = f64_arg(args, "paper_height") {
            params.insert("paperHeight".into(), json!(value));
        }
        let result = cdp::send_cdp_command(
            port,
            Some(&page.target_id),
            "Page.printToPDF",
            Value::Object(params),
            timeout_arg(args, 30_000)?,
        )
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: printToPDF failed".into()))?;
        let data = result
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.port = Some(port);
        Ok(json!({
            "page": page_payload(&page, true),
            "mime_type": "application/pdf",
            "data_base64": data,
            "byte_length": base64::engine::general_purpose::STANDARD.decode(&data).map(|bytes| bytes.len()).unwrap_or(0),
        }))
    }

    pub fn cdp_send(&mut self, args: &Value) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args).ok();
        let method = raw_cdp_method(args)?;
        require_raw_cdp(args, &method)?;
        let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
        let result = cdp::send_cdp_command(
            port,
            page.as_ref().map(|page| page.target_id.as_str()),
            &method,
            params.clone(),
            timeout_arg(args, 10_000)?,
        )
        .ok_or_else(|| cdp_failed("raw CDP command", port, page.as_ref()))?;
        self.port = Some(port);
        Ok(json!({
            "method": method,
            "params": params,
            "result": result,
            "page": page.as_ref().map(|page| page_payload(page, true)),
        }))
    }

    pub fn cdp_subscribe(&mut self, args: &Value) -> SootieResult<Value> {
        let domain = required_str(args, "domain")?;
        require_raw_cdp(args, &format!("{domain}.enable"))?;
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let event = str_arg(args, "event");
        let max_events = u32_arg(args, "max_events").unwrap_or(20).clamp(1, 200);
        let events = cdp::collect_cdp_events(
            port,
            Some(&page.target_id),
            &domain,
            event.as_deref(),
            timeout_arg(args, 2_000)?,
            max_events,
        )
        .ok_or_else(|| SootieError::Platform("CDP_COMMAND_FAILED: cdp subscribe failed".into()))?;
        self.port = Some(port);
        Ok(json!({
            "domain": domain,
            "event": event,
            "events": events,
            "page": page_payload(&page, true),
        }))
    }

    fn target_action(&mut self, args: &Value, action: &str, body: &str) -> SootieResult<Value> {
        let port = self.port_from_args(args);
        let page = self.selected_page(port, args)?;
        let target = self.resolved_browser_target(&page.target_id, args);
        let expression = format!(
            "(() => {{\nconst target = {};\n{}\n{}\n}})()",
            serde_json::to_string(&target).unwrap_or_else(|_| "{}".into()),
            BROWSER_DOM_HELPERS,
            body,
        );
        let value = cdp::evaluate_on_page(
            port,
            Some(&page.target_id),
            &expression,
            true,
            Duration::from_secs(10),
        )
        .ok_or_else(|| SootieError::Platform(format!("CDP_COMMAND_FAILED: {action} failed")))?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(SootieError::NotFound(format!(
                "TARGET_NOT_FOUND: {}",
                value
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("target not found")
            )));
        }
        let result = ActionResult {
            method: format!("cdp-browser-{action}"),
            details: value.clone(),
        };
        self.port = Some(port);
        Ok(action_payload(
            action,
            &page,
            result,
            value.get("target").cloned(),
        ))
    }

    fn resolved_browser_target(&self, page_id: &str, args: &Value) -> Value {
        self.registry
            .resolve_target(page_id, args)
            .unwrap_or_else(|| browser_target(args))
    }

    fn extract_target(&self, page_id: &str, args: &Value) -> Value {
        let Some(target) = args.get("target").and_then(Value::as_object) else {
            return self.resolved_browser_target(page_id, args);
        };
        if target.get("page").and_then(Value::as_bool).unwrap_or(false) {
            return json!({ "page": true });
        }
        let target = Value::Object(target.clone());
        self.registry
            .resolve_target(page_id, &target)
            .unwrap_or(target)
    }

    fn selected_page(&self, port: u16, args: &Value) -> SootieResult<cdp::PageInfo> {
        let page_id = self
            .page_id_arg(args)
            .or_else(|| self.selected_page_id.clone());
        cdp::page_by_id(port, page_id.as_deref()).ok_or_else(|| {
            SootieError::NotFound("PAGE_NOT_FOUND: no browser page is available".into())
        })
    }

    fn port_from_args(&self, args: &Value) -> u16 {
        u32_arg(args, "port")
            .and_then(|port| u16::try_from(port).ok())
            .or_else(|| {
                str_arg(args, "ws_url")
                    .as_deref()
                    .and_then(port_from_ws_url)
            })
            .or_else(|| {
                str_arg(args, "browser_id")
                    .and_then(|id| id.strip_prefix("cdp:").and_then(|port| port.parse().ok()))
            })
            .or(self.port)
            .or_else(cdp::configured_port)
            .unwrap_or(9222)
    }

    fn page_id_arg(&self, args: &Value) -> Option<String> {
        str_arg(args, "page_id").filter(|value| !value.trim().is_empty())
    }

    fn wait_condition_matches(
        &self,
        port: u16,
        page_id: &str,
        args: &Value,
        condition: &str,
    ) -> SootieResult<bool> {
        match condition {
            "load" | "domcontentloaded" | "networkidle" | "stable" | "none" => Ok(
                cdp::wait_for_page_ready(port, Some(page_id), condition, Duration::from_millis(1)),
            ),
            "urlContains" => {
                let value = required_str(args, "value")?;
                Ok(cdp::page_by_id(port, Some(page_id))
                    .is_some_and(|page| page.url.contains(&value)))
            }
            "urlChanged" => {
                let value = required_str(args, "value")?;
                Ok(cdp::page_by_id(port, Some(page_id)).is_some_and(|page| page.url != value))
            }
            "titleContains" => {
                let value = required_str(args, "value")?;
                Ok(cdp::page_by_id(port, Some(page_id))
                    .is_some_and(|page| page.title.unwrap_or_default().contains(&value)))
            }
            "titleChanged" => {
                let value = required_str(args, "value")?;
                Ok(cdp::page_by_id(port, Some(page_id))
                    .is_some_and(|page| page.title.unwrap_or_default() != value))
            }
            "textExists" => {
                let value = required_str(args, "value")?;
                let expression = browser_visible_text_expression(200_000);
                Ok(cdp::evaluate_on_page(
                    port,
                    Some(page_id),
                    &expression,
                    false,
                    Duration::from_secs(2),
                )
                .and_then(|text| text.as_str().map(|text| text.contains(&value)))
                .unwrap_or(false))
            }
            "elementExists" | "elementGone" => {
                let mut target = browser_target(args);
                if target.as_object().is_some_and(|object| object.is_empty()) {
                    if let Some(value) = str_arg(args, "value") {
                        target["query"] = json!(value);
                    }
                }
                let expression = format!(
                    "(() => {{\nconst target = {};\n{}\nreturn findBrowserElements(target, 1).length > 0;\n}})()",
                    serde_json::to_string(&target).unwrap_or_else(|_| "{}".into()),
                    BROWSER_DOM_HELPERS,
                );
                let exists = cdp::evaluate_on_page(
                    port,
                    Some(page_id),
                    &expression,
                    false,
                    Duration::from_secs(2),
                )
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
                Ok(if condition == "elementGone" {
                    !exists
                } else {
                    exists
                })
            }
            other => Err(SootieError::InvalidArguments(format!(
                "unsupported browser wait condition '{other}'"
            ))),
        }
    }
}

fn pages_payload(pages: &[cdp::PageInfo], selected_page_id: Option<&str>) -> Value {
    Value::Array(
        pages
            .iter()
            .map(|page| page_payload(page, selected_page_id == Some(page.target_id.as_str())))
            .collect(),
    )
}

fn page_payload(page: &cdp::PageInfo, active: bool) -> Value {
    json!({
        "page_id": page.target_id,
        "target_id": page.target_id,
        "title": page.title.clone().unwrap_or_default(),
        "url": page.url,
        "active": active,
        "type": page.page_type,
    })
}

fn action_payload(
    action: &str,
    page: &cdp::PageInfo,
    result: ActionResult,
    target: Option<Value>,
) -> Value {
    json!({
        "action": action,
        "page": page_payload(page, true),
        "target": target,
        "method": result.method,
        "details": result.details,
        "action_result": result,
    })
}

impl BrowserElementRegistry {
    fn remember_elements(&mut self, page_id: &str, elements: &mut Value) {
        let Some(elements) = elements.as_array_mut() else {
            return;
        };
        for element in elements {
            let Some(fingerprint) = element_fingerprint(page_id, element) else {
                continue;
            };
            let ref_id = self
                .by_fingerprint
                .get(&fingerprint)
                .cloned()
                .unwrap_or_else(|| {
                    self.next_id = self.next_id.saturating_add(1);
                    let ref_id = format!("br_{}", self.next_id);
                    self.by_fingerprint
                        .insert(fingerprint.clone(), ref_id.clone());
                    ref_id
                });
            let volatile_ref = element
                .get("ref")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(volatile_ref) = volatile_ref {
                element["volatile_ref"] = json!(volatile_ref);
            }
            element["ref"] = json!(ref_id.clone());
            let record = BrowserElementRecord {
                ref_id: ref_id.clone(),
                page_id: page_id.to_string(),
                fingerprint,
                selector_candidates: string_vec_field(element, "selector_candidates"),
                dom_id: string_field(element, "dom_id"),
                role: string_field(element, "role"),
                name: string_field(element, "name"),
                text: string_field(element, "text"),
                bounds: element.get("bounds").cloned(),
                last_seen_at: Instant::now(),
            };
            self.by_ref.insert(ref_id, record);
        }
    }

    fn resolve_target(&self, page_id: &str, args: &Value) -> Option<Value> {
        let ref_id = args.get("ref").and_then(Value::as_str)?;
        let record = self.by_ref.get(ref_id)?;
        if record.page_id != page_id {
            return None;
        }
        if record.last_seen_at.elapsed() > Duration::from_secs(30 * 60) {
            return None;
        }
        let mut target = browser_target(args);
        let target_object = target.as_object_mut()?;
        if let Some(selector) = record
            .selector_candidates
            .iter()
            .find(|selector| is_durable_selector_candidate(selector))
        {
            target_object.insert("selector".to_string(), json!(selector));
        } else if let Some(dom_id) = &record.dom_id {
            target_object.insert("dom_id".to_string(), json!(dom_id));
        } else {
            insert_optional_string(target_object, "role", record.role.as_deref());
            insert_optional_string(target_object, "name", record.name.as_deref());
            insert_optional_string(target_object, "text", record.text.as_deref());
            if let Some(bounds) = &record.bounds {
                if let (Some(x), Some(y), Some(width), Some(height)) = (
                    bounds.get("x").and_then(Value::as_f64),
                    bounds.get("y").and_then(Value::as_f64),
                    bounds.get("width").and_then(Value::as_f64),
                    bounds.get("height").and_then(Value::as_f64),
                ) {
                    target_object.insert("x".to_string(), json!(x + width / 2.0));
                    target_object.insert("y".to_string(), json!(y + height / 2.0));
                }
            }
        }
        target_object.insert("ref".to_string(), json!(record.ref_id));
        Some(target)
    }

    fn clear_page(&mut self, page_id: &str) {
        let refs = self
            .by_ref
            .iter()
            .filter_map(|(ref_id, record)| (record.page_id == page_id).then_some(ref_id.clone()))
            .collect::<Vec<_>>();
        for ref_id in refs {
            if let Some(record) = self.by_ref.remove(&ref_id) {
                self.by_fingerprint.remove(&record.fingerprint);
            }
        }
    }
}

fn is_durable_selector_candidate(selector: &str) -> bool {
    selector.starts_with('#')
        || selector.starts_with("[data-testid=")
        || selector.contains("[name=")
        || selector.contains('.')
}

fn element_fingerprint(page_id: &str, element: &Value) -> Option<String> {
    if let Some(dom_id) = string_field(element, "dom_id") {
        return Some(format!("{page_id}|id:{dom_id}"));
    }
    let selector = string_vec_field(element, "selector_candidates")
        .into_iter()
        .next();
    let role = string_field(element, "role").unwrap_or_default();
    let name = string_field(element, "name").unwrap_or_default();
    let text = string_field(element, "text").unwrap_or_default();
    selector
        .or_else(|| (!role.is_empty() || !name.is_empty() || !text.is_empty()).then(String::new))
        .map(|selector| format!("{page_id}|sel:{selector}|role:{role}|name:{name}|text:{text}"))
}

fn insert_optional_string(
    target: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        target.insert(key.to_string(), json!(value));
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn string_vec_field(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn browser_not_connected(port: u16, timeout: Duration) -> SootieError {
    let waited = if timeout.is_zero() {
        String::new()
    } else {
        format!(" after {}ms", timeout.as_millis())
    };
    SootieError::Platform(format!(
        "BROWSER_NOT_CONNECTED: no reachable CDP endpoint on 127.0.0.1:{port}{waited}"
    ))
}

fn wait_for_browser_pages(port: u16, timeout: Duration) -> Option<Vec<cdp::PageInfo>> {
    let started = Instant::now();
    loop {
        if let Some(pages) = cdp::browser_pages(port) {
            return Some(pages);
        }
        if started.elapsed() >= timeout {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn free_local_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|listener| listener.local_addr().ok())
        .map(|addr| addr.port())
        .unwrap_or(9222)
}

fn managed_user_data_dir(port: u16) -> PathBuf {
    let stamp = Instant::now().elapsed().as_nanos();
    env::temp_dir().join(format!("sootie-cdp-{port}-{}-{stamp}", std::process::id()))
}

fn browser_executable(browser: &str) -> SootieResult<PathBuf> {
    let normalized = browser.to_ascii_lowercase();
    let candidates = browser_executable_candidates(&normalized);
    candidates
        .into_iter()
        .find(|path| path.exists() || path.components().count() == 1)
        .ok_or_else(|| {
            SootieError::Platform(format!(
                "BROWSER_LAUNCH_FAILED: no executable found for browser '{browser}'"
            ))
        })
}

fn browser_executable_candidates(browser: &str) -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        match browser {
            "edge" | "msedge" => vec![PathBuf::from(
                "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            )],
            "chromium" => vec![
                PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
                PathBuf::from("chromium"),
            ],
            _ => vec![
                PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
                PathBuf::from("google-chrome"),
                PathBuf::from("chromium"),
            ],
        }
    }
    #[cfg(target_os = "windows")]
    {
        match browser {
            "edge" | "msedge" => vec![PathBuf::from("msedge.exe")],
            "chromium" => vec![PathBuf::from("chromium.exe")],
            _ => vec![
                PathBuf::from("chrome.exe"),
                PathBuf::from("google-chrome.exe"),
            ],
        }
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        match browser {
            "edge" | "msedge" => vec![PathBuf::from("microsoft-edge"), PathBuf::from("msedge")],
            "chromium" => vec![PathBuf::from("chromium"), PathBuf::from("chromium-browser")],
            _ => vec![
                PathBuf::from("google-chrome"),
                PathBuf::from("chrome"),
                PathBuf::from("chromium"),
                PathBuf::from("chromium-browser"),
            ],
        }
    }
}

fn cdp_failed(operation: &str, port: u16, page: Option<&cdp::PageInfo>) -> SootieError {
    let page_context = page
        .map(|page| {
            format!(
                "; page_id={}; url={}; title={}",
                page.target_id,
                page.url,
                page.title.as_deref().unwrap_or("")
            )
        })
        .unwrap_or_default();
    SootieError::Platform(format!(
        "CDP_COMMAND_FAILED: {operation} failed; port={port}{page_context}; suggested_next_call=sootie_browser_pages"
    ))
}

fn browser_id(port: u16) -> String {
    format!("cdp:{port}")
}

fn browser_target(args: &Value) -> Value {
    let mut target = serde_json::Map::new();
    for key in [
        "ref",
        "selector",
        "dom_id",
        "dom_class",
        "role",
        "name",
        "text",
        "query",
        "into",
    ] {
        if let Some(value) = str_arg(args, key) {
            let key = if key == "into" { "query" } else { key };
            target.insert(key.to_string(), json!(value));
        }
    }
    if let Some(value) = bool_arg(args, "focused") {
        target.insert("focused".to_string(), json!(value));
    }
    if let Some(x) = f64_arg(args, "x") {
        target.insert("x".to_string(), json!(x));
    }
    if let Some(y) = f64_arg(args, "y") {
        target.insert("y".to_string(), json!(y));
    }
    Value::Object(target)
}

fn browser_launch_headless(args: &Value, mode: Option<&str>, profile: Option<&str>) -> bool {
    if let Some(value) = bool_arg(args, "headless") {
        return value;
    }
    if let Some(mode) = mode {
        return launch_value_has_token(Some(mode), "headless");
    }
    if let Some(profile) = profile {
        if launch_value_has_token(Some(profile), "headless") {
            return true;
        }
        if launch_value_has_token(Some(profile), "visible")
            || launch_value_has_token(Some(profile), "normal")
        {
            return false;
        }
    }
    true
}

fn launch_value_has_token(value: Option<&str>, token: &str) -> bool {
    value
        .map(|value| {
            value
                .split(|character: char| {
                    character == '-'
                        || character == '_'
                        || character == ','
                        || character.is_whitespace()
                })
                .any(|part| part.eq_ignore_ascii_case(token))
        })
        .unwrap_or(false)
}

fn browser_launch_args(
    port: u16,
    user_data_dir: &Path,
    incognito: bool,
    headless: bool,
) -> Vec<String> {
    let mut args = vec![
        format!("--remote-debugging-port={port}"),
        format!("--user-data-dir={}", user_data_dir.display()),
        "--no-first-run".to_string(),
        "--no-default-browser-check".to_string(),
    ];
    if headless {
        args.push("--headless=new".to_string());
        args.push("--disable-gpu".to_string());
    }
    if incognito {
        args.push("--incognito".to_string());
    }
    args
}

fn has_target(args: &Value) -> bool {
    browser_target(args)
        .as_object()
        .is_some_and(|target| !target.is_empty())
}

#[cfg(test)]
fn browser_extract_target(args: &Value) -> Value {
    if let Some(target) = args.get("target").and_then(Value::as_object) {
        if target.get("page").and_then(Value::as_bool).unwrap_or(false) {
            return json!({ "page": true });
        }
        return Value::Object(target.clone());
    }
    browser_target(args)
}

fn require_unsafe(args: &Value, action: &str) -> SootieResult<()> {
    if bool_arg(args, "unsafe").unwrap_or(false) {
        Ok(())
    } else {
        Err(SootieError::Platform(format!(
            "POLICY_BLOCKED: {action} requires unsafe=true"
        )))
    }
}

fn require_raw_cdp(args: &Value, method: &str) -> SootieResult<()> {
    require_unsafe(args, "raw CDP")?;
    if high_risk_raw_cdp_method(method)
        && std::env::var("SOOTIE_ENABLE_UNSAFE_RAW_CDP")
            .ok()
            .as_deref()
            != Some("1")
    {
        return Err(SootieError::Platform(format!(
            "POLICY_BLOCKED: raw CDP method {method} requires SOOTIE_ENABLE_UNSAFE_RAW_CDP=1"
        )));
    }
    Ok(())
}

fn high_risk_raw_cdp_method(method: &str) -> bool {
    matches!(
        method,
        "Runtime.evaluate"
            | "Fetch.enable"
            | "Network.getResponseBody"
            | "Browser.grantPermissions"
            | "Page.setDownloadBehavior"
            | "Browser.setDownloadBehavior"
            | "Storage.clearDataForOrigin"
    )
}

fn raw_cdp_method(args: &Value) -> SootieResult<String> {
    let method = required_str(args, "method")?;
    if method.contains('.') {
        return Ok(method);
    }
    let domain = required_str(args, "domain")?;
    Ok(format!("{domain}.{method}"))
}

fn cookie_url_arg(args: &Value, fallback: &str) -> String {
    str_arg(args, "url").unwrap_or_else(|| fallback.to_string())
}

fn insert_arg(
    params: &mut serde_json::Map<String, Value>,
    args: &Value,
    arg_name: &str,
    param_name: &str,
) {
    if let Some(value) = str_arg(args, arg_name) {
        params.insert(param_name.to_string(), json!(value));
    }
}

fn include_bool(args: &Value, key: &str) -> Option<bool> {
    args.get("include")
        .and_then(Value::as_object)
        .and_then(|include| include.get(key))
        .and_then(|value| match value {
            Value::Bool(value) => Some(*value),
            Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
            Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
            _ => None,
        })
}

fn filter_elements_to_viewport(payload: &mut Value) {
    let viewport = &payload["page"]["viewport"];
    let width = viewport.get("width").and_then(Value::as_f64).unwrap_or(0.0);
    let height = viewport
        .get("height")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    if let Some(elements) = payload.get_mut("elements").and_then(Value::as_array_mut) {
        elements.retain(|element| {
            let Some(bounds) = element.get("bounds") else {
                return false;
            };
            let x = bounds.get("x").and_then(Value::as_f64).unwrap_or(0.0);
            let y = bounds.get("y").and_then(Value::as_f64).unwrap_or(0.0);
            let element_width = bounds.get("width").and_then(Value::as_f64).unwrap_or(0.0);
            let element_height = bounds.get("height").and_then(Value::as_f64).unwrap_or(0.0);
            x + element_width > 0.0 && y + element_height > 0.0 && x < width && y < height
        });
    }
}

fn browser_scroll_amount(args: &Value) -> i32 {
    match args.get("amount") {
        Some(Value::String(value)) if value.eq_ignore_ascii_case("small") => 1,
        Some(Value::String(value)) if value.eq_ignore_ascii_case("medium") => 2,
        Some(Value::String(value)) if value.eq_ignore_ascii_case("large") => 4,
        _ => i32_arg(args, "amount").unwrap_or(1).max(1),
    }
}

fn timeout_arg(args: &Value, default_ms: u64) -> SootieResult<Duration> {
    let ms = u32_arg(args, "timeout_ms").unwrap_or(default_ms as u32) as u64;
    if ms == 0 {
        return Err(SootieError::InvalidArguments(
            "timeout_ms must be a positive integer".into(),
        ));
    }
    Ok(Duration::from_millis(ms.min(120_000)))
}

fn interval_arg(args: &Value, default_ms: u64) -> SootieResult<Duration> {
    let ms = u32_arg(args, "interval_ms").unwrap_or(default_ms as u32) as u64;
    if ms == 0 {
        return Err(SootieError::InvalidArguments(
            "interval_ms must be a positive integer".into(),
        ));
    }
    Ok(Duration::from_millis(ms.min(30_000)))
}

fn required_str(args: &Value, key: &str) -> SootieResult<String> {
    str_arg(args, key).ok_or_else(|| SootieError::InvalidArguments(format!("{key} is required")))
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    match args.get(key)? {
        Value::Bool(value) => Some(*value),
        Value::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
        Value::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn u32_arg(args: &Value, key: &str) -> Option<u32> {
    match args.get(key)? {
        Value::Number(value) => value.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn i32_arg(args: &Value, key: &str) -> Option<i32> {
    match args.get(key)? {
        Value::Number(value) => value.as_i64().and_then(|value| i32::try_from(value).ok()),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn f64_arg(args: &Value, key: &str) -> Option<f64> {
    let value = match args.get(key)? {
        Value::Number(value) => value.as_f64()?,
        Value::String(value) => value.parse().ok()?,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn string_array_arg(args: &Value, key: &str) -> Vec<String> {
    match args.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn port_from_ws_url(url: &str) -> Option<u16> {
    let rest = url.strip_prefix("ws://")?;
    let (host_port, _) = rest.split_once('/')?;
    let (_, port) = host_port.rsplit_once(':')?;
    port.parse().ok()
}

fn browser_observe_expression(max_elements: u32, max_text_chars: u32) -> String {
    format!(
        "(() => {{\nconst maxElements = {max_elements};\nconst maxTextChars = {max_text_chars};\n{}\n{}\nreturn observeBrowserPage(maxElements, maxTextChars);\n}})()",
        BROWSER_DOM_HELPERS, BROWSER_OBSERVE_BODY
    )
}

fn browser_visible_text_expression(max_text_chars: u32) -> String {
    format!(
        "(() => {{\nconst maxTextChars = {max_text_chars};\n{}\nreturn visibleBrowserText(maxTextChars).content;\n}})()",
        BROWSER_OBSERVE_BODY
    )
}

const BROWSER_DOM_HELPERS: &str = r#"
function cssEscape(value) {
  if (window.CSS && CSS.escape) return CSS.escape(String(value));
  return String(value).replace(/["\\#.;:[\]()>+~*^$|=,\s]/g, '\\$&');
}
function isVisible(el) {
  if (!el || el.nodeType !== 1) return false;
  const style = window.getComputedStyle(el);
  if (style.display === 'none' || style.visibility === 'hidden') return false;
  const rect = el.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}
function roleOf(el) {
  const tag = el.tagName.toLowerCase();
  const type = (el.getAttribute('type') || '').toLowerCase();
  return el.getAttribute('role')
    || (tag === 'a' ? 'link' : null)
    || (tag === 'button' ? 'button' : null)
    || (tag === 'textarea' ? 'textbox' : null)
    || (tag === 'select' ? 'combobox' : null)
    || (tag === 'input' ? (type === 'password' ? 'textbox' : (type || 'input')) : null)
    || tag;
}
function labelOf(el) {
  const labelledBy = el.getAttribute('aria-labelledby');
  const labelledText = labelledBy ? labelledBy.split(/\s+/).map((id) => {
    const node = document.getElementById(id);
    return node ? node.innerText || node.textContent || '' : '';
  }).join(' ') : '';
  return String(
    el.getAttribute('aria-label')
    || labelledText
    || el.getAttribute('alt')
    || el.getAttribute('title')
    || el.getAttribute('placeholder')
    || el.getAttribute('name')
    || el.innerText
    || el.value
    || el.id
    || ''
  ).trim();
}
function elementText(el) {
  return String(el.innerText || el.textContent || el.value || '').trim();
}
function elementSearchText(el) {
  return [
    roleOf(el),
    labelOf(el),
    elementText(el),
    el.getAttribute('title') || '',
    el.getAttribute('data-testid') || '',
    el.id || '',
    el.className || ''
  ].join(' ').toLowerCase();
}
function interactiveElements() {
  const selector = 'a,button,input,textarea,select,summary,[role],[onclick],[tabindex],[contenteditable="true"],[draggable="true"]';
  return Array.from(document.querySelectorAll(selector)).filter(isVisible);
}
function selectorCandidates(el) {
  const out = [];
  const tag = el.tagName.toLowerCase();
  if (el.id) out.push('#' + cssEscape(el.id));
  const testid = el.getAttribute('data-testid');
  if (testid) out.push('[data-testid="' + String(testid).replace(/"/g, '\\"') + '"]');
  const name = el.getAttribute('name');
  if (name) out.push(tag + '[name="' + String(name).replace(/"/g, '\\"') + '"]');
  if (el.classList && el.classList.length) {
    out.push(tag + '.' + Array.from(el.classList).slice(0, 3).map(cssEscape).join('.'));
  }
  out.push(tag);
  return Array.from(new Set(out));
}
function browserElementPayload(el, index) {
  const rect = el.getBoundingClientRect();
  const role = roleOf(el);
  const editable = el.isContentEditable || el.matches('input,textarea,select');
  const checked = el.matches('input[type="checkbox"],input[type="radio"]') ? Boolean(el.checked) : undefined;
  return {
    ref: 'b_' + index,
    role,
    name: labelOf(el),
    text: elementText(el).slice(0, 500),
    value: ('value' in el ? String(el.value || '') : undefined),
    tag: el.tagName.toLowerCase(),
    dom_id: el.id || undefined,
    dom_class: Array.from(el.classList || []),
    state: {
      visible: isVisible(el),
      enabled: !(el.disabled || el.getAttribute('aria-disabled') === 'true'),
      editable,
      checked,
      selected: Boolean(el.selected || el.getAttribute('aria-selected') === 'true'),
      expanded: el.getAttribute('aria-expanded') === null ? undefined : el.getAttribute('aria-expanded') === 'true'
    },
    bounds: { x: rect.left, y: rect.top, width: rect.width, height: rect.height },
    selector_candidates: selectorCandidates(el)
  };
}
function matchesBrowserTarget(el, target) {
  if (!target) return true;
  if (target.selector) return false;
  const haystack = elementSearchText(el);
  const role = roleOf(el).toLowerCase();
  if (target.query && !haystack.includes(String(target.query).toLowerCase())) return false;
  if (target.text && !elementText(el).toLowerCase().includes(String(target.text).toLowerCase())) return false;
  if (target.name && !labelOf(el).toLowerCase().includes(String(target.name).toLowerCase())) return false;
  if (target.role && !role.includes(String(target.role).toLowerCase())) return false;
  if (target.dom_id && el.id !== target.dom_id) return false;
  if (target.dom_class && !Array.from(el.classList || []).includes(target.dom_class)) return false;
  return true;
}
function resolveBrowserTarget(target) {
  if (target && target.focused && document.activeElement) return document.activeElement;
  if (target && target.selector) return document.querySelector(target.selector);
  if (target && target.dom_id) return document.getElementById(target.dom_id);
  if (target && Number.isFinite(Number(target.x)) && Number.isFinite(Number(target.y))) {
    return document.elementFromPoint(Number(target.x), Number(target.y));
  }
  const elements = interactiveElements();
  if (target && target.ref) {
    const match = String(target.ref).match(/^b_(\d+)$/);
    if (match) return elements[Number(match[1])] || null;
  }
  return elements.find((el) => matchesBrowserTarget(el, target)) || null;
}
function findBrowserElements(target, maxResults) {
  const elements = interactiveElements();
  if (target && target.selector) {
    return Array.from(document.querySelectorAll(target.selector))
      .filter(isVisible)
      .slice(0, maxResults)
      .map((el) => browserElementPayload(el, Math.max(0, elements.indexOf(el))));
  }
  return elements
    .map((el, index) => ({ el, index }))
    .filter(({ el }) => matchesBrowserTarget(el, target))
    .slice(0, maxResults)
    .map(({ el, index }) => browserElementPayload(el, index));
}
"#;

const BROWSER_OBSERVE_BODY: &str = r#"
function visibleBrowserText(maxTextChars) {
  const root = document.body || document.documentElement;
  const text = String(root ? (root.innerText || root.textContent || '') : '').replace(/\r/g, '\n');
  const normalized = text.split(/\n+/).map((line) => line.trim()).filter(Boolean).join('\n');
  return {
    content: normalized.slice(0, maxTextChars),
    truncated: normalized.length > maxTextChars
  };
}
function observeBrowserPage(maxElements, maxTextChars) {
  const text = visibleBrowserText(maxTextChars);
  return {
    page: {
      url: location.href,
      title: document.title,
      loading: document.readyState !== 'complete',
      viewport: {
        width: window.innerWidth,
        height: window.innerHeight,
        scroll_x: window.scrollX,
        scroll_y: window.scrollY
      }
    },
    elements: interactiveElements().slice(0, maxElements).map(browserElementPayload),
    text: {
      visible_text: text.content,
      truncated: text.truncated
    },
    diagnostics: {
      network_busy: document.readyState !== 'complete',
      console_error_count: 0,
      frame_count: window.frames.length
    }
  };
}
"#;

const BROWSER_CLICK_BODY: &str = r#"
const el = resolveBrowserTarget(target);
if (!el) return { ok: false, reason: 'target-not-found' };
const elements = interactiveElements();
const index = Math.max(0, elements.indexOf(el));
el.scrollIntoView({ block: 'center', inline: 'center' });
const rect = el.getBoundingClientRect();
const clientX = rect.left + rect.width / 2;
const clientY = rect.top + rect.height / 2;
const button = buttonName === 'right' ? 2 : (buttonName === 'middle' ? 1 : 0);
for (let i = 0; i < clickCount; i++) {
  el.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, cancelable: true, view: window, clientX, clientY, button, buttons: 1 }));
  el.dispatchEvent(new MouseEvent('mouseup', { bubbles: true, cancelable: true, view: window, clientX, clientY, button, buttons: 0 }));
  el.click();
}
return { ok: true, target: browserElementPayload(el, index), clientX, clientY, button: buttonName, count: clickCount };
"#;

const BROWSER_TYPE_BODY: &str = r#"
const el = resolveBrowserTarget(target);
if (!el) return { ok: false, reason: 'target-not-found' };
const elements = interactiveElements();
const index = Math.max(0, elements.indexOf(el));
el.scrollIntoView({ block: 'center', inline: 'center' });
el.focus();
if ('value' in el) {
  el.value = clear ? text : String(el.value || '') + text;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
} else if (el.isContentEditable) {
  el.textContent = clear ? text : String(el.textContent || '') + text;
  el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
} else {
  return { ok: false, reason: 'target-not-editable' };
}
if (submit) {
  const form = el.form || el.closest('form');
  if (form) form.requestSubmit ? form.requestSubmit() : form.submit();
  else el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true }));
}
return { ok: true, target: browserElementPayload(el, index), bytes: text.length, clear, submit };
"#;

const BROWSER_SCROLL_TARGET_BODY: &str = r#"
const el = resolveBrowserTarget(target);
if (!el) return { ok: false, reason: 'target-not-found' };
const elements = interactiveElements();
const index = Math.max(0, elements.indexOf(el));
const pixels = Math.max(1, Number(amount) || 1) * 400;
let left = 0;
let top = 0;
if (direction === 'left') left = -pixels;
else if (direction === 'right') left = pixels;
else if (direction === 'up') top = -pixels;
else top = pixels;
el.scrollBy ? el.scrollBy({ left, top, behavior: 'instant' }) : window.scrollBy({ left, top, behavior: 'instant' });
return { ok: true, target: browserElementPayload(el, index), direction, amount, delta_x: left, delta_y: top };
"#;

const BROWSER_EXTRACT_BODY: &str = r#"
function normalize(value) {
  return String(value || '').replace(/\r/g, '\n').split(/\n+/).map((line) => line.trim()).filter(Boolean).join('\n');
}
function extractBrowserContent(format, maxChars, target) {
  const hasTarget = target && !target.page && Object.keys(target).length > 0;
  const targetRoot = hasTarget ? resolveBrowserTarget(target) : null;
  if (hasTarget && !targetRoot) return { ok: false, reason: 'target-not-found' };
  const root = targetRoot || document.body || document.documentElement;
  let content = '';
  let outFormat = format;
  if (format === 'html') {
    content = root ? root.outerHTML : '';
  } else if (format === 'json') {
    content = JSON.stringify({
      url: location.href,
      title: document.title,
      text: normalize(root ? (root.innerText || root.textContent || '') : '')
    });
  } else {
    outFormat = format === 'markdown' ? 'markdown' : 'text';
    content = normalize(root ? (root.innerText || root.textContent || '') : '');
  }
  const truncated = content.length > maxChars;
  const elements = interactiveElements();
  const index = targetRoot ? Math.max(0, elements.indexOf(targetRoot)) : -1;
  return {
    ok: true,
    format: outFormat,
    content: content.slice(0, maxChars),
    truncated,
    target: targetRoot ? browserElementPayload(targetRoot, index) : undefined
  };
}
"#;

const BROWSER_NETWORK_BODY: &str = r#"
function browserNetworkEntries(maxEntries, sinceMs, urlContains, resourceType) {
  const now = performance.now();
  return performance.getEntriesByType('resource')
    .filter((entry) => !sinceMs || now - entry.startTime <= sinceMs)
    .filter((entry) => !urlContains || String(entry.name || '').includes(urlContains))
    .filter((entry) => !resourceType || String(entry.initiatorType || '').toLowerCase() === String(resourceType).toLowerCase())
    .slice(-maxEntries)
    .map((entry, index) => ({
      request_id: String(index),
      url: entry.name,
      method: undefined,
      status: undefined,
      resource_type: entry.initiatorType || 'other',
      timing: {
        start_time: entry.startTime,
        duration: entry.duration,
        response_end: entry.responseEnd,
        transfer_size: entry.transferSize || 0,
        encoded_body_size: entry.encodedBodySize || 0,
        decoded_body_size: entry.decodedBodySize || 0
      }
    }));
}
"#;

const BROWSER_CONSOLE_BODY: &str = r#"
function installSootieConsoleHook() {
  if (window.__sootieConsoleHookInstalled) return;
  window.__sootieConsoleHookInstalled = true;
  window.__sootieConsoleEvents = window.__sootieConsoleEvents || [];
  for (const level of ['log', 'info', 'warn', 'warning', 'error', 'debug']) {
    const original = console[level === 'warning' ? 'warn' : level];
    if (typeof original !== 'function') continue;
    console[level === 'warning' ? 'warn' : level] = function(...args) {
      window.__sootieConsoleEvents.push({
        level: level === 'warn' ? 'warning' : level,
        text: args.map((arg) => {
          try { return typeof arg === 'string' ? arg : JSON.stringify(arg); }
          catch (_) { return String(arg); }
        }).join(' '),
        timestamp: Date.now()
      });
      return original.apply(this, args);
    };
  }
}
function browserConsoleEntries(maxEntries, sinceMs, level) {
  installSootieConsoleHook();
  const entries = window.__sootieConsoleEvents || [];
  const now = Date.now();
  return entries
    .filter((entry) => !sinceMs || now - entry.timestamp <= sinceMs)
    .filter((entry) => !level || entry.level === level)
    .slice(-maxEntries);
}
"#;

const BROWSER_STORAGE_BODY: &str = r#"
function browserStorageAction(areaName, action, key, value, origin) {
  if (origin && origin !== location.origin) {
    return { ok: false, reason: 'origin must match current page origin' };
  }
  const store = areaName === 'sessionStorage' ? sessionStorage : localStorage;
  if (action === 'list') {
    const entries = {};
    for (let i = 0; i < store.length; i++) {
      const itemKey = store.key(i);
      entries[itemKey] = store.getItem(itemKey);
    }
    return { ok: true, area: areaName, entries };
  }
  if (action === 'get') return { ok: true, area: areaName, key, value: store.getItem(key) };
  if (action === 'set') {
    store.setItem(key, value);
    return { ok: true, area: areaName, key, value: store.getItem(key) };
  }
  if (action === 'remove') {
    store.removeItem(key);
    return { ok: true, area: areaName, key, removed: true };
  }
  if (action === 'clear') {
    store.clear();
    return { ok: true, area: areaName, cleared: true };
  }
  return { ok: false, reason: 'action must be list/get/set/remove/clear' };
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_extract_target_prefers_nested_target() {
        let target = browser_extract_target(&json!({
            "selector": "#outer",
            "target": { "ref": "b_2" }
        }));
        assert_eq!(target, json!({ "ref": "b_2" }));
    }

    #[test]
    fn browser_extract_target_can_force_full_page() {
        let target = browser_extract_target(&json!({
            "selector": "#outer",
            "target": { "page": true }
        }));
        assert_eq!(target, json!({ "page": true }));
    }

    #[test]
    fn browser_launch_defaults_to_headless() {
        assert!(browser_launch_headless(&json!({}), None, None));

        let args = browser_launch_args(9222, Path::new("/tmp/sootie-headless"), false, true);
        assert!(args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--disable-gpu".to_string()));
        assert!(!args.contains(&"--incognito".to_string()));
    }

    #[test]
    fn browser_launch_can_request_visible_mode() {
        assert!(!browser_launch_headless(&json!({}), Some("normal"), None));
        assert!(!browser_launch_headless(
            &json!({ "headless": false }),
            None,
            Some("incognito"),
        ));

        let args = browser_launch_args(9222, Path::new("/tmp/sootie-visible"), true, false);
        assert!(!args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--incognito".to_string()));
    }

    #[test]
    fn browser_launch_accepts_headless_incognito_mode() {
        assert!(browser_launch_headless(
            &json!({}),
            Some("headless-incognito"),
            None,
        ));
        assert!(launch_value_has_token(
            Some("headless-incognito"),
            "incognito"
        ));

        let args = browser_launch_args(9222, Path::new("/tmp/sootie-private"), true, true);
        assert!(args.contains(&"--headless=new".to_string()));
        assert!(args.contains(&"--incognito".to_string()));
    }

    #[test]
    fn observe_include_flags_accept_booleans_and_strings() {
        let args = json!({
            "include": {
                "elements": false,
                "text": "true"
            }
        });
        assert_eq!(include_bool(&args, "elements"), Some(false));
        assert_eq!(include_bool(&args, "text"), Some(true));
        assert_eq!(include_bool(&args, "screenshot"), None);
    }

    #[test]
    fn viewport_filter_keeps_only_intersecting_elements() {
        let mut payload = json!({
            "page": { "viewport": { "width": 100, "height": 100 } },
            "elements": [
                { "bounds": { "x": 10, "y": 10, "width": 20, "height": 20 } },
                { "bounds": { "x": 120, "y": 10, "width": 20, "height": 20 } },
                { "bounds": { "x": -10, "y": -10, "width": 20, "height": 20 } }
            ]
        });
        filter_elements_to_viewport(&mut payload);
        assert_eq!(payload["elements"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn browser_scroll_amount_accepts_named_and_numeric_values() {
        assert_eq!(browser_scroll_amount(&json!({ "amount": "small" })), 1);
        assert_eq!(browser_scroll_amount(&json!({ "amount": "medium" })), 2);
        assert_eq!(browser_scroll_amount(&json!({ "amount": "large" })), 4);
        assert_eq!(browser_scroll_amount(&json!({ "amount": 3 })), 3);
    }

    #[test]
    fn diagnostic_scripts_honor_public_filter_arguments() {
        assert!(BROWSER_NETWORK_BODY.contains("sinceMs"));
        assert!(BROWSER_NETWORK_BODY.contains("entry.startTime"));
        assert!(BROWSER_CONSOLE_BODY.contains("sinceMs"));
        assert!(BROWSER_CONSOLE_BODY.contains("entry.timestamp"));
        assert!(BROWSER_STORAGE_BODY.contains("origin !== location.origin"));
    }

    #[test]
    fn element_registry_reuses_stable_refs_for_same_element() {
        let mut registry = BrowserElementRegistry::default();
        let mut first = json!([
            {
                "ref": "b_0",
                "role": "button",
                "name": "Submit",
                "text": "Submit",
                "dom_id": "submit",
                "selector_candidates": ["#submit"],
                "bounds": { "x": 10, "y": 20, "width": 80, "height": 30 }
            }
        ]);
        registry.remember_elements("page-1", &mut first);
        let stable_ref = first[0]["ref"].as_str().unwrap().to_string();
        assert!(stable_ref.starts_with("br_"));
        assert_eq!(first[0]["volatile_ref"], "b_0");

        let mut second = json!([
            {
                "ref": "b_1",
                "role": "button",
                "name": "Submit",
                "text": "Submit",
                "dom_id": "submit",
                "selector_candidates": ["#submit"],
                "bounds": { "x": 10, "y": 20, "width": 80, "height": 30 }
            }
        ]);
        registry.remember_elements("page-1", &mut second);
        assert_eq!(second[0]["ref"], stable_ref);
        assert_eq!(second[0]["volatile_ref"], "b_1");
    }

    #[test]
    fn element_registry_resolves_stable_ref_to_browser_target() {
        let mut registry = BrowserElementRegistry::default();
        let mut elements = json!([
            {
                "ref": "b_3",
                "role": "button",
                "name": "Save",
                "text": "Save",
                "selector_candidates": ["button.save", "button"],
                "bounds": { "x": 50, "y": 60, "width": 70, "height": 20 }
            }
        ]);
        registry.remember_elements("page-1", &mut elements);
        let stable_ref = elements[0]["ref"].as_str().unwrap();
        let target = registry
            .resolve_target("page-1", &json!({ "ref": stable_ref }))
            .unwrap();
        assert_eq!(target["selector"], "button.save");
        assert_eq!(target["ref"], stable_ref);
    }

    #[test]
    fn extract_target_resolves_nested_stable_ref() {
        let mut service = BrowserService::default();
        let mut elements = json!([
            {
                "ref": "b_4",
                "role": "heading",
                "name": "Result",
                "text": "Result",
                "selector_candidates": ["#result"],
                "bounds": { "x": 0, "y": 0, "width": 40, "height": 20 }
            }
        ]);
        service.registry.remember_elements("page-1", &mut elements);
        let stable_ref = elements[0]["ref"].as_str().unwrap();
        let target = service.extract_target("page-1", &json!({ "target": { "ref": stable_ref }}));
        assert_eq!(target["selector"], "#result");
    }

    #[test]
    fn element_registry_does_not_resolve_refs_to_generic_tag_selectors() {
        let mut registry = BrowserElementRegistry::default();
        let mut elements = json!([
            {
                "ref": "b_5",
                "role": "button",
                "name": "Delete",
                "text": "Delete",
                "selector_candidates": ["button"],
                "bounds": { "x": 20, "y": 30, "width": 40, "height": 20 }
            }
        ]);
        registry.remember_elements("page-1", &mut elements);
        let stable_ref = elements[0]["ref"].as_str().unwrap();
        let target = registry
            .resolve_target("page-1", &json!({ "ref": stable_ref }))
            .unwrap();
        assert!(target.get("selector").is_none());
        assert_eq!(target["role"], "button");
        assert_eq!(target["name"], "Delete");
        assert_eq!(target["text"], "Delete");
        assert_eq!(target["x"], 40.0);
        assert_eq!(target["y"], 40.0);
    }
}
