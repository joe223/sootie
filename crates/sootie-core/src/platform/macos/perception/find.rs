use crate::perception::PerceptionError;
use crate::selector::{Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector};

use super::super::ax_fns::*;
use super::context::get_pid_for_app_name;
use super::utils::normalize_role;

pub fn find_elements(selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
    let pid = find_pid_for_app(selector);

    let app_element = if let Some(pid) = pid {
        unsafe { AXUIElementCreateApplication(pid) }
    } else {
        unsafe { AXUIElementCreateSystemWide() }
    };

    let mut results = Vec::new();
    let mut index = 0u32;

    unsafe {
        find_elements_in_tree(app_element, selector, &mut results, &mut index);
        release_ax_element(app_element);
    }

    let app_info = pid.and_then(|p| {
        let ctx = super::context::get_running_apps();
        ctx.apps
            .iter()
            .find(|a| get_pid_for_app_name(&a.app.name) == p)
            .map(|a| a.app.clone())
    });

    let (status, total_matches) = match results.len() {
        0 => (MatchStatus::None, 0),
        1 => (MatchStatus::Unique, 1),
        n => (MatchStatus::Multiple, n as u32),
    };

    Ok(ResolvedTarget {
        status,
        total_matches,
        app: app_info,
        window: None,
        elements: results,
    })
}

fn find_pid_for_app(selector: &Selector) -> Option<i32> {
    let ctx = super::context::get_running_apps();
    for app_ctx in &ctx.apps {
        if let Some(ref app_sel) = selector.app {
            if let Some(ref name) = app_sel.name {
                if !app_ctx
                    .app
                    .name
                    .to_lowercase()
                    .contains(&name.to_lowercase())
                {
                    continue;
                }
            }
            if let Some(ref bid) = app_sel.bundle_id {
                if !app_ctx
                    .app
                    .bundle_id
                    .to_lowercase()
                    .contains(&bid.to_lowercase())
                {
                    continue;
                }
            }
            if let Some(frontmost) = app_sel.is_frontmost {
                if app_ctx.app.is_frontmost != frontmost {
                    continue;
                }
            }
            return Some(get_pid_for_app_name(&app_ctx.app.name));
        }
    }

    if let Some(ref app_sel) = selector.app {
        if app_sel.is_frontmost == Some(true) {
            for app_ctx in &ctx.apps {
                if app_ctx.app.is_frontmost {
                    return Some(get_pid_for_app_name(&app_ctx.app.name));
                }
            }
        }
    }

    None
}

unsafe fn find_elements_in_tree(
    element: AXUIElementRef,
    selector: &Selector,
    results: &mut Vec<Element>,
    index: &mut u32,
) {
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
        find_elements_in_tree(child, selector, results, index);
        release_ax_element(child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selector::Selector;

    #[test]
    fn test_module_loads() {
        assert!(true);
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_find_elements_basic() {
        let selector = Selector::new().with_name("NonExistent");
        let result = find_elements(&selector);
        assert!(result.is_ok() || result.is_err());
        if let Ok(target) = result {
            assert_eq!(target.status, MatchStatus::None);
        }
    }
}
