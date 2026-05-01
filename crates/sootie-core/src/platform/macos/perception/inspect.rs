use crate::perception::{DeepInspection, PerceptionError};
use crate::selector::{Bounds, Element, ElementState, Selector};

use super::find::find_elements;
use super::utils::normalize_role;
use super::super::ax_fns::*;

pub fn inspect_element(selector: &Selector) -> Result<DeepInspection, PerceptionError> {
    let resolved = find_elements(selector)?;

    if resolved.elements.is_empty() {
        return Err(PerceptionError::TargetNotFound(
            "no element matches selector".to_string(),
        ));
    }

    let element = resolved.elements[0].clone();

    let pid = super::context::get_pid_for_app_name(
        &resolved
            .app
            .map(|a| a.name)
            .unwrap_or_else(|| "System Events".to_string()),
    );

    let app_element = if pid > 0 {
        unsafe { AXUIElementCreateApplication(pid) }
    } else {
        unsafe { AXUIElementCreateSystemWide() }
    };

    let mut children = Vec::new();
    let mut child_index = 0u32;
    unsafe {
        collect_children(app_element, &mut children, &mut child_index, 3, 0);
    }

    let actions = vec!["click".to_string(), "hover".to_string()];

    Ok(DeepInspection {
        element,
        children,
        backend: "at_tree".to_string(),
        actions,
        raw_metadata: None,
    })
}

unsafe fn collect_children(
    element: AXUIElementRef,
    results: &mut Vec<Element>,
    index: &mut u32,
    max_depth: u32,
    current_depth: u32,
) {
    if current_depth >= max_depth {
        return;
    }

    let children = get_children(element);
    for child in children {
        let role = get_string_attr(child, "AXRole").unwrap_or_default();
        let title = get_string_attr(child, "AXTitle").unwrap_or_default();
        let value = get_string_attr(child, "AXValue").unwrap_or_default();
        let desc = get_string_attr(child, "AXDescription").unwrap_or_default();
        let identifier = get_string_attr(child, "AXIdentifier").unwrap_or_default();

        let name = if !title.is_empty() {
            title.clone()
        } else if !desc.is_empty() {
            desc.clone()
        } else {
            String::new()
        };

        let position = get_point_attr(child, "AXPosition");
        let size = get_size_attr(child, "AXSize");

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

        let focused = get_bool_attr(child, "AXFocused").unwrap_or(false);
        let enabled = get_bool_attr(child, "AXEnabled").unwrap_or(true);

        results.push(Element {
            role: normalize_role(&role),
            name,
            text: if value.is_empty() {
                None
            } else {
                Some(value)
            },
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

        collect_children(child, results, index, max_depth, current_depth + 1);
    }
}