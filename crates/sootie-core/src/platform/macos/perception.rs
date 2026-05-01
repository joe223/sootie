use async_trait::async_trait;
use tracing::{debug, warn};

use crate::perception::{
    AppContext, Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::{
    App, Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector, Window,
};

use super::ax_fns::*;

pub struct MacPerceptionProvider;

impl MacPerceptionProvider {
    pub fn new() -> Self {
        Self
    }

    fn get_app_windows(&self, pid: i32) -> Vec<Window> {
        unsafe {
            let app_element = AXUIElementCreateApplication(pid);
            let mut windows = Vec::new();

            let window_refs = get_children(app_element);
            for (index, window_ref) in window_refs.iter().enumerate() {
                let role = get_string_attr(*window_ref, "AXRole").unwrap_or_default();
                if role != "AXWindow" {
                    continue;
                }

                let title = get_string_attr(*window_ref, "AXTitle").unwrap_or_default();
                let position = get_point_attr(*window_ref, "AXPosition");
                let size = get_size_attr(*window_ref, "AXSize");

                let bounds = match (position, size) {
                    (Some(pos), Some(sz)) => Bounds {
                        x: pos.x,
                        y: pos.y,
                        width: sz.width,
                        height: sz.height,
                    },
                    _ => Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                };

                windows.push(Window {
                    id: format!("win_{}", index),
                    title,
                    index: index as u32,
                    focused: index == 0,
                    bounds,
                });
            }

            windows
        }
    }

    fn find_elements_in_tree(
        &self,
        element: AXUIElementRef,
        selector: &Selector,
        results: &mut Vec<Element>,
        index: &mut u32,
    ) {
        unsafe {
            let role = get_string_attr(element, "AXRole").unwrap_or_default();
            let title = get_string_attr(element, "AXTitle").unwrap_or_default();
            let value = get_string_attr(element, "AXValue").unwrap_or_default();
            let desc = get_string_attr(element, "AXDescription").unwrap_or_default();
            let identifier = get_string_attr(element, "AXIdentifier").unwrap_or_default();

            let name = if !title.is_empty() {
                title.clone()
            } else if !desc.is_empty() {
                desc.clone()
            } else {
                String::new()
            };

            let matches_role = selector
                .element
                .role
                .as_ref()
                .map(|r| role.to_lowercase().contains(&r.to_lowercase()))
                .unwrap_or(true);

            let matches_name = selector
                .element
                .name
                .as_ref()
                .map(|n| {
                    name.to_lowercase().contains(&n.to_lowercase())
                        || value.to_lowercase().contains(&n.to_lowercase())
                })
                .unwrap_or(true);

            let matches_id = selector
                .element
                .id
                .as_ref()
                .map(|id| identifier == *id)
                .unwrap_or(true);

            let matches_text = selector
                .element
                .text
                .as_ref()
                .map(|t| value.contains(t.as_str()))
                .unwrap_or(true);

            if matches_role && matches_name && matches_id && matches_text {
                let position = get_point_attr(element, "AXPosition");
                let size = get_size_attr(element, "AXSize");

                let bounds = match (position, size) {
                    (Some(pos), Some(sz)) => Bounds {
                        x: pos.x,
                        y: pos.y,
                        width: sz.width,
                        height: sz.height,
                    },
                    _ => Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                };

                let focused = get_bool_attr(element, "AXFocused").unwrap_or(false);
                let enabled = get_bool_attr(element, "AXEnabled").unwrap_or(true);

                results.push(Element {
                    role: normalize_role(&role),
                    name,
                    text: if value.is_empty() { None } else { Some(value) },
                    id: if identifier.is_empty() {
                        None
                    } else {
                        Some(identifier)
                    },
                    state: ElementState {
                        visible: true,
                        focused: Some(focused),
                        enabled: Some(enabled),
                    },
                    bounds,
                    index: *index,
                });
                *index += 1;
            }

            let children = get_children(element);
            for child in children {
                self.find_elements_in_tree(child, selector, results, index);
            }
        }
    }
}

fn normalize_role(ax_role: &str) -> String {
    let role_lower = ax_role.to_lowercase();
    let role_str = role_lower.trim_start_matches("ax");
    match role_str {
        "button" => "button",
        "textfield" | "textarea" => "textfield",
        "link" => "link",
        "checkbox" => "checkbox",
        "radiobutton" => "radio",
        "combobox" | "popupbutton" => "combobox",
        "statictext" => "text",
        "image" => "image",
        "list" => "list",
        "row" => "listitem",
        "tab" => "tab",
        "menu" => "menu",
        "menuitem" => "menuitem",
        "dialog" | "sheet" => "dialog",
        "toolbar" => "toolbar",
        "window" => "window",
        "group" => "group",
        "scrollarea" => "scrollarea",
        "slider" => "slider",
        "progressindicator" => "progressbar",
        "busyindicator" => "busyindicator",
        _ => role_str,
    }
    .to_string()
}

#[async_trait]
impl PerceptionProvider for MacPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        debug!("Getting macOS context");
        Ok(Context { apps: vec![] })
    }

    async fn find(&self, selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
        Err(PerceptionError::NotImplemented(
            "macOS find not yet fully implemented".to_string(),
        ))
    }

    async fn inspect(&self, _selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        debug!("Inspecting element");
        Err(PerceptionError::NotImplemented(
            "macOS inspect not yet fully implemented".to_string(),
        ))
    }

    async fn wait(
        &self,
        _selector: &Selector,
        _condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        debug!("Waiting for element");
        Err(PerceptionError::NotImplemented(
            "macOS wait not yet fully implemented".to_string(),
        ))
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        _region: Option<&Bounds>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        Err(PerceptionError::NotImplemented(
            "macOS screenshot not yet fully implemented".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_role() {
        assert_eq!(normalize_role("AXButton"), "button");
        assert_eq!(normalize_role("AXTextField"), "textfield");
        assert_eq!(normalize_role("AXLink"), "link");
        assert_eq!(normalize_role("AXCheckBox"), "checkbox");
        assert_eq!(normalize_role("AXWindow"), "window");
        assert_eq!(normalize_role("AXGroup"), "group");
    }

    #[test]
    fn test_mac_provider_creation() {
        let provider = MacPerceptionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }
}
