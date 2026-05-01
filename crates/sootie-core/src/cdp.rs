use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::perception::ScreenshotData;
use crate::selector::{Bounds, Selector};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::ElementSelector;

    fn make_cdp_element(
        tag: &str,
        text: Option<&str>,
        attrs: Vec<(&str, &str)>,
    ) -> CdpElement {
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
        assert!(provider.click_element("page_1", 1, "left", 1).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_type_text() {
        let provider = StubCdpProvider;
        assert!(provider.type_text("page_1", Some(1), "hello", false).await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_press_key() {
        let provider = StubCdpProvider;
        assert!(provider.press_key("page_1", "Return").await.is_err());
    }

    #[tokio::test]
    async fn test_stub_cdp_provider_scroll() {
        let provider = StubCdpProvider;
        assert!(provider.scroll("page_1", 0.0, 0.0, 0.0, 100.0).await.is_err());
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
}
