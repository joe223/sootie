use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::perception::ScreenshotData;
use crate::selector::{
    Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector, Window,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpConnection {
    pub host: String,
    pub port: u16,
    pub ws_url: Option<String>,
}

impl CdpConnection {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            host: host.to_string(),
            port,
            ws_url: None,
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CdpPage {
    pub id: String,
    pub title: String,
    pub url: String,
    pub window_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpElement {
    pub node_id: u32,
    pub tag_name: String,
    pub text_content: Option<String>,
    pub attributes: std::collections::HashMap<String, String>,
    pub bounds: Option<Bounds>,
}

impl CdpElement {
    pub fn matches_selector(&self, selector: &Selector) -> bool {
        if let Some(ref role) = selector.element.role {
            let mapped_tag = map_role_to_tag(role);
            if self.tag_name.to_lowercase() != mapped_tag.to_lowercase() {
                return false;
            }
        }

        if let Some(ref name) = selector.element.name {
            let accessible_name = self
                .attributes
                .get("aria-label")
                .or_else(|| self.attributes.get("title"))
                .or_else(|| self.attributes.get("placeholder"));

            match accessible_name {
                Some(n) if n == name => {}
                _ => {
                    if let Some(ref text) = self.text_content {
                        if !text.contains(name.as_str()) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
        }

        if let Some(ref id) = selector.element.id {
            match self.attributes.get("id") {
                Some(dom_id) if dom_id == id => {}
                _ => return false,
            }
        }

        true
    }
}

fn map_role_to_tag(role: &str) -> String {
    match role {
        "button" => "button",
        "textfield" | "text_field" => "input",
        "link" => "a",
        "checkbox" => "input",
        "radio" => "input",
        "combobox" | "dropdown" => "select",
        "heading" => "h1",
        "image" => "img",
        "list" => "ul",
        "listitem" | "list_item" => "li",
        "tab" => "tab",
        "menu" => "menu",
        "menuitem" | "menu_item" => "menuitem",
        "dialog" => "dialog",
        "toolbar" => "toolbar",
        _ => role,
    }
    .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum CdpError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("page not found: {0}")]
    PageNotFound(String),

    #[error("element not found: {0}")]
    ElementNotFound(String),

    #[error("CDP protocol error: {0}")]
    ProtocolError(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[async_trait]
pub trait CdpProvider: Send + Sync {
    async fn connect(&mut self, connection: &CdpConnection) -> Result<(), CdpError>;

    async fn list_pages(&self) -> Result<Vec<CdpPage>, CdpError>;

    async fn find_elements(
        &self,
        page_id: &str,
        selector: &Selector,
    ) -> Result<Vec<CdpElement>, CdpError>;

    async fn click_element(
        &self,
        page_id: &str,
        node_id: u32,
        button: &str,
        click_count: u32,
    ) -> Result<(), CdpError>;

    async fn type_text(
        &self,
        page_id: &str,
        node_id: Option<u32>,
        text: &str,
        clear_first: bool,
    ) -> Result<(), CdpError>;

    async fn press_key(&self, page_id: &str, key: &str) -> Result<(), CdpError>;

    async fn scroll(
        &self,
        page_id: &str,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<(), CdpError>;

    async fn screenshot(&self, page_id: &str) -> Result<ScreenshotData, CdpError>;
}

pub struct StubCdpProvider;

#[async_trait]
impl CdpProvider for StubCdpProvider {
    async fn connect(&mut self, _connection: &CdpConnection) -> Result<(), CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn list_pages(&self) -> Result<Vec<CdpPage>, CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn find_elements(
        &self,
        _page_id: &str,
        _selector: &Selector,
    ) -> Result<Vec<CdpElement>, CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn click_element(
        &self,
        _page_id: &str,
        _node_id: u32,
        _button: &str,
        _click_count: u32,
    ) -> Result<(), CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn type_text(
        &self,
        _page_id: &str,
        _node_id: Option<u32>,
        _text: &str,
        _clear_first: bool,
    ) -> Result<(), CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn press_key(&self, _page_id: &str, _key: &str) -> Result<(), CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn scroll(
        &self,
        _page_id: &str,
        _x: f64,
        _y: f64,
        _delta_x: f64,
        _delta_y: f64,
    ) -> Result<(), CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }

    async fn screenshot(&self, _page_id: &str) -> Result<ScreenshotData, CdpError> {
        Err(CdpError::NotImplemented("stub provider".to_string()))
    }
}

pub struct WebSocketCdpProvider {
    connection: Option<CdpConnection>,
    ws_url: Option<String>,
    element_cache: Mutex<HashMap<String, Vec<CdpElement>>>,
}

impl WebSocketCdpProvider {
    pub fn new() -> Self {
        Self {
            connection: None,
            ws_url: None,
            element_cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_connection(connection: CdpConnection) -> Self {
        Self {
            ws_url: connection.ws_url.clone(),
            connection: Some(connection),
            element_cache: Mutex::new(HashMap::new()),
        }
    }

    async fn fetch_targets(&self) -> Result<Vec<TargetInfo>, CdpError> {
        let connection = self.connection.as_ref().ok_or_else(|| {
            CdpError::ConnectionFailed("CDP connection not configured".to_string())
        })?;
        let url = format!("{}/json", connection.endpoint_url());
        let client = reqwest::Client::new();
        client
            .get(url)
            .send()
            .await
            .map_err(|e| CdpError::ConnectionFailed(e.to_string()))?
            .json::<Vec<TargetInfo>>()
            .await
            .map_err(|e| CdpError::ConnectionFailed(e.to_string()))
    }

    async fn page_ws_url(&self, page_id: &str) -> Result<String, CdpError> {
        let targets = self.fetch_targets().await?;
        targets
            .into_iter()
            .find(|target| target.id == page_id)
            .and_then(|target| target.web_socket_debugger_url)
            .ok_or_else(|| CdpError::PageNotFound(page_id.to_string()))
    }

    async fn send_command(
        &self,
        ws_url: &str,
        id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, CdpError> {
        let (mut socket, _) = connect_async(ws_url)
            .await
            .map_err(|e| CdpError::ConnectionFailed(e.to_string()))?;

        let message = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        socket
            .send(Message::Text(message.to_string().into()))
            .await
            .map_err(|e| CdpError::ProtocolError(e.to_string()))?;

        while let Some(message) = socket.next().await {
            let message = message.map_err(|e| CdpError::ProtocolError(e.to_string()))?;
            let Message::Text(text) = message else {
                continue;
            };
            let payload: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| CdpError::ProtocolError(e.to_string()))?;
            if payload.get("id").and_then(|value| value.as_u64()) != Some(id) {
                continue;
            }
            if let Some(error) = payload.get("error") {
                return Err(CdpError::ProtocolError(error.to_string()));
            }
            return payload
                .get("result")
                .cloned()
                .ok_or_else(|| CdpError::ProtocolError("missing result".to_string()));
        }

        Err(CdpError::ProtocolError(
            "websocket closed without response".to_string(),
        ))
    }

    async fn evaluate_js(
        &self,
        ws_url: &str,
        expression: &str,
    ) -> Result<serde_json::Value, CdpError> {
        let result = self
            .send_command(
                ws_url,
                1,
                "Runtime.evaluate",
                serde_json::json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                }),
            )
            .await?;

        result
            .get("result")
            .and_then(|value| value.get("value"))
            .cloned()
            .ok_or_else(|| CdpError::ProtocolError("missing evaluation value".to_string()))
    }
}

impl Default for WebSocketCdpProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CdpProvider for WebSocketCdpProvider {
    async fn connect(&mut self, connection: &CdpConnection) -> Result<(), CdpError> {
        let http_url = connection.endpoint_url();
        let json_url = format!("{}{}", http_url, "/json");

        let client = reqwest::Client::new();
        let response = client
            .get(&json_url)
            .send()
            .await
            .map_err(|e| CdpError::ConnectionFailed(e.to_string()))?;

        let targets: Vec<serde_json::Value> = response
            .json()
            .await
            .map_err(|e| CdpError::ConnectionFailed(e.to_string()))?;

        let ws_url = if let Some(ref url) = connection.ws_url {
            url.clone()
        } else if let Some(first_target) = targets.first() {
            first_target
                .get("webSocketDebuggerUrl")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| CdpError::ConnectionFailed("No WebSocket URL found".to_string()))?
        } else {
            return Err(CdpError::ConnectionFailed(
                "No targets available".to_string(),
            ));
        };

        self.ws_url = Some(ws_url);
        self.connection = Some(connection.clone());

        Ok(())
    }

    async fn list_pages(&self) -> Result<Vec<CdpPage>, CdpError> {
        let targets = self.fetch_targets().await?;
        Ok(targets
            .into_iter()
            .filter(|target| target.target_type.as_deref() == Some("page"))
            .map(|target| CdpPage {
                id: target.id,
                title: target.title,
                url: target.url,
                window_id: None,
            })
            .collect())
    }

    async fn find_elements(
        &self,
        page_id: &str,
        selector: &Selector,
    ) -> Result<Vec<CdpElement>, CdpError> {
        let ws_url = self.page_ws_url(page_id).await?;
        let value = self.evaluate_js(&ws_url, DOM_SNAPSHOT_JS).await?;
        let elements = parse_cdp_elements(value, selector)?;
        self.element_cache
            .lock()
            .await
            .insert(page_id.to_string(), elements.clone());
        Ok(elements)
    }

    async fn click_element(
        &self,
        page_id: &str,
        node_id: u32,
        button: &str,
        click_count: u32,
    ) -> Result<(), CdpError> {
        let ws_url = self.page_ws_url(page_id).await?;
        let bounds = cached_element_bounds(&self.element_cache, page_id, node_id).await?;
        let x = bounds.x + bounds.width / 2.0;
        let y = bounds.y + bounds.height / 2.0;

        self.send_command(
            &ws_url,
            2,
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mousePressed",
                "x": x,
                "y": y,
                "button": button,
                "clickCount": click_count,
            }),
        )
        .await?;
        self.send_command(
            &ws_url,
            3,
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mouseReleased",
                "x": x,
                "y": y,
                "button": button,
                "clickCount": click_count,
            }),
        )
        .await?;
        Ok(())
    }

    async fn type_text(
        &self,
        page_id: &str,
        node_id: Option<u32>,
        text: &str,
        clear_first: bool,
    ) -> Result<(), CdpError> {
        if let Some(node_id) = node_id {
            self.click_element(page_id, node_id, "left", 1).await?;
        }

        let ws_url = self.page_ws_url(page_id).await?;
        if clear_first {
            self.evaluate_js(
                &ws_url,
                "(() => { const el = document.activeElement; if (!el) return false; if ('value' in el) el.value = ''; if ('textContent' in el) el.textContent = ''; return true; })()",
            )
            .await?;
        }

        self.send_command(
            &ws_url,
            4,
            "Input.insertText",
            serde_json::json!({ "text": text }),
        )
        .await?;
        Ok(())
    }

    async fn press_key(&self, page_id: &str, key: &str) -> Result<(), CdpError> {
        let ws_url = self.page_ws_url(page_id).await?;
        let key = normalize_key(key);
        self.send_command(
            &ws_url,
            5,
            "Input.dispatchKeyEvent",
            serde_json::json!({
                "type": "keyDown",
                "key": key,
                "text": if key.len() == 1 { key.clone() } else { String::new() },
            }),
        )
        .await?;
        self.send_command(
            &ws_url,
            6,
            "Input.dispatchKeyEvent",
            serde_json::json!({
                "type": "keyUp",
                "key": key,
            }),
        )
        .await?;
        Ok(())
    }

    async fn scroll(
        &self,
        page_id: &str,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<(), CdpError> {
        let ws_url = self.page_ws_url(page_id).await?;
        self.send_command(
            &ws_url,
            7,
            "Input.dispatchMouseEvent",
            serde_json::json!({
                "type": "mouseWheel",
                "x": x,
                "y": y,
                "deltaX": delta_x,
                "deltaY": delta_y,
            }),
        )
        .await?;
        Ok(())
    }

    async fn screenshot(&self, page_id: &str) -> Result<ScreenshotData, CdpError> {
        let ws_url = self.page_ws_url(page_id).await?;
        let result = self
            .send_command(&ws_url, 8, "Page.captureScreenshot", serde_json::json!({}))
            .await?;
        let data = result
            .get("data")
            .and_then(|value| value.as_str())
            .ok_or_else(|| CdpError::ProtocolError("missing screenshot data".to_string()))?;
        let data = decode_base64(data)?;
        Ok(ScreenshotData {
            format: crate::perception::ScreenshotFormat::Png,
            data,
            bounds: None,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct TargetInfo {
    id: String,
    title: String,
    url: String,
    #[serde(rename = "type")]
    target_type: Option<String>,
    #[serde(rename = "webSocketDebuggerUrl")]
    web_socket_debugger_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsElement {
    #[serde(rename = "tagName")]
    tag_name: String,
    #[serde(rename = "textContent")]
    text_content: Option<String>,
    attributes: HashMap<String, String>,
    bounds: Option<Bounds>,
    visible: bool,
}

const DOM_SNAPSHOT_JS: &str = r#"
(() => {
  return Array.from(document.querySelectorAll('*')).map((el) => {
    const rect = el.getBoundingClientRect();
    const style = window.getComputedStyle(el);
    const attributes = {};
    for (const attr of el.getAttributeNames()) {
      attributes[attr] = el.getAttribute(attr) ?? '';
    }
    return {
      tagName: el.tagName.toLowerCase(),
      textContent: (el.innerText || el.textContent || '').trim() || null,
      attributes,
      bounds: rect.width || rect.height ? {
        x: rect.left + window.scrollX,
        y: rect.top + window.scrollY,
        width: rect.width,
        height: rect.height
      } : null,
      visible: style.display !== 'none' && style.visibility !== 'hidden'
    };
  });
})()
"#;

fn parse_cdp_elements(
    value: serde_json::Value,
    selector: &Selector,
) -> Result<Vec<CdpElement>, CdpError> {
    let raw_elements = serde_json::from_value::<Vec<JsElement>>(value)
        .map_err(|e| CdpError::ProtocolError(e.to_string()))?;

    Ok(raw_elements
        .into_iter()
        .enumerate()
        .filter(|(_, element)| element.visible)
        .map(|(index, element)| CdpElement {
            node_id: index as u32 + 1,
            tag_name: element.tag_name,
            text_content: element.text_content,
            attributes: element.attributes,
            bounds: element.bounds,
        })
        .filter(|element| element.matches_selector(selector))
        .collect())
}

async fn cached_element_bounds(
    cache: &Mutex<HashMap<String, Vec<CdpElement>>>,
    page_id: &str,
    node_id: u32,
) -> Result<Bounds, CdpError> {
    let cache = cache.lock().await;
    let elements = cache.get(page_id).ok_or_else(|| {
        CdpError::ElementNotFound(format!("no cached elements for page {}", page_id))
    })?;
    let element = elements
        .iter()
        .find(|element| element.node_id == node_id)
        .ok_or_else(|| CdpError::ElementNotFound(format!("node {}", node_id)))?;
    element
        .bounds
        .clone()
        .ok_or_else(|| CdpError::ElementNotFound(format!("node {} missing bounds", node_id)))
}

fn normalize_key(key: &str) -> String {
    match key {
        "Return" => "Enter",
        other => other,
    }
    .to_string()
}

fn decode_base64(input: &str) -> Result<Vec<u8>, CdpError> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer: u32 = 0;
    let mut bits = 0;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return Err(CdpError::ProtocolError("invalid base64 data".to_string())),
        } as u32;

        buffer = (buffer << 6) | value;
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    Ok(output)
}

pub enum RuntimeCdpProvider {
    WebSocket(WebSocketCdpProvider),
    Stub(StubCdpProvider),
}

impl RuntimeCdpProvider {
    pub fn from_env() -> Self {
        let host = std::env::var("SOOTIE_CDP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("SOOTIE_CDP_PORT")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(9222);
        let mut connection = CdpConnection::new(&host, port);
        connection.ws_url = std::env::var("SOOTIE_CDP_WS_URL").ok();
        Self::WebSocket(WebSocketCdpProvider::with_connection(connection))
    }
}

#[async_trait]
impl CdpProvider for RuntimeCdpProvider {
    async fn connect(&mut self, connection: &CdpConnection) -> Result<(), CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => provider.connect(connection).await,
            RuntimeCdpProvider::Stub(provider) => provider.connect(connection).await,
        }
    }

    async fn list_pages(&self) -> Result<Vec<CdpPage>, CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => provider.list_pages().await,
            RuntimeCdpProvider::Stub(provider) => provider.list_pages().await,
        }
    }

    async fn find_elements(
        &self,
        page_id: &str,
        selector: &Selector,
    ) -> Result<Vec<CdpElement>, CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => {
                provider.find_elements(page_id, selector).await
            }
            RuntimeCdpProvider::Stub(provider) => provider.find_elements(page_id, selector).await,
        }
    }

    async fn click_element(
        &self,
        page_id: &str,
        node_id: u32,
        button: &str,
        click_count: u32,
    ) -> Result<(), CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => {
                provider
                    .click_element(page_id, node_id, button, click_count)
                    .await
            }
            RuntimeCdpProvider::Stub(provider) => {
                provider
                    .click_element(page_id, node_id, button, click_count)
                    .await
            }
        }
    }

    async fn type_text(
        &self,
        page_id: &str,
        node_id: Option<u32>,
        text: &str,
        clear_first: bool,
    ) -> Result<(), CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => {
                provider
                    .type_text(page_id, node_id, text, clear_first)
                    .await
            }
            RuntimeCdpProvider::Stub(provider) => {
                provider
                    .type_text(page_id, node_id, text, clear_first)
                    .await
            }
        }
    }

    async fn press_key(&self, page_id: &str, key: &str) -> Result<(), CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => provider.press_key(page_id, key).await,
            RuntimeCdpProvider::Stub(provider) => provider.press_key(page_id, key).await,
        }
    }

    async fn scroll(
        &self,
        page_id: &str,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    ) -> Result<(), CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => {
                provider.scroll(page_id, x, y, delta_x, delta_y).await
            }
            RuntimeCdpProvider::Stub(provider) => {
                provider.scroll(page_id, x, y, delta_x, delta_y).await
            }
        }
    }

    async fn screenshot(&self, page_id: &str) -> Result<ScreenshotData, CdpError> {
        match self {
            RuntimeCdpProvider::WebSocket(provider) => provider.screenshot(page_id).await,
            RuntimeCdpProvider::Stub(provider) => provider.screenshot(page_id).await,
        }
    }
}

pub async fn try_find_via_cdp(selector: &Selector) -> Result<Option<ResolvedTarget>, CdpError> {
    let provider = RuntimeCdpProvider::from_env();
    let pages = provider.list_pages().await?;
    let Some(page) = choose_page_for_selector(&pages, selector) else {
        return Ok(None);
    };

    let elements = provider.find_elements(&page.id, selector).await?;
    let (status, total_matches) = match elements.len() {
        0 => (MatchStatus::None, 0),
        1 => (MatchStatus::Unique, 1),
        count => (MatchStatus::Multiple, count as u32),
    };

    Ok(Some(ResolvedTarget {
        status,
        total_matches,
        app: None,
        window: Some(Window {
            id: page.id.clone(),
            title: page.title.clone(),
            index: 0,
            focused: true,
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            display_id: None,
        }),
        elements: elements
            .into_iter()
            .enumerate()
            .map(|(index, element)| Element {
                role: element.tag_name,
                name: element
                    .attributes
                    .get("aria-label")
                    .cloned()
                    .or_else(|| element.attributes.get("title").cloned())
                    .or_else(|| element.text_content.clone())
                    .unwrap_or_default(),
                text: element.text_content,
                id: element.attributes.get("id").cloned(),
                state: ElementState {
                    visible: true,
                    focused: None,
                    enabled: Some(true),
                },
                bounds: element.bounds.unwrap_or(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                }),
                index: index as u32,
            })
            .collect(),
    }))
}

fn choose_page_for_selector<'a>(pages: &'a [CdpPage], selector: &Selector) -> Option<&'a CdpPage> {
    if let Some(window) = selector.window.as_ref() {
        if let Some(id) = window.id.as_ref() {
            if let Some(page) = pages
                .iter()
                .find(|page| &page.id == id || page.window_id.as_ref() == Some(id))
            {
                return Some(page);
            }
        }

        if let Some(title) = window.title.as_ref() {
            if let Some(page) = pages.iter().find(|page| {
                page.title.to_lowercase().contains(&title.to_lowercase())
                    || page.url.to_lowercase().contains(&title.to_lowercase())
            }) {
                return Some(page);
            }
        }
    }

    if pages.len() == 1 {
        pages.first()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::ElementSelector;

    fn make_cdp_element(tag: &str, text: Option<&str>, attrs: Vec<(&str, &str)>) -> CdpElement {
        let mut attributes = std::collections::HashMap::new();
        for (k, v) in attrs {
            attributes.insert(k.to_string(), v.to_string());
        }

        CdpElement {
            node_id: 1,
            tag_name: tag.to_string(),
            text_content: text.map(|s| s.to_string()),
            attributes,
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 30.0,
            }),
        }
    }

    #[test]
    fn test_cdp_element_matches_role() {
        let element = make_cdp_element("button", Some("Submit"), vec![]);
        let selector = Selector::new().with_role("button");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_mismatches_role() {
        let element = make_cdp_element("div", Some("Submit"), vec![]);
        let selector = Selector::new().with_role("button");
        assert!(!element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_matches_name_via_aria_label() {
        let element = make_cdp_element("button", None, vec![("aria-label", "Submit Form")]);
        let selector = Selector::new().with_name("Submit Form");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_matches_name_via_text() {
        let element = make_cdp_element("button", Some("Submit"), vec![]);
        let selector = Selector::new().with_name("Submit");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_matches_name_via_title() {
        let element = make_cdp_element("button", None, vec![("title", "Close dialog")]);
        let selector = Selector::new().with_name("Close dialog");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_matches_id() {
        let element = make_cdp_element("input", None, vec![("id", "email_input")]);
        let selector = Selector {
            app: None,
            window: None,
            element: ElementSelector {
                role: None,
                name: None,
                text: None,
                id: Some("email_input".to_string()),
                state: None,
            },
        };
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_mismatches_id() {
        let element = make_cdp_element("input", None, vec![("id", "other_input")]);
        let selector = Selector {
            app: None,
            window: None,
            element: ElementSelector {
                role: None,
                name: None,
                text: None,
                id: Some("email_input".to_string()),
                state: None,
            },
        };
        assert!(!element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_combined_match() {
        let element = make_cdp_element(
            "input",
            None,
            vec![("id", "email"), ("aria-label", "Email address")],
        );
        let selector = Selector {
            app: None,
            window: None,
            element: ElementSelector {
                role: Some("textfield".to_string()),
                name: Some("Email address".to_string()),
                text: None,
                id: Some("email".to_string()),
                state: None,
            },
        };
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_role_to_tag_mapping() {
        assert_eq!(map_role_to_tag("button"), "button");
        assert_eq!(map_role_to_tag("textfield"), "input");
        assert_eq!(map_role_to_tag("text_field"), "input");
        assert_eq!(map_role_to_tag("link"), "a");
        assert_eq!(map_role_to_tag("checkbox"), "input");
        assert_eq!(map_role_to_tag("combobox"), "select");
        assert_eq!(map_role_to_tag("dropdown"), "select");
        assert_eq!(map_role_to_tag("heading"), "h1");
        assert_eq!(map_role_to_tag("image"), "img");
        assert_eq!(map_role_to_tag("list"), "ul");
        assert_eq!(map_role_to_tag("listitem"), "li");
        assert_eq!(map_role_to_tag("unknown_role"), "unknown_role");
    }

    #[test]
    fn test_cdp_connection_endpoint_url() {
        let conn = CdpConnection::new("localhost", 9222);
        assert_eq!(conn.endpoint_url(), "http://localhost:9222");
    }

    #[test]
    fn test_cdp_page_serialize() {
        let page = CdpPage {
            id: "page_1".to_string(),
            title: "Google".to_string(),
            url: "https://google.com".to_string(),
            window_id: Some("win_1".to_string()),
        };

        let json = serde_json::to_string(&page).unwrap();
        assert!(json.contains("page_1"));
        assert!(json.contains("Google"));
    }

    #[test]
    fn test_cdp_element_serialize() {
        let element = make_cdp_element("button", Some("Click me"), vec![("id", "btn1")]);
        let json = serde_json::to_string(&element).unwrap();
        let deserialized: CdpElement = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tag_name, "button");
        assert_eq!(deserialized.text_content, Some("Click me".to_string()));
    }

    #[test]
    fn test_no_match_when_name_differs() {
        let element = make_cdp_element("button", Some("Cancel"), vec![]);
        let selector = Selector::new().with_name("Submit");
        assert!(!element.matches_selector(&selector));
    }

    #[test]
    fn test_empty_selector_matches_all() {
        let element = make_cdp_element("button", Some("Submit"), vec![]);
        let selector = Selector::new();
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_no_attributes() {
        let element = make_cdp_element("div", None, vec![]);
        let selector = Selector::new();
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_with_placeholder() {
        let element = make_cdp_element("input", None, vec![("placeholder", "Enter email")]);
        let selector = Selector::new().with_name("Enter email");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_name_not_found() {
        let element = make_cdp_element("input", None, vec![]);
        let selector = Selector::new().with_name("Submit");
        assert!(!element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_connection_with_ws_url() {
        let mut conn = CdpConnection::new("localhost", 9222);
        conn.ws_url = Some("ws://localhost:9222/devtools/browser/123".to_string());
        assert_eq!(conn.endpoint_url(), "http://localhost:9222");
        assert!(conn.ws_url.is_some());
    }

    #[test]
    fn test_cdp_page_serialize_full() {
        let page = CdpPage {
            id: "page_1".to_string(),
            title: "Google".to_string(),
            url: "https://google.com".to_string(),
            window_id: Some("win_1".to_string()),
        };
        let json = serde_json::to_string_pretty(&page).unwrap();
        let deserialized: CdpPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page.id, deserialized.id);
        assert_eq!(page.title, deserialized.title);
        assert_eq!(page.url, deserialized.url);
        assert_eq!(page.window_id, deserialized.window_id);
    }

    #[test]
    fn test_cdp_page_without_window_id() {
        let page = CdpPage {
            id: "page_1".to_string(),
            title: "Google".to_string(),
            url: "https://google.com".to_string(),
            window_id: None,
        };
        let json = serde_json::to_string(&page).unwrap();
        let deserialized: CdpPage = serde_json::from_str(&json).unwrap();
        assert_eq!(page, deserialized);
    }

    #[test]
    fn test_cdp_error_display() {
        let err = CdpError::ConnectionFailed("timeout".to_string());
        assert!(err.to_string().contains("connection failed"));

        let err = CdpError::PageNotFound("page_1".to_string());
        assert!(err.to_string().contains("page not found"));

        let err = CdpError::ElementNotFound("button".to_string());
        assert!(err.to_string().contains("element not found"));

        let err = CdpError::ProtocolError("invalid message".to_string());
        assert!(err.to_string().contains("CDP protocol error"));

        let err = CdpError::NotImplemented("stub".to_string());
        assert!(err.to_string().contains("not implemented"));
    }

    #[test]
    fn test_role_to_tag_all_mappings() {
        assert_eq!(map_role_to_tag("button"), "button");
        assert_eq!(map_role_to_tag("textfield"), "input");
        assert_eq!(map_role_to_tag("text_field"), "input");
        assert_eq!(map_role_to_tag("link"), "a");
        assert_eq!(map_role_to_tag("checkbox"), "input");
        assert_eq!(map_role_to_tag("radio"), "input");
        assert_eq!(map_role_to_tag("combobox"), "select");
        assert_eq!(map_role_to_tag("dropdown"), "select");
        assert_eq!(map_role_to_tag("heading"), "h1");
        assert_eq!(map_role_to_tag("image"), "img");
        assert_eq!(map_role_to_tag("list"), "ul");
        assert_eq!(map_role_to_tag("listitem"), "li");
        assert_eq!(map_role_to_tag("list_item"), "li");
        assert_eq!(map_role_to_tag("tab"), "tab");
        assert_eq!(map_role_to_tag("menu"), "menu");
        assert_eq!(map_role_to_tag("menuitem"), "menuitem");
        assert_eq!(map_role_to_tag("menu_item"), "menuitem");
        assert_eq!(map_role_to_tag("dialog"), "dialog");
        assert_eq!(map_role_to_tag("toolbar"), "toolbar");
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_connect() {
        let mut provider = StubCdpProvider;
        let conn = CdpConnection::new("localhost", 9222);
        assert!(provider.connect(&conn).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_list_pages() {
        let provider = StubCdpProvider;
        assert!(provider.list_pages().await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_find_elements() {
        let provider = StubCdpProvider;
        let selector = Selector::new().with_role("button");
        assert!(provider.find_elements("page_1", &selector).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_click_element() {
        let provider = StubCdpProvider;
        assert!(provider
            .click_element("page_1", 1, "left", 1)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_type_text() {
        let provider = StubCdpProvider;
        assert!(provider
            .type_text("page_1", Some(1), "hello", false)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_press_key() {
        let provider = StubCdpProvider;
        assert!(provider.press_key("page_1", "Return").await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_scroll() {
        let provider = StubCdpProvider;
        assert!(provider
            .scroll("page_1", 0.0, 0.0, 0.0, 100.0)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_screenshot() {
        let provider = StubCdpProvider;
        assert!(provider.screenshot("page_1").await.is_err());
    }

    #[test]
    fn test_cdp_element_with_multiple_attributes() {
        let element = make_cdp_element(
            "button",
            Some("Submit"),
            vec![
                ("id", "submit-btn"),
                ("class", "btn btn-primary"),
                ("aria-label", "Submit form"),
                ("title", "Click to submit"),
            ],
        );
        let selector = Selector::new().with_name("Submit form");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_matches_by_text_content() {
        let element = make_cdp_element("span", Some("Hello World"), vec![]);
        let selector = Selector::new().with_name("Hello");
        assert!(element.matches_selector(&selector));
    }

    #[test]
    fn test_cdp_element_no_bounds() {
        let mut element = make_cdp_element("button", Some("Submit"), vec![]);
        element.bounds = None;
        let json = serde_json::to_string(&element).unwrap();
        let deserialized: CdpElement = serde_json::from_str(&json).unwrap();
        assert!(deserialized.bounds.is_none());
    }

    #[test]
    fn test_websocket_cdp_provider_new() {
        let provider = WebSocketCdpProvider::new();
        assert!(provider.connection.is_none());
        assert!(provider.ws_url.is_none());
    }

    #[test]
    fn test_websocket_cdp_provider_default() {
        let provider = WebSocketCdpProvider::default();
        assert!(provider.connection.is_none());
        assert!(provider.ws_url.is_none());
    }

    #[tokio::test]
    async fn test_websocket_cdp_provider_list_pages_not_implemented() {
        let provider = WebSocketCdpProvider::new();
        assert!(provider.list_pages().await.is_err());
    }

    #[tokio::test]
    async fn test_websocket_cdp_provider_find_elements_not_implemented() {
        let provider = WebSocketCdpProvider::new();
        let selector = Selector::new().with_role("button");
        assert!(provider.find_elements("page_1", &selector).await.is_err());
    }

    #[tokio::test]
    async fn test_websocket_cdp_provider_click_element_not_implemented() {
        let provider = WebSocketCdpProvider::new();
        assert!(provider
            .click_element("page_1", 123, "left", 1)
            .await
            .is_err());
    }

    #[test]
    fn test_choose_page_for_selector_by_title() {
        let pages = vec![
            CdpPage {
                id: "page-1".to_string(),
                title: "Inbox - Gmail".to_string(),
                url: "https://mail.google.com".to_string(),
                window_id: None,
            },
            CdpPage {
                id: "page-2".to_string(),
                title: "GitHub".to_string(),
                url: "https://github.com".to_string(),
                window_id: None,
            },
        ];

        let selector = Selector::new().with_window("Gmail".into());
        let page = choose_page_for_selector(&pages, &selector).unwrap();
        assert_eq!(page.id, "page-1");
    }

    #[test]
    fn test_choose_page_for_selector_single_page() {
        let pages = vec![CdpPage {
            id: "page-1".to_string(),
            title: "Only Page".to_string(),
            url: "https://example.com".to_string(),
            window_id: None,
        }];

        let selector = Selector::new().with_role("button");
        let page = choose_page_for_selector(&pages, &selector).unwrap();
        assert_eq!(page.id, "page-1");
    }
}
