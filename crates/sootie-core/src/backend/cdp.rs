use std::env;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::{json, Value};

use base64::Engine;

use crate::types::{ActionResult, Bounds, ElementInfo, FindQuery, Screenshot};

use super::png_dimensions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PageInfo {
    pub target_id: String,
    pub url: String,
    pub title: Option<String>,
    pub page_type: String,
    pub web_socket_debugger_url: Option<String>,
}

struct WsEndpoint {
    host: String,
    port: u16,
    path: String,
}

struct KeyDescriptor {
    key: String,
    code: String,
    windows_virtual_key_code: u32,
}

#[allow(dead_code)]
pub(crate) fn parse_remote_debugging_port(cmdline: &str) -> Option<u16> {
    let args = cmdline.split_whitespace().collect::<Vec<_>>();
    for (index, arg) in args.iter().enumerate() {
        if let Some(port) = arg.strip_prefix("--remote-debugging-port=") {
            return port.parse::<u16>().ok();
        }
        if *arg == "--remote-debugging-port" {
            return args.get(index + 1).and_then(|port| port.parse().ok());
        }
    }
    None
}

#[allow(dead_code)]
pub(crate) fn current_page_url(port: u16) -> Option<String> {
    pages_json(port).and_then(|payload| parse_page_url(&payload))
}

#[allow(dead_code)]
pub(crate) fn configured_port() -> Option<u16> {
    env::var("SOOTIE_CDP_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .or_else(|| {
            env::var("SOOTIE_CDP_WS_URL")
                .ok()
                .and_then(|url| parse_ws_endpoint(&url).map(|endpoint| endpoint.port))
        })
}

#[allow(dead_code)]
pub(crate) fn current_page(port: u16) -> Option<PageInfo> {
    let payload = pages_json(port)?;
    let page = parse_current_page(&payload);
    match &page {
        Some(page) => tracing::debug!(
            port,
            url = %page.url,
            title = page.title.as_deref().unwrap_or(""),
            ws_url = page.web_socket_debugger_url.as_deref().unwrap_or(""),
            "selected CDP page"
        ),
        None => tracing::debug!(port, payload, "no reportable CDP page found"),
    }
    page
}

#[allow(dead_code)]
pub(crate) fn browser_pages(port: u16) -> Option<Vec<PageInfo>> {
    pages_json(port).and_then(|payload| parse_pages(&payload))
}

#[allow(dead_code)]
pub(crate) fn page_by_id(port: u16, page_id: Option<&str>) -> Option<PageInfo> {
    let pages = browser_pages(port)?;
    if let Some(page_id) = page_id.filter(|value| !value.trim().is_empty()) {
        return pages
            .into_iter()
            .find(|page| page.target_id == page_id || page_id == page.url);
    }
    pages.into_iter().find(is_reportable_page)
}

#[allow(dead_code)]
pub(crate) fn page_elements(port: u16) -> Option<Vec<ElementInfo>> {
    let page = current_page(port)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    evaluate_json(&ws_url, DOM_ELEMENTS_EXPRESSION).map(|value| parse_dom_elements(&value))
}

#[allow(dead_code)]
pub(crate) fn page_text(port: u16, query: Option<&str>, depth: Option<u32>) -> Option<String> {
    let page = current_page(port)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let query = serde_json::to_string(&query.filter(|query| !query.trim().is_empty())).ok()?;
    let depth = depth
        .map(|depth| depth.min(64).to_string())
        .unwrap_or_else(|| "null".to_string());
    let expression = format!(
        "(() => {{\nconst query = {query};\nconst maxDepth = {depth};\n{READ_TEXT_BODY}\n}})()"
    );
    evaluate_json(&ws_url, &expression)
        .and_then(|value| value.as_str().map(str::to_string))
        .map(normalize_read_text)
}

#[allow(dead_code)]
pub(crate) fn page_screenshot(port: u16) -> Option<Screenshot> {
    page_screenshot_by_id(port, None, false)
}

#[allow(dead_code)]
pub(crate) fn page_screenshot_by_id(
    port: u16,
    page_id: Option<&str>,
    full_page: bool,
) -> Option<Screenshot> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let params = if full_page {
        json!({ "format": "png", "fromSurface": true, "captureBeyondViewport": true })
    } else {
        json!({ "format": "png", "fromSurface": true })
    };
    let result = cdp_command(&ws_url, "Page.captureScreenshot", params)?;
    let data_base64 = result.get("data").and_then(Value::as_str)?.to_string();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data_base64)
        .ok()?;
    let (width, height) = png_dimensions(&bytes);
    Some(Screenshot {
        mime_type: "image/png".to_string(),
        data_base64,
        width,
        height,
        window_title: page.title,
        window_frame: None,
    })
}

#[allow(dead_code)]
pub(crate) fn click_element(port: u16, query: &FindQuery) -> Option<ActionResult> {
    let value = evaluate_dom_action(port, query, CLICK_ELEMENT_BODY)?;
    action_result("cdp-click", value, query)
}

#[allow(dead_code)]
pub(crate) fn type_text_element(
    port: u16,
    query: &FindQuery,
    text: &str,
    clear: bool,
) -> Option<ActionResult> {
    let body = format!(
        "const text = {};\nconst clear = {};\n{}",
        serde_json::to_string(text).ok()?,
        if clear { "true" } else { "false" },
        TYPE_TEXT_ELEMENT_BODY
    );
    let value = evaluate_dom_action(port, query, &body)?;
    let mut result = action_result("cdp-type", value, query)?;
    result.details["bytes"] = json!(text.len());
    result.details["clear"] = json!(clear);
    Some(result)
}

#[allow(dead_code)]
pub(crate) fn hover_element(port: u16, query: &FindQuery) -> Option<ActionResult> {
    let value = evaluate_dom_action(port, query, HOVER_ELEMENT_BODY)?;
    action_result("cdp-hover", value, query)
}

#[allow(dead_code)]
pub(crate) fn long_press_element(
    port: u16,
    query: &FindQuery,
    duration_secs: f64,
    button: &str,
) -> Option<ActionResult> {
    let duration_ms = (duration_secs.max(0.0) * 1000.0).round() as u64;
    let body = format!(
        "const durationMs = {};\nconst buttonName = {};\n{}",
        duration_ms,
        serde_json::to_string(button).ok()?,
        LONG_PRESS_ELEMENT_BODY
    );
    let timeout = Duration::from_millis(duration_ms.saturating_add(2_000).min(30_000));
    let value = evaluate_dom_action_await(port, query, &body, timeout)?;
    let mut result = action_result("cdp-long-press", value, query)?;
    result.details["duration"] = json!(duration_secs);
    result.details["button"] = json!(button);
    Some(result)
}

#[allow(dead_code)]
pub(crate) fn drag_element(
    port: u16,
    query: &FindQuery,
    to: (f64, f64),
    duration_secs: f64,
    hold_duration_secs: f64,
) -> Option<ActionResult> {
    let duration_ms = (duration_secs.max(0.0) * 1000.0).round() as u64;
    let hold_ms = (hold_duration_secs.max(0.0) * 1000.0).round() as u64;
    let body = format!(
        "const toX = {};\nconst toY = {};\nconst durationMs = {};\nconst holdMs = {};\n{}",
        finite_number(to.0)?,
        finite_number(to.1)?,
        duration_ms,
        hold_ms,
        DRAG_ELEMENT_BODY
    );
    let timeout = Duration::from_millis(
        duration_ms
            .saturating_add(hold_ms)
            .saturating_add(2_000)
            .min(30_000),
    );
    let value = evaluate_dom_action_await(port, query, &body, timeout)?;
    let mut result = action_result("cdp-drag", value, query)?;
    result.details["to"] = json!({ "x": to.0, "y": to.1 });
    result.details["duration"] = json!(duration_secs);
    result.details["hold_duration"] = json!(hold_duration_secs);
    Some(result)
}

#[allow(dead_code)]
pub(crate) fn press_key(port: u16, key: &str, modifiers: &[String]) -> Option<ActionResult> {
    press_key_on_page(port, None, key, modifiers)
}

#[allow(dead_code)]
pub(crate) fn press_key_on_page(
    port: u16,
    page_id: Option<&str>,
    key: &str,
    modifiers: &[String],
) -> Option<ActionResult> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let key = key_descriptor(key)?;
    let modifier_mask = cdp_modifier_mask(modifiers)?;
    for event_type in ["rawKeyDown", "keyUp"] {
        cdp_command(
            &ws_url,
            "Input.dispatchKeyEvent",
            json!({
                "type": event_type,
                "key": &key.key,
                "code": &key.code,
                "windowsVirtualKeyCode": key.windows_virtual_key_code,
                "nativeVirtualKeyCode": key.windows_virtual_key_code,
                "modifiers": modifier_mask
            }),
        )?;
    }
    Some(ActionResult {
        method: "cdp-key".to_string(),
        details: json!({
            "key": key.key,
            "code": key.code,
            "modifiers": modifiers,
            "modifier_mask": modifier_mask
        }),
    })
}

#[allow(dead_code)]
pub(crate) fn scroll_page(port: u16, direction: &str, amount: i32) -> Option<ActionResult> {
    scroll_page_by_id(port, None, direction, amount)
}

#[allow(dead_code)]
pub(crate) fn scroll_page_by_id(
    port: u16,
    page_id: Option<&str>,
    direction: &str,
    amount: i32,
) -> Option<ActionResult> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let direction = serde_json::to_string(direction).ok()?;
    let steps = amount.abs().max(1);
    let expression =
        format!("(() => {{\nconst direction = {direction};\nconst amount = {steps};\n{SCROLL_PAGE_BODY}\n}})()");
    let value = evaluate_json(&ws_url, &expression)?;
    (value.get("ok").and_then(Value::as_bool) == Some(true)).then_some(ActionResult {
        method: "cdp-scroll".to_string(),
        details: value,
    })
}

#[allow(dead_code)]
pub(crate) fn open_page(
    port: u16,
    url: &str,
    new_page: bool,
    timeout: Duration,
) -> Option<PageInfo> {
    if new_page {
        return new_browser_page(port, url, timeout).or_else(|| {
            let page = current_page(port)?;
            navigate_page(port, Some(&page.target_id), url, timeout)
        });
    }
    let page = current_page(port)?;
    navigate_page(port, Some(&page.target_id), url, timeout)
}

#[allow(dead_code)]
pub(crate) fn navigate_page(
    port: u16,
    page_id: Option<&str>,
    url: &str,
    timeout: Duration,
) -> Option<PageInfo> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    cdp_command_with_timeout(&ws_url, "Page.navigate", json!({ "url": url }), timeout)?;
    wait_for_page_ready(port, Some(&page.target_id), "domcontentloaded", timeout);
    page_by_id(port, Some(&page.target_id)).or(Some(PageInfo {
        url: url.to_string(),
        ..page
    }))
}

#[allow(dead_code)]
pub(crate) fn close_page(port: u16, page_id: &str) -> Option<()> {
    http_text(port, "GET", &format!("/json/close/{page_id}")).map(|_| ())
}

#[allow(dead_code)]
pub(crate) fn browser_history_action(
    port: u16,
    page_id: Option<&str>,
    direction: &str,
    timeout: Duration,
) -> Option<PageInfo> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let expression = match direction {
        "back" => "history.back(); true",
        "forward" => "history.forward(); true",
        "reload" => "location.reload(); true",
        _ => return None,
    };
    evaluate_json_with_options(&ws_url, expression, false, timeout)?;
    wait_for_page_ready(port, Some(&page.target_id), "domcontentloaded", timeout);
    page_by_id(port, Some(&page.target_id)).or(Some(page))
}

#[allow(dead_code)]
pub(crate) fn evaluate_on_page(
    port: u16,
    page_id: Option<&str>,
    expression: &str,
    await_promise: bool,
    timeout: Duration,
) -> Option<Value> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    evaluate_json_with_options(&ws_url, expression, await_promise, timeout)
}

#[allow(dead_code)]
pub(crate) fn send_cdp_command(
    port: u16,
    page_id: Option<&str>,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Option<Value> {
    let ws_url = if method.starts_with("Browser.") {
        browser_websocket_url(port).or_else(|| {
            page_by_id(port, page_id)
                .and_then(|page| page.web_socket_debugger_url)
                .map(|url| ws_url_with_fallback_port(&url, port))
        })?
    } else {
        let page = page_by_id(port, page_id)?;
        ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port)
    };
    cdp_command_with_timeout(&ws_url, method, params, timeout)
}

#[allow(dead_code)]
pub(crate) fn collect_cdp_events(
    port: u16,
    page_id: Option<&str>,
    domain: &str,
    event: Option<&str>,
    timeout: Duration,
    max_events: u32,
) -> Option<Vec<Value>> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let endpoint = parse_ws_endpoint(&ws_url)?;
    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port)).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    websocket_handshake(&mut stream, &endpoint)?;
    let enable = json!({ "id": 1, "method": format!("{domain}.enable"), "params": {} });
    send_ws_text(&mut stream, &enable.to_string())?;
    let started = std::time::Instant::now();
    let mut events = Vec::new();
    while started.elapsed() < timeout && events.len() < max_events as usize {
        let Some(message) = read_ws_text(&mut stream) else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(&message) else {
            continue;
        };
        if payload.get("id").is_some() {
            continue;
        }
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
        if !method.starts_with(&format!("{domain}.")) {
            continue;
        }
        if let Some(event) = event {
            if method != format!("{domain}.{event}") {
                continue;
            }
        }
        events.push(payload);
    }
    Some(events)
}

#[allow(dead_code)]
pub(crate) fn set_file_input_files(
    port: u16,
    page_id: Option<&str>,
    target: &Value,
    file_paths: &[String],
    timeout: Duration,
) -> Option<Value> {
    let page = page_by_id(port, page_id)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let selector = target
        .get("selector")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            target
                .get("dom_id")
                .and_then(Value::as_str)
                .map(|id| format!("#{id}"))
        })?;
    let expression = format!(
        "document.querySelector({})",
        serde_json::to_string(&selector).ok()?
    );
    let mut stream = open_cdp_stream(&ws_url, timeout)?;
    let evaluate = cdp_command_on_stream(
        &mut stream,
        &ws_url,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": false }),
        1,
    )?;
    let object_id = evaluate
        .get("result")
        .and_then(|result| result.get("objectId"))
        .and_then(Value::as_str)?;
    let described = cdp_command_on_stream(
        &mut stream,
        &ws_url,
        "DOM.describeNode",
        json!({ "objectId": object_id }),
        2,
    )?;
    let backend_node_id = described
        .get("node")
        .and_then(|node| node.get("backendNodeId"))
        .and_then(Value::as_i64)?;
    cdp_command_on_stream(
        &mut stream,
        &ws_url,
        "DOM.setFileInputFiles",
        json!({ "backendNodeId": backend_node_id, "files": file_paths }),
        3,
    )?;
    Some(json!({
        "selector": selector,
        "file_count": file_paths.len(),
        "backend_node_id": backend_node_id,
    }))
}

#[allow(dead_code)]
pub(crate) fn page_ready_state(port: u16, page_id: Option<&str>) -> Option<String> {
    evaluate_on_page(
        port,
        page_id,
        "document.readyState",
        false,
        Duration::from_secs(2),
    )
    .and_then(|value| value.as_str().map(str::to_string))
}

#[allow(dead_code)]
pub(crate) fn wait_for_page_ready(
    port: u16,
    page_id: Option<&str>,
    condition: &str,
    timeout: Duration,
) -> bool {
    let started = std::time::Instant::now();
    loop {
        let ready = page_ready_state(port, page_id);
        let matched = match condition {
            "none" => true,
            "domcontentloaded" | "stable" | "networkidle" => ready
                .as_deref()
                .is_some_and(|state| state == "interactive" || state == "complete"),
            "load" => ready.as_deref() == Some("complete"),
            _ => true,
        };
        if matched {
            return true;
        }
        if started.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn new_browser_page(port: u16, url: &str, timeout: Duration) -> Option<PageInfo> {
    let target = http_json(
        port,
        "PUT",
        &format!("/json/new?{}", percent_encode_url(url)),
    )?;
    let target_id = target
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)?;
    wait_for_page_ready(port, Some(&target_id), "domcontentloaded", timeout);
    page_by_id(port, Some(&target_id)).or_else(|| page_from_json(&target))
}

#[allow(dead_code)]
fn pages_json(port: u16) -> Option<String> {
    http_text(port, "GET", "/json")
}

fn browser_websocket_url(port: u16) -> Option<String> {
    http_json(port, "GET", "/json/version")?
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(|url| ws_url_with_fallback_port(url, port))
}

fn http_json(port: u16, method: &str, path: &str) -> Option<Value> {
    http_text(port, method, path).and_then(|payload| serde_json::from_str(&payload).ok())
}

fn http_text(port: u16, method: &str, path: &str) -> Option<String> {
    let host = cdp_host();
    let mut stream = match TcpStream::connect((host.as_str(), port)) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::debug!(host, port, %error, "failed to connect to CDP HTTP endpoint");
            return None;
        }
    };
    let timeout = Some(Duration::from_secs(2));
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    if let Err(error) = stream.write_all(
        format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
    ) {
        tracing::debug!(host, port, %error, "failed to write CDP HTTP request");
        return None;
    }
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                bytes.extend_from_slice(&buffer[..count]);
                if http_response_body_complete(&bytes) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if bytes.is_empty() {
                    tracing::debug!(host, port, %error, "timed out before reading CDP HTTP response");
                    return None;
                }
                break;
            }
            Err(error) => {
                tracing::debug!(host, port, %error, "failed to read CDP HTTP response");
                return None;
            }
        }
    }
    let response = String::from_utf8_lossy(&bytes).to_string();
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .or(Some(response))
}

fn percent_encode_url(url: &str) -> String {
    url.bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b':'
            | b'/'
            | b'?'
            | b'&'
            | b'='
            | b'#'
            | b'%' => vec![byte as char],
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn cdp_host() -> String {
    env::var("SOOTIE_CDP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn http_response_body_complete(bytes: &[u8]) -> bool {
    let Some(header_end) = find_header_end(bytes) else {
        return false;
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]);
    let Some(content_length) = http_content_length(&headers) else {
        return false;
    };
    bytes.len() >= header_end + 4 + content_length
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

fn http_content_length(headers: &str) -> Option<usize> {
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())
            .flatten()
    })
}

fn parse_page_url(payload: &str) -> Option<String> {
    parse_current_page(payload).map(|page| page.url)
}

fn parse_current_page(payload: &str) -> Option<PageInfo> {
    parse_pages(payload)?.into_iter().find(is_reportable_page)
}

fn parse_pages(payload: &str) -> Option<Vec<PageInfo>> {
    let pages = serde_json::from_str::<serde_json::Value>(payload).ok()?;
    Some(
        pages
            .as_array()?
            .iter()
            .filter_map(page_from_json)
            .collect(),
    )
}

fn page_from_json(page: &Value) -> Option<PageInfo> {
    let target_id = page
        .get("id")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .or_else(|| {
            page.get("webSocketDebuggerUrl")
                .and_then(serde_json::Value::as_str)
                .and_then(|url| url.rsplit('/').next())
                .filter(|id| !id.is_empty())
        })
        .or_else(|| page.get("url").and_then(serde_json::Value::as_str))
        .map(str::to_string)?;
    let url = page
        .get("url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let page_type = page
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("other")
        .to_string();
    Some(PageInfo {
        target_id,
        url,
        title: page
            .get("title")
            .and_then(serde_json::Value::as_str)
            .filter(|title| !title.is_empty())
            .map(str::to_string),
        page_type,
        web_socket_debugger_url: page
            .get("webSocketDebuggerUrl")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    })
}

fn is_reportable_page(page: &PageInfo) -> bool {
    page.page_type == "page" && is_reportable_url(&page.url)
}

fn is_reportable_url(url: &str) -> bool {
    !url.is_empty()
        && !url.starts_with("devtools://")
        && !url.starts_with("chrome://")
        && !url.starts_with("chrome-extension://")
        && !url.starts_with("chrome-untrusted://")
        && !url.starts_with("edge://")
        && !url.starts_with("brave://")
        && url != "about:blank"
}

fn evaluate_json(ws_url: &str, expression: &str) -> Option<Value> {
    evaluate_json_with_options(ws_url, expression, false, Duration::from_secs(2))
}

fn evaluate_json_await(ws_url: &str, expression: &str, timeout: Duration) -> Option<Value> {
    evaluate_json_with_options(ws_url, expression, true, timeout)
}

fn evaluate_json_with_options(
    ws_url: &str,
    expression: &str,
    await_promise: bool,
    timeout: Duration,
) -> Option<Value> {
    let result = cdp_command_with_timeout(
        ws_url,
        "Runtime.evaluate",
        json!({
            "expression": expression,
            "returnByValue": true,
            "awaitPromise": await_promise
        }),
        timeout,
    )?;
    if result.get("exceptionDetails").is_some() {
        tracing::debug!(
            ws_url,
            exception = %result["exceptionDetails"],
            "CDP Runtime.evaluate returned exception"
        );
        return None;
    }
    let value = result
        .get("result")
        .and_then(|result| result.get("value"))
        .cloned();
    if value.is_none() {
        tracing::debug!(ws_url, result = %result, "CDP Runtime.evaluate returned no by-value result");
    } else if let Some(value) = &value {
        tracing::debug!(
            ws_url,
            value_type = value_type_label(value),
            array_len = value.as_array().map(Vec::len),
            "CDP Runtime.evaluate returned by-value result"
        );
    }
    value
}

fn value_type_label(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn cdp_command(ws_url: &str, method: &str, params: Value) -> Option<Value> {
    cdp_command_with_timeout(ws_url, method, params, Duration::from_secs(2))
}

fn cdp_command_with_timeout(
    ws_url: &str,
    method: &str,
    params: Value,
    timeout: Duration,
) -> Option<Value> {
    let mut stream = open_cdp_stream(ws_url, timeout)?;
    cdp_command_on_stream(&mut stream, ws_url, method, params, 1)
}

fn open_cdp_stream(ws_url: &str, timeout: Duration) -> Option<TcpStream> {
    let endpoint = match parse_ws_endpoint(ws_url) {
        Some(endpoint) => endpoint,
        None => {
            tracing::debug!(ws_url, "failed to parse CDP WebSocket endpoint");
            return None;
        }
    };
    let mut stream = match TcpStream::connect((endpoint.host.as_str(), endpoint.port)) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::debug!(
                ws_url,
                %error,
                "failed to connect to CDP WebSocket endpoint"
            );
            return None;
        }
    };
    let timeout = Some(timeout);
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    if websocket_handshake(&mut stream, &endpoint).is_none() {
        tracing::debug!(ws_url, "CDP WebSocket handshake failed");
        return None;
    }
    Some(stream)
}

fn cdp_command_on_stream(
    stream: &mut TcpStream,
    ws_url: &str,
    method: &str,
    params: Value,
    id: i64,
) -> Option<Value> {
    let request = json!({
        "id": id,
        "method": method,
        "params": params
    });
    if send_ws_text(stream, &request.to_string()).is_none() {
        tracing::debug!(ws_url, method, "failed to write CDP WebSocket frame");
        return None;
    }
    for _ in 0..8 {
        let message = match read_ws_text(stream) {
            Some(message) => message,
            None => {
                tracing::debug!(
                    ws_url,
                    method,
                    "CDP WebSocket closed before command response"
                );
                return None;
            }
        };
        let payload = match serde_json::from_str::<Value>(&message) {
            Ok(payload) => payload,
            Err(error) => {
                tracing::debug!(ws_url, method, %error, "CDP WebSocket returned invalid JSON");
                return None;
            }
        };
        if payload.get("id").and_then(Value::as_i64) == Some(id) {
            if payload.get("error").is_some() {
                return None;
            }
            return payload.get("result").cloned();
        }
    }
    tracing::debug!(ws_url, method, "CDP command response id was not observed");
    None
}

fn evaluate_dom_action(port: u16, query: &FindQuery, body: &str) -> Option<Value> {
    evaluate_dom_action_with_options(port, query, body, false, Duration::from_secs(2))
}

fn evaluate_dom_action_await(
    port: u16,
    query: &FindQuery,
    body: &str,
    timeout: Duration,
) -> Option<Value> {
    evaluate_dom_action_with_options(port, query, body, true, timeout)
}

fn evaluate_dom_action_with_options(
    port: u16,
    query: &FindQuery,
    body: &str,
    await_promise: bool,
    timeout: Duration,
) -> Option<Value> {
    let page = current_page(port)?;
    let ws_url = ws_url_with_fallback_port(page.web_socket_debugger_url.as_deref()?, port);
    let query = serde_json::to_string(&query_value(query)).ok()?;
    let expression =
        format!("(() => {{\nconst query = {query};\n{DOM_QUERY_HELPERS}\n{body}\n}})()");
    let value = if await_promise {
        evaluate_json_await(&ws_url, &expression, timeout)?
    } else {
        evaluate_json(&ws_url, &expression)?
    };
    (value.get("ok").and_then(Value::as_bool) == Some(true)).then_some(value)
}

fn action_result(method: &str, value: Value, query: &FindQuery) -> Option<ActionResult> {
    Some(ActionResult {
        method: method.to_string(),
        details: json!({
            "query": query_value(query),
            "element": value
        }),
    })
}

fn finite_number(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn cdp_modifier_mask(modifiers: &[String]) -> Option<u8> {
    modifiers.iter().try_fold(0_u8, |mask, modifier| {
        let bit = match modifier.to_lowercase().as_str() {
            "alt" | "option" => 1,
            "ctrl" | "control" => 2,
            "meta" | "cmd" | "command" | "super" | "win" | "windows" => 4,
            "shift" => 8,
            _ => return None,
        };
        Some(mask | bit)
    })
}

fn key_descriptor(key: &str) -> Option<KeyDescriptor> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let lower = key.to_lowercase();
    let descriptor = match lower.as_str() {
        "enter" | "return" => ("Enter", "Enter", 13),
        "escape" | "esc" => ("Escape", "Escape", 27),
        "tab" => ("Tab", "Tab", 9),
        "backspace" => ("Backspace", "Backspace", 8),
        "delete" | "del" => ("Delete", "Delete", 46),
        "space" | "spacebar" => (" ", "Space", 32),
        "arrowup" | "up" => ("ArrowUp", "ArrowUp", 38),
        "arrowdown" | "down" => ("ArrowDown", "ArrowDown", 40),
        "arrowleft" | "left" => ("ArrowLeft", "ArrowLeft", 37),
        "arrowright" | "right" => ("ArrowRight", "ArrowRight", 39),
        "home" => ("Home", "Home", 36),
        "end" => ("End", "End", 35),
        "pageup" => ("PageUp", "PageUp", 33),
        "pagedown" => ("PageDown", "PageDown", 34),
        _ => return printable_key_descriptor(key),
    };
    Some(KeyDescriptor {
        key: descriptor.0.to_string(),
        code: descriptor.1.to_string(),
        windows_virtual_key_code: descriptor.2,
    })
}

fn printable_key_descriptor(key: &str) -> Option<KeyDescriptor> {
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || !ch.is_ascii() {
        return None;
    }
    if ch.is_ascii_alphabetic() {
        let upper = ch.to_ascii_uppercase();
        return Some(KeyDescriptor {
            key: ch.to_ascii_lowercase().to_string(),
            code: format!("Key{upper}"),
            windows_virtual_key_code: upper as u32,
        });
    }
    if ch.is_ascii_digit() {
        return Some(KeyDescriptor {
            key: ch.to_string(),
            code: format!("Digit{ch}"),
            windows_virtual_key_code: ch as u32,
        });
    }
    None
}

fn parse_ws_endpoint(url: &str) -> Option<WsEndpoint> {
    let rest = url.strip_prefix("ws://")?;
    let (host_port, path) = rest.split_once('/')?;
    let (host, port) = host_port.rsplit_once(':')?;
    Some(WsEndpoint {
        host: host.to_string(),
        port: port.parse().ok()?,
        path: format!("/{path}"),
    })
}

fn ws_url_with_fallback_port(url: &str, fallback_port: u16) -> String {
    if parse_ws_endpoint(url).is_some() {
        return url.to_string();
    }
    let Some(rest) = url.strip_prefix("ws://") else {
        return url.to_string();
    };
    let Some((host, path)) = rest.split_once('/') else {
        return url.to_string();
    };
    if host.rsplit_once(':').is_some() {
        return url.to_string();
    }
    format!("ws://{host}:{fallback_port}/{path}")
}

fn websocket_handshake(stream: &mut TcpStream, endpoint: &WsEndpoint) -> Option<()> {
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}:{}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
        endpoint.path, endpoint.host, endpoint.port
    );
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = Vec::new();
    let mut byte = [0u8; 1];
    while response.len() < 8192 {
        stream.read_exact(&mut byte).ok()?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let response = String::from_utf8_lossy(&response);
    response.starts_with("HTTP/1.1 101").then_some(())
}

fn send_ws_text(stream: &mut TcpStream, message: &str) -> Option<()> {
    let payload = message.as_bytes();
    let mut frame = Vec::with_capacity(payload.len() + 16);
    frame.push(0x81);
    if payload.len() < 126 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= u16::MAX as usize {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
    let mask = [0x73, 0x6f, 0x6f, 0x74];
    frame.extend_from_slice(&mask);
    for (index, byte) in payload.iter().enumerate() {
        frame.push(byte ^ mask[index % mask.len()]);
    }
    stream.write_all(&frame).ok()
}

fn read_ws_text(stream: &mut TcpStream) -> Option<String> {
    for _ in 0..8 {
        let mut header = [0u8; 2];
        stream.read_exact(&mut header).ok()?;
        let opcode = header[0] & 0x0f;
        let masked = header[1] & 0x80 != 0;
        let mut len = (header[1] & 0x7f) as u64;
        if len == 126 {
            let mut bytes = [0u8; 2];
            stream.read_exact(&mut bytes).ok()?;
            len = u16::from_be_bytes(bytes) as u64;
        } else if len == 127 {
            let mut bytes = [0u8; 8];
            stream.read_exact(&mut bytes).ok()?;
            len = u64::from_be_bytes(bytes);
        }
        if len > 1_048_576 {
            return None;
        }
        let mut mask = [0u8; 4];
        if masked {
            stream.read_exact(&mut mask).ok()?;
        }
        let mut payload = vec![0u8; len as usize];
        stream.read_exact(&mut payload).ok()?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % mask.len()];
            }
        }
        match opcode {
            0x1 => return String::from_utf8(payload).ok(),
            0x8 => return None,
            0x9 | 0xA => continue,
            _ => continue,
        }
    }
    None
}

fn parse_dom_elements(value: &Value) -> Vec<ElementInfo> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(dom_element)
        .collect()
}

fn normalize_read_text(text: String) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn dom_element(value: &Value) -> Option<ElementInfo> {
    let role = string_field(value, "role").unwrap_or_else(|| "generic".to_string());
    let editable = value.get("editable").and_then(Value::as_bool);
    let mut actions = Vec::new();
    if editable == Some(true) {
        actions.push("setValue".to_string());
    }
    if role.contains("button")
        || role.contains("link")
        || value
            .get("clickable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        actions.push("click".to_string());
    }
    actions.sort();
    actions.dedup();
    Some(ElementInfo {
        id: string_field(value, "id"),
        role,
        title: string_field(value, "title"),
        name: string_field(value, "name"),
        text: string_field(value, "text"),
        bounds: bounds_field(value, "bounds"),
        actions,
        editable,
        enabled: value.get("enabled").and_then(Value::as_bool),
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn bounds_field(value: &Value, key: &str) -> Option<Bounds> {
    let bounds = value.get(key)?;
    Some(Bounds {
        x: bounds.get("x")?.as_f64()?,
        y: bounds.get("y")?.as_f64()?,
        width: bounds.get("width")?.as_f64()?,
        height: bounds.get("height")?.as_f64()?,
    })
}

fn query_value(query: &FindQuery) -> Value {
    json!({
        "query": query.query,
        "role": query.role,
        "dom_id": query.dom_id,
        "dom_class": query.dom_class,
        "identifier": query.identifier
    })
}

const DOM_QUERY_HELPERS: &str = r#"
function roleOf(el) {
  const tag = el.tagName.toLowerCase();
  return el.getAttribute('role')
    || (tag === 'a' ? 'link' : null)
    || (tag === 'button' ? 'button' : null)
    || (tag === 'textarea' ? 'textbox' : null)
    || (tag === 'select' ? 'combobox' : null)
    || (tag === 'input' ? (el.getAttribute('type') || 'input') : null)
    || tag;
}
function labelOf(el) {
  return String(
    el.getAttribute('aria-label')
    || el.getAttribute('alt')
    || el.getAttribute('placeholder')
    || el.getAttribute('name')
    || el.innerText
    || el.value
    || el.id
    || ''
  ).trim();
}
function textOf(el) {
  return [
    labelOf(el),
    el.getAttribute('title') || '',
    el.getAttribute('data-testid') || '',
    el.id || '',
    el.className || ''
  ].join(' ').toLowerCase();
}
function findSootieElement(query) {
  const selector = 'a,button,input,textarea,select,[role],[onclick],[tabindex],[contenteditable="true"],[draggable="true"]';
  return Array.from(document.querySelectorAll(selector)).find((el) => {
    const role = roleOf(el).toLowerCase();
    const text = textOf(el);
    if (query.query && !text.includes(String(query.query).toLowerCase())) return false;
    if (query.role && !role.includes(String(query.role).toLowerCase())) return false;
    if (query.dom_id && el.id !== query.dom_id) return false;
    if (query.dom_class && !String(el.className || '').split(/\s+/).includes(query.dom_class)) return false;
    if (query.identifier) {
      const identifier = String(query.identifier).toLowerCase();
      const ids = [el.id, el.name, el.getAttribute('data-testid'), el.getAttribute('aria-label')]
        .filter(Boolean)
        .map((value) => String(value).toLowerCase());
      if (!ids.some((value) => value.includes(identifier))) return false;
    }
    return true;
  }) || null;
}
function elementPayload(el) {
  return {
    ok: true,
    id: el.id || el.getAttribute('data-testid') || null,
    role: roleOf(el),
    name: labelOf(el),
    editable: el.matches('input,textarea,select,[contenteditable="true"]'),
    enabled: !(el.disabled || el.getAttribute('aria-disabled') === 'true')
  };
}
"#;

const CLICK_ELEMENT_BODY: &str = r#"
const el = findSootieElement(query);
if (!el) return { ok: false, reason: 'not-found' };
el.click();
return elementPayload(el);
"#;

const TYPE_TEXT_ELEMENT_BODY: &str = r#"
const el = findSootieElement(query);
if (!el) return { ok: false, reason: 'not-found' };
el.focus();
if ('value' in el) {
  el.value = clear ? text : String(el.value || '') + text;
  el.dispatchEvent(new Event('input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
} else if (el.isContentEditable) {
  el.textContent = clear ? text : String(el.textContent || '') + text;
  el.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: text }));
} else {
  return { ok: false, reason: 'not-editable' };
}
return elementPayload(el);
"#;

const HOVER_ELEMENT_BODY: &str = r#"
const el = findSootieElement(query);
if (!el) return { ok: false, reason: 'not-found' };
const rect = el.getBoundingClientRect();
const clientX = rect.left + rect.width / 2;
const clientY = rect.top + rect.height / 2;
const eventOptions = { bubbles: true, cancelable: true, view: window, clientX, clientY };
el.dispatchEvent(new MouseEvent('mouseover', eventOptions));
el.dispatchEvent(new MouseEvent('mouseenter', eventOptions));
el.dispatchEvent(new MouseEvent('mousemove', eventOptions));
return { ...elementPayload(el), clientX, clientY };
"#;

const LONG_PRESS_ELEMENT_BODY: &str = r#"
const el = findSootieElement(query);
if (!el) return { ok: false, reason: 'not-found' };
const rect = el.getBoundingClientRect();
const clientX = rect.left + rect.width / 2;
const clientY = rect.top + rect.height / 2;
const button = buttonName === 'right' ? 2 : (buttonName === 'middle' ? 1 : 0);
const buttons = buttonName === 'right' ? 2 : (buttonName === 'middle' ? 4 : 1);
const eventOptions = { bubbles: true, cancelable: true, view: window, clientX, clientY, button, buttons };
return new Promise((resolve) => {
  el.dispatchEvent(new MouseEvent('mouseover', eventOptions));
  el.dispatchEvent(new MouseEvent('mousemove', eventOptions));
  el.dispatchEvent(new MouseEvent('mousedown', eventOptions));
  window.setTimeout(() => {
    el.dispatchEvent(new MouseEvent('mouseup', eventOptions));
    el.dispatchEvent(new MouseEvent('click', { ...eventOptions, buttons: 0 }));
    resolve({ ...elementPayload(el), clientX, clientY, button: buttonName, duration_ms: durationMs });
  }, durationMs);
});
"#;

const DRAG_ELEMENT_BODY: &str = r#"
const el = findSootieElement(query);
if (!el) return { ok: false, reason: 'not-found' };
const rect = el.getBoundingClientRect();
const fromX = rect.left + rect.width / 2;
const fromY = rect.top + rect.height / 2;
function mouse(type, x, y, buttons) {
  el.dispatchEvent(new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    view: window,
    clientX: x,
    clientY: y,
    button: 0,
    buttons
  }));
}
return new Promise((resolve) => {
  mouse('mouseover', fromX, fromY, 0);
  mouse('mousemove', fromX, fromY, 0);
  mouse('mousedown', fromX, fromY, 1);
  window.setTimeout(() => {
    mouse('mousemove', toX, toY, 1);
    window.setTimeout(() => {
      mouse('mouseup', toX, toY, 0);
      resolve({ ...elementPayload(el), fromX, fromY, toX, toY, duration_ms: durationMs, hold_ms: holdMs });
    }, durationMs);
  }, holdMs);
});
"#;

const READ_TEXT_BODY: &str = r#"
const root = document.body || document.documentElement;
if (!root) return '';
function normalizeText(value) {
  return String(value || '')
    .replace(/\r/g, '\n')
    .split(/\n+/)
    .map((line) => line.trim())
    .filter(Boolean)
    .join('\n');
}
function elementDepth(el) {
  let depth = 0;
  let current = el;
  while (current && current !== root) {
    depth += 1;
    current = current.parentElement;
  }
  return depth;
}
function isVisible(el) {
  const style = window.getComputedStyle(el);
  if (style.display === 'none' || style.visibility === 'hidden') return false;
  const rect = el.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}
function controlValue(el) {
  if (el.matches('input,textarea,select')) return el.value || '';
  if (el.isContentEditable) return el.innerText || el.textContent || '';
  return '';
}
function searchText(el) {
  return [
    el.innerText || '',
    el.textContent || '',
    controlValue(el),
    el.getAttribute('aria-label') || '',
    el.getAttribute('alt') || '',
    el.getAttribute('title') || '',
    el.getAttribute('placeholder') || '',
    el.getAttribute('name') || '',
    el.id || ''
  ].join('\n');
}
const controlText = Array.from(root.querySelectorAll('input,textarea,select,[contenteditable="true"]'))
  .filter(isVisible)
  .map(controlValue)
  .filter(Boolean)
  .join('\n');
const fullText = normalizeText([root.innerText || root.textContent || '', controlText].join('\n'));
if (!query) return fullText;
const needle = String(query).toLowerCase();
const matchingLines = fullText
  .split('\n')
  .filter((line) => line.toLowerCase().includes(needle));
if (matchingLines.length > 0) return matchingLines.join('\n');
const seen = new Set();
const blocks = Array.from(root.querySelectorAll('*'))
  .filter((el) => maxDepth === null || elementDepth(el) <= maxDepth)
  .filter(isVisible)
  .filter((el) => searchText(el).toLowerCase().includes(needle))
  .map((el) => normalizeText(el.innerText || el.textContent || controlValue(el) || el.getAttribute('aria-label') || el.getAttribute('placeholder') || ''))
  .filter(Boolean)
  .filter((text) => {
    if (seen.has(text)) return false;
    seen.add(text);
    return true;
  })
  .slice(0, 50);
return blocks.join('\n');
"#;

const SCROLL_PAGE_BODY: &str = r#"
const pixels = Math.max(1, Number(amount) || 1) * 400;
let left = 0;
let top = 0;
if (direction === 'left') left = -pixels;
else if (direction === 'right') left = pixels;
else if (direction === 'up') top = -pixels;
else top = pixels;
window.scrollBy({ left, top, behavior: 'instant' });
return {
  ok: true,
  direction,
  amount,
  delta_x: left,
  delta_y: top,
  scroll_x: window.scrollX,
  scroll_y: window.scrollY
};
"#;

const DOM_ELEMENTS_EXPRESSION: &str = r#"(() => {
  const selector = 'a,button,input,textarea,select,[role],[onclick],[tabindex],[contenteditable="true"],[draggable="true"]';
  function isVisible(el) {
    const style = window.getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    return style.display !== 'none' && style.visibility !== 'hidden' && rect.width > 0 && rect.height > 0;
  }
  return Array.from(document.querySelectorAll(selector)).filter(isVisible).slice(0, 200).map((el) => {
    const tag = el.tagName.toLowerCase();
    const rect = el.getBoundingClientRect();
    const role = el.getAttribute('role')
      || (tag === 'a' ? 'link' : null)
      || (tag === 'button' ? 'button' : null)
      || (tag === 'textarea' ? 'textbox' : null)
      || (tag === 'select' ? 'combobox' : null)
      || (tag === 'input' ? (el.getAttribute('type') || 'input') : null)
      || tag;
    const text = String(el.innerText || el.value || '').trim();
    const name = String(
      el.getAttribute('aria-label')
      || el.getAttribute('alt')
      || el.getAttribute('placeholder')
      || el.getAttribute('name')
      || text
      || el.id
      || ''
    ).trim();
    const editable = el.matches('input,textarea,select,[contenteditable="true"]');
    const enabled = !(el.disabled || el.getAttribute('aria-disabled') === 'true');
    return {
      id: el.id || el.getAttribute('data-testid') || null,
      role,
      title: el.getAttribute('title') || el.getAttribute('aria-label') || null,
      name,
      text,
      bounds: { x: rect.left, y: rect.top, width: rect.width, height: rect.height },
      editable,
      enabled,
      clickable: Boolean(el.onclick || el.getAttribute('onclick') || role === 'button' || role === 'link')
    };
  });
})()"#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::sync::Mutex;

    use serde_json::json;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static TCP_MOCK_LOCK: Mutex<()> = Mutex::new(());
    const CDP_ENV_KEYS: [&str; 3] = ["SOOTIE_CDP_PORT", "SOOTIE_CDP_WS_URL", "SOOTIE_CDP_HOST"];

    #[test]
    fn parses_remote_debugging_port_from_cmdline() {
        assert_eq!(
            parse_remote_debugging_port(
                "google-chrome --profile-directory=Default --remote-debugging-port=9222"
            ),
            Some(9222)
        );
        assert_eq!(
            parse_remote_debugging_port("chromium --remote-debugging-port 9333"),
            Some(9333)
        );
        assert_eq!(parse_remote_debugging_port("firefox --new-window"), None);
    }

    #[test]
    fn parses_page_url() {
        let payload = r#"[
          {"type":"other","url":"devtools://devtools/bundled/inspector.html"},
          {"id":"internal","type":"page","url":"chrome://new-tab-page","title":"New Tab","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/internal"},
          {"id":"blank","type":"page","url":"about:blank"},
          {"id":"page-1","type":"page","url":"https://example.com/current","title":"Current","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/1"}
        ]"#;
        assert_eq!(
            parse_page_url(payload),
            Some("https://example.com/current".to_string())
        );
        assert_eq!(
            parse_current_page(payload),
            Some(PageInfo {
                target_id: "page-1".to_string(),
                url: "https://example.com/current".to_string(),
                title: Some("Current".to_string()),
                page_type: "page".to_string(),
                web_socket_debugger_url: Some("ws://127.0.0.1:9222/devtools/page/1".to_string())
            })
        );
    }

    #[test]
    fn parses_pages_for_browser_api() {
        let payload = r#"[
          {"id":"page-1","type":"page","url":"https://example.com","title":"Example","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/page-1"},
          {"id":"bg-1","type":"background_page","url":"chrome-extension://extension/background.html","title":"Background"}
        ]"#;
        let pages = parse_pages(payload).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].target_id, "page-1");
        assert_eq!(pages[0].page_type, "page");
        assert_eq!(pages[1].page_type, "background_page");
    }

    #[test]
    fn configured_port_prefers_explicit_port() {
        with_cdp_env(
            &[
                ("SOOTIE_CDP_PORT", Some("9333")),
                (
                    "SOOTIE_CDP_WS_URL",
                    Some("ws://127.0.0.1:9444/devtools/page/fallback"),
                ),
            ],
            || {
                assert_eq!(configured_port(), Some(9333));
            },
        );
    }

    #[test]
    fn configured_port_falls_back_to_websocket_url() {
        with_cdp_env(
            &[(
                "SOOTIE_CDP_WS_URL",
                Some("ws://127.0.0.1:9444/devtools/page/fallback"),
            )],
            || {
                assert_eq!(configured_port(), Some(9444));
            },
        );
    }

    #[test]
    fn configured_port_ignores_invalid_values() {
        with_cdp_env(
            &[
                ("SOOTIE_CDP_PORT", Some("not-a-port")),
                (
                    "SOOTIE_CDP_WS_URL",
                    Some("ws://127.0.0.1:9555/devtools/page/fallback"),
                ),
            ],
            || {
                assert_eq!(configured_port(), Some(9555));
            },
        );
    }

    #[test]
    fn cdp_host_uses_default_or_environment_override() {
        with_cdp_env(&[], || {
            assert_eq!(cdp_host(), "127.0.0.1");
        });
        with_cdp_env(&[("SOOTIE_CDP_HOST", Some("browser.local"))], || {
            assert_eq!(cdp_host(), "browser.local");
        });
    }

    #[test]
    fn parses_local_ws_endpoint() {
        let endpoint = parse_ws_endpoint("ws://127.0.0.1:9222/devtools/page/ABC").unwrap();
        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 9222);
        assert_eq!(endpoint.path, "/devtools/page/ABC");
    }

    #[test]
    fn fills_missing_websocket_port_from_cdp_http_port() {
        assert_eq!(
            ws_url_with_fallback_port("ws://127.0.0.1/devtools/page/ABC", 9222),
            "ws://127.0.0.1:9222/devtools/page/ABC"
        );
        assert_eq!(
            ws_url_with_fallback_port("ws://127.0.0.1:9333/devtools/page/ABC", 9222),
            "ws://127.0.0.1:9333/devtools/page/ABC"
        );
    }

    #[test]
    fn detects_complete_http_response_body_from_content_length() {
        let partial = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhe";
        let complete = b"HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello";
        assert!(!http_response_body_complete(partial));
        assert!(http_response_body_complete(complete));
        assert!(find_header_end(complete).is_some());
        assert_eq!(
            http_content_length("HTTP/1.1 200 OK\r\nContent-Length: 12"),
            Some(12)
        );
    }

    #[test]
    fn parses_dom_elements_from_runtime_value() {
        let elements = parse_dom_elements(&json!([
            {
                "id": "submit",
                "role": "button",
                "name": "Submit",
                "text": "Submit",
                "bounds": { "x": 10, "y": 20, "width": 80, "height": 24 },
                "editable": false,
                "enabled": true,
                "clickable": true
            },
            {
                "id": "name",
                "role": "textbox",
                "name": "Name",
                "editable": true,
                "enabled": true
            }
        ]));
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[0].id.as_deref(), Some("submit"));
        assert_eq!(elements[0].actions, vec!["click"]);
        assert_eq!(elements[0].bounds.as_ref().unwrap().x, 10.0);
        assert_eq!(elements[0].bounds.as_ref().unwrap().width, 80.0);
        assert_eq!(elements[1].editable, Some(true));
        assert_eq!(elements[1].actions, vec!["setValue"]);
    }

    #[test]
    fn evaluate_json_round_trips_local_websocket() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut handshake = Vec::new();
            let mut byte = [0u8; 1];
            while !handshake.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                handshake.push(byte[0]);
            }
            assert_valid_websocket_key(&String::from_utf8(handshake).unwrap());
            stream
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                )
                .unwrap();
            let request = read_ws_text(&mut stream).unwrap();
            assert!(request.contains("\"Runtime.evaluate\""));
            write_server_ws_text(
                &mut stream,
                &json!({
                    "id": 1,
                    "result": { "result": { "value": { "answer": 42 } } }
                })
                .to_string(),
            );
        });

        let value = evaluate_json(
            &format!("ws://127.0.0.1:{port}/devtools/page/test"),
            "1 + 1",
        )
        .unwrap();
        assert_eq!(value["answer"], 42);
        server.join().unwrap();
    }

    #[test]
    fn page_screenshot_uses_cdp_capture_screenshot() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let png = test_png_base64(320, 240);
        let png_for_server = png.clone();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            read_http_request(&mut http);
            let body = json!([{
                "type": "page",
                "url": "https://example.com",
                "title": "Example",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .unwrap();
            drop(http);

            let (mut ws, _) = listener.accept().unwrap();
            let handshake = read_http_request(&mut ws);
            assert_valid_websocket_key(&handshake);
            ws.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .unwrap();
            let request = read_ws_text(&mut ws).unwrap();
            assert!(request.contains("\"Page.captureScreenshot\""));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 1,
                    "result": { "data": png_for_server }
                })
                .to_string(),
            );
        });

        let screenshot = page_screenshot(port).unwrap();
        assert_eq!(screenshot.mime_type, "image/png");
        assert_eq!(screenshot.data_base64, png);
        assert_eq!(screenshot.width, Some(320));
        assert_eq!(screenshot.height, Some(240));
        assert_eq!(screenshot.window_title.as_deref(), Some("Example"));
        server.join().unwrap();
    }

    #[test]
    fn file_upload_uses_single_cdp_session_for_object_resolution() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            read_http_request(&mut http);
            let body = json!([{
                "id": "page-1",
                "type": "page",
                "url": "https://example.com",
                "title": "Example",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/page-1")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .unwrap();
            drop(http);

            let (mut ws, _) = listener.accept().unwrap();
            let _ = ws.set_read_timeout(Some(Duration::from_secs(2)));
            let handshake = read_http_request(&mut ws);
            assert_valid_websocket_key(&handshake);
            ws.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .unwrap();

            let evaluate = read_ws_text(&mut ws).expect("runtime evaluate request");
            assert!(evaluate.contains("\"Runtime.evaluate\""));
            assert!(evaluate.contains("document.querySelector"));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 1,
                    "result": { "result": { "objectId": "object-1" } }
                })
                .to_string(),
            );

            let describe = read_ws_text(&mut ws).expect("describe node request on same session");
            assert!(describe.contains("\"DOM.describeNode\""));
            assert!(describe.contains("\"objectId\":\"object-1\""));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 2,
                    "result": { "node": { "backendNodeId": 99 } }
                })
                .to_string(),
            );

            let upload = read_ws_text(&mut ws).expect("set file input files request");
            assert!(upload.contains("\"DOM.setFileInputFiles\""));
            assert!(upload.contains("\"backendNodeId\":99"));
            assert!(upload.contains("/tmp/sootie-upload.txt"));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 3,
                    "result": {}
                })
                .to_string(),
            );
        });

        let result = set_file_input_files(
            port,
            Some("page-1"),
            &json!({ "selector": "#file" }),
            &["/tmp/sootie-upload.txt".into()],
            Duration::from_secs(2),
        )
        .unwrap();
        assert_eq!(result["selector"], "#file");
        assert_eq!(result["file_count"], 1);
        assert_eq!(result["backend_node_id"], 99);
        server.join().unwrap();
    }

    #[test]
    fn page_text_uses_runtime_evaluate() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            read_http_request(&mut http);
            let body = json!([{
                "type": "page",
                "url": "https://example.com",
                "title": "Example",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .unwrap();
            drop(http);

            let (mut ws, _) = listener.accept().unwrap();
            let handshake = read_http_request(&mut ws);
            assert_valid_websocket_key(&handshake);
            ws.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .unwrap();
            let request = read_ws_text(&mut ws).unwrap();
            assert!(request.contains("\"Runtime.evaluate\""));
            assert!(request.contains("const query = \\\"Name\\\";"));
            assert!(request.contains("const maxDepth = 3;"));
            assert!(request.contains("controlValue(el)"));
            assert!(request.contains("input,textarea,select"));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 1,
                    "result": { "result": { "value": "  Name\n\n  Save  " } }
                })
                .to_string(),
            );
        });

        let text = page_text(port, Some("Name"), Some(3)).unwrap();
        assert_eq!(text, "Name\nSave");
        server.join().unwrap();
    }

    #[test]
    fn scroll_page_uses_runtime_evaluate() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            read_http_request(&mut http);
            let body = json!([{
                "type": "page",
                "url": "https://example.com",
                "title": "Example",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .unwrap();
            drop(http);

            let (mut ws, _) = listener.accept().unwrap();
            let handshake = read_http_request(&mut ws);
            assert_valid_websocket_key(&handshake);
            ws.write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .unwrap();
            let request = read_ws_text(&mut ws).unwrap();
            assert!(request.contains("\"Runtime.evaluate\""));
            assert!(request.contains("const direction = \\\"down\\\";"));
            assert!(request.contains("const amount = 2;"));
            write_server_ws_text(
                &mut ws,
                &json!({
                    "id": 1,
                    "result": {
                        "result": {
                            "value": {
                                "ok": true,
                                "direction": "down",
                                "amount": 2,
                                "delta_y": 800,
                                "scroll_y": 640
                            }
                        }
                    }
                })
                .to_string(),
            );
        });

        let result = scroll_page(port, "down", 2).unwrap();
        assert_eq!(result.method, "cdp-scroll");
        assert_eq!(result.details["scroll_y"], 640);
        server.join().unwrap();
    }

    #[test]
    fn press_key_dispatches_cdp_key_events() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut http, _) = listener.accept().unwrap();
            read_http_request(&mut http);
            let body = json!([{
                "type": "page",
                "url": "https://example.com",
                "title": "Example",
                "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
            }])
            .to_string();
            http.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .unwrap();
            drop(http);

            for event_type in ["rawKeyDown", "keyUp"] {
                let (mut ws, _) = listener.accept().unwrap();
                let handshake = read_http_request(&mut ws);
                assert_valid_websocket_key(&handshake);
                ws.write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                )
                .unwrap();
                let request = read_ws_text(&mut ws).unwrap();
                assert!(request.contains("\"Input.dispatchKeyEvent\""));
                assert!(request.contains(&format!("\"type\":\"{event_type}\"")));
                assert!(request.contains("\"key\":\"a\""));
                assert!(request.contains("\"code\":\"KeyA\""));
                assert!(request.contains("\"modifiers\":6"));
                write_server_ws_text(&mut ws, &json!({ "id": 1, "result": {} }).to_string());
            }
        });

        let result = press_key(port, "a", &["ctrl".to_string(), "cmd".to_string()]).unwrap();
        assert_eq!(result.method, "cdp-key");
        assert_eq!(result.details["key"], "a");
        assert_eq!(result.details["code"], "KeyA");
        assert_eq!(result.details["modifier_mask"], 6);
        server.join().unwrap();
    }

    #[test]
    fn maps_cdp_key_descriptors_and_modifiers() {
        let enter = key_descriptor("Enter").unwrap();
        assert_eq!(enter.key, "Enter");
        assert_eq!(enter.code, "Enter");
        assert_eq!(enter.windows_virtual_key_code, 13);
        let letter = key_descriptor("l").unwrap();
        assert_eq!(letter.key, "l");
        assert_eq!(letter.code, "KeyL");
        assert_eq!(letter.windows_virtual_key_code, 76);
        assert_eq!(
            cdp_modifier_mask(&["shift".to_string(), "alt".to_string()]),
            Some(9)
        );
        assert!(key_descriptor("not-a-key").is_none());
        assert!(cdp_modifier_mask(&["hyper".to_string()]).is_none());
    }

    #[test]
    fn hover_long_press_and_drag_use_runtime_evaluate() {
        let _tcp_guard = TCP_MOCK_LOCK.lock().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for index in 0..3 {
                let (mut http, _) = listener.accept().unwrap();
                read_http_request(&mut http);
                let body = json!([{
                    "type": "page",
                    "url": "https://example.com",
                    "title": "Example",
                    "webSocketDebuggerUrl": format!("ws://127.0.0.1:{port}/devtools/page/test")
                }])
                .to_string();
                http.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                )
                .unwrap();
                drop(http);

                let (mut ws, _) = listener.accept().unwrap();
                let handshake = read_http_request(&mut ws);
                assert_valid_websocket_key(&handshake);
                ws.write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
                )
                .unwrap();
                let request = read_ws_text(&mut ws).unwrap();
                assert!(request.contains("\"Runtime.evaluate\""));
                if index == 0 {
                    assert!(request.contains("MouseEvent('mouseover'"));
                    assert!(request.contains("\"awaitPromise\":false"));
                } else {
                    if index == 2 {
                        assert!(request.contains("const toX = 120;"));
                        assert!(request.contains("const toY = 240;"));
                        assert!(request.contains("const holdMs = 20;"));
                    } else {
                        assert!(request.contains("const durationMs = 10;"));
                        assert!(request.contains("const buttonName = \\\"right\\\";"));
                    }
                    assert!(request.contains("\"awaitPromise\":true"));
                }
                write_server_ws_text(
                    &mut ws,
                    &json!({
                        "id": 1,
                        "result": {
                            "result": {
                                "value": {
                                    "ok": true,
                                    "id": "target",
                                    "role": "button",
                                    "name": "Target",
                                    "clientX": 42,
                                    "clientY": 24
                                }
                            }
                        }
                    })
                    .to_string(),
                );
            }
        });

        let query = FindQuery {
            query: Some("Target".to_string()),
            ..Default::default()
        };
        let hover = hover_element(port, &query).unwrap();
        assert_eq!(hover.method, "cdp-hover");
        assert_eq!(hover.details["element"]["clientX"], 42);
        let long_press = long_press_element(port, &query, 0.01, "right").unwrap();
        assert_eq!(long_press.method, "cdp-long-press");
        assert_eq!(long_press.details["button"], "right");
        assert_eq!(long_press.details["duration"], 0.01);
        let drag = drag_element(port, &query, (120.0, 240.0), 0.03, 0.02).unwrap();
        assert_eq!(drag.method, "cdp-drag");
        assert_eq!(drag.details["to"]["x"], 120.0);
        assert_eq!(drag.details["duration"], 0.03);
        assert_eq!(drag.details["hold_duration"], 0.02);
        server.join().unwrap();
    }

    #[test]
    fn normalizes_read_text_lines() {
        assert_eq!(
            normalize_read_text("  First \n\n Second\r\n  ".to_string()),
            "First\nSecond"
        );
    }

    #[test]
    fn action_result_carries_selector_payload() {
        let query = FindQuery {
            query: Some("Submit".to_string()),
            role: Some("button".to_string()),
            dom_id: Some("submit".to_string()),
            dom_class: Some("primary".to_string()),
            identifier: Some("submit-button".to_string()),
            ..Default::default()
        };
        let result = action_result("cdp-click", json!({"ok": true}), &query).unwrap();
        assert_eq!(result.method, "cdp-click");
        assert_eq!(result.details["query"]["query"], "Submit");
        assert_eq!(result.details["query"]["role"], "button");
        assert_eq!(result.details["query"]["dom_id"], "submit");
        assert_eq!(result.details["query"]["dom_class"], "primary");
        assert_eq!(result.details["query"]["identifier"], "submit-button");
    }

    fn write_server_ws_text(stream: &mut TcpStream, message: &str) {
        let payload = message.as_bytes();
        let mut frame = Vec::with_capacity(payload.len() + 8);
        frame.push(0x81);
        if payload.len() < 126 {
            frame.push(payload.len() as u8);
        } else {
            frame.push(126);
            frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        }
        frame.extend_from_slice(payload);
        stream.write_all(&frame).unwrap();
    }

    fn read_http_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut byte = [0u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
        }
        String::from_utf8(bytes).unwrap()
    }

    fn test_png_base64(width: u32, height: u32) -> String {
        let mut bytes = vec![0; 24];
        bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        bytes[12..16].copy_from_slice(b"IHDR");
        bytes[16..20].copy_from_slice(&width.to_be_bytes());
        bytes[20..24].copy_from_slice(&height.to_be_bytes());
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn assert_valid_websocket_key(request: &str) {
        let key = request
            .lines()
            .find_map(|line| line.strip_prefix("Sec-WebSocket-Key: "))
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(key)
            .unwrap();
        assert_eq!(bytes.len(), 16);
    }

    fn with_cdp_env<T>(vars: &[(&str, Option<&str>)], test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved = CDP_ENV_KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<(&str, Option<OsString>)>>();

        for key in CDP_ENV_KEYS {
            std::env::remove_var(key);
        }
        for (key, value) in vars {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }

        let result = test();

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }

        result
    }
}
