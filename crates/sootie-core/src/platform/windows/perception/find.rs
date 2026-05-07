use windows::Win32::Foundation::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::perception::PerceptionError;
use crate::selector::{Bounds, Element, ElementState, MatchStatus, ResolvedTarget, Selector};

pub fn find_elements(selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
    unsafe {
        let root = GetDesktopWindow();

        let mut elements = Vec::new();
        let mut index = 0u32;

        find_elements_in_window(root, selector, &mut elements, &mut index);

        let (status, total_matches) = match elements.len() {
            0 => (MatchStatus::None, 0),
            1 => (MatchStatus::Unique, 1),
            n => (MatchStatus::Multiple, n as u32),
        };

        Ok(ResolvedTarget {
            status,
            total_matches,
            app: None,
            window: None,
            elements,
        })
    }
}

unsafe fn find_elements_in_window(
    hwnd: HWND,
    selector: &Selector,
    results: &mut Vec<Element>,
    index: &mut u32,
) {
    let element = IAccessibleFromWindow(hwnd, OBJID_WINDOW.0 as i32, &GUID_NULL);
    if element.is_err() {
        return;
    }

    let accessible = element.unwrap();

    let role = accessible.accRole(&VARIANT::default());
    let name = accessible.accName(&VARIANT::default());

    let role_string = match role {
        VARIANT::I4(val) => role_to_string(val),
        _ => "unknown".to_string(),
    };

    let name_string = match name {
        VARIANT::VT_BSTR(val) => val.to_string(),
        _ => String::new(),
    };

    let matches_role = selector
        .element
        .role
        .as_ref()
        .map(|r| role_string.to_lowercase().contains(&r.to_lowercase()))
        .unwrap_or(true);

    let matches_name = selector
        .element
        .name
        .as_ref()
        .map(|n| name_string.to_lowercase().contains(&n.to_lowercase()))
        .unwrap_or(true);

    if matches_role && matches_name {
        let mut rect = windows::Win32::Graphics::Gdi::RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(hwnd, &mut rect);

        let bounds = Bounds {
            x: rect.left as f64,
            y: rect.top as f64,
            width: (rect.right - rect.left) as f64,
            height: (rect.bottom - rect.top) as f64,
        };

        let visible = IsWindowVisible(hwnd).as_bool();

        results.push(Element {
            role: role_string,
            name: name_string,
            text: None,
            id: Some(format!("win_{}", hwnd.0)),
            state: ElementState {
                visible,
                focused: Some(GetForegroundWindow() == hwnd),
                enabled: Some(true),
            },
            bounds,
            index: *index,
        });
        *index += 1;
    }

    let child_count = accessible.accChildCount();

    for i in 0..child_count {
        let child_variant = VARIANT::I4(i as i32);
        let child = accessible.accChild(&child_variant);

        if let VARIANT::VT_DISPATCH(val) = child {
            find_elements_in_accessible(val, selector, results, index);
        }
    }
}

unsafe fn find_elements_in_accessible(
    accessible: windows::core::IUnknown,
    selector: &Selector,
    results: &mut Vec<Element>,
    index: &mut u32,
) {
    let accessible: IAccessible = accessible.cast::<IAccessible>().unwrap();

    let role = accessible.accRole(&VARIANT::default());
    let name = accessible.accName(&VARIANT::default());

    let role_string = match role {
        VARIANT::I4(val) => role_to_string(val),
        _ => "unknown".to_string(),
    };

    let name_string = match name {
        VARIANT::VT_BSTR(val) => val.to_string(),
        _ => String::new(),
    };

    let matches_role = selector
        .element
        .role
        .as_ref()
        .map(|r| role_string.to_lowercase().contains(&r.to_lowercase()))
        .unwrap_or(true);

    let matches_name = selector
        .element
        .name
        .as_ref()
        .map(|n| name_string.to_lowercase().contains(&n.to_lowercase()))
        .unwrap_or(true);

    if matches_role && matches_name {
        let location = accessible.accLocation(&VARIANT::default());

        let bounds = Bounds {
            x: location.left as f64,
            y: location.top as f64,
            width: (location.right - location.left) as f64,
            height: (location.bottom - location.top) as f64,
        };

        results.push(Element {
            role: role_string,
            name: name_string,
            text: None,
            id: None,
            state: ElementState {
                visible: true,
                focused: None,
                enabled: Some(true),
            },
            bounds,
            index: *index,
        });
        *index += 1;
    }

    let child_count = accessible.accChildCount();

    for i in 0..child_count {
        let child_variant = VARIANT::I4(i as i32);
        let child = accessible.accChild(&child_variant);

        if let VARIANT::VT_DISPATCH(val) = child {
            find_elements_in_accessible(val, selector, results, index);
        }
    }
}

fn role_to_string(role: i32) -> String {
    match role {
        ROLE_SYSTEM_TITLEBAR => "titlebar",
        ROLE_SYSTEM_MENUBAR => "menubar",
        ROLE_SYSTEM_SCROLLBAR => "scrollbar",
        ROLE_SYSTEM_GRIP => "grip",
        ROLE_SYSTEM_SOUND => "sound",
        ROLE_SYSTEM_CURSOR => "cursor",
        ROLE_SYSTEM_CARET => "caret",
        ROLE_SYSTEM_ALERT => "alert",
        ROLE_SYSTEM_WINDOW => "window",
        ROLE_SYSTEM_CLIENT => "client",
        ROLE_SYSTEM_MENUPOPUP => "menupopup",
        ROLE_SYSTEM_MENUITEM => "menuitem",
        ROLE_SYSTEM_TOOLTIP => "tooltip",
        ROLE_SYSTEM_APPLICATION => "application",
        ROLE_SYSTEM_DOCUMENT => "document",
        ROLE_SYSTEM_PANE => "pane",
        ROLE_SYSTEM_CHART => "chart",
        ROLE_SYSTEM_DIALOG => "dialog",
        ROLE_SYSTEM_BORDER => "border",
        ROLE_SYSTEM_GROUPING => "grouping",
        ROLE_SYSTEM_SEPARATOR => "separator",
        ROLE_SYSTEM_TOOLBAR => "toolbar",
        ROLE_SYSTEM_STATUSBAR => "statusbar",
        ROLE_SYSTEM_TABLE => "table",
        ROLE_SYSTEM_COLUMNHEADER => "columnheader",
        ROLE_SYSTEM_ROWHEADER => "rowheader",
        ROLE_SYSTEM_COLUMN => "column",
        ROLE_SYSTEM_ROW => "row",
        ROLE_SYSTEM_CELL => "cell",
        ROLE_SYSTEM_LINK => "link",
        ROLE_SYSTEM_HELPBALLOON => "helpballoon",
        ROLE_SYSTEM_CHARACTER => "character",
        ROLE_SYSTEM_LIST => "list",
        ROLE_SYSTEM_LISTITEM => "listitem",
        ROLE_SYSTEM_OUTLINE => "outline",
        ROLE_SYSTEM_OUTLINEITEM => "outlineitem",
        ROLE_SYSTEM_PAGETAB => "pagetab",
        ROLE_SYSTEM_PROPERTYPAGE => "propertypage",
        ROLE_SYSTEM_INDICATOR => "indicator",
        ROLE_SYSTEM_GRAPHIC => "graphic",
        ROLE_SYSTEM_STATICTEXT => "text",
        ROLE_SYSTEM_TEXT => "textfield",
        ROLE_SYSTEM_PUSHBUTTON => "button",
        ROLE_SYSTEM_CHECKBUTTON => "checkbox",
        ROLE_SYSTEM_RADIOBUTTON => "radio",
        ROLE_SYSTEM_COMBOBOX => "combobox",
        ROLE_SYSTEM_DROPLIST => "droplist",
        ROLE_SYSTEM_PROGRESSBAR => "progressbar",
        ROLE_SYSTEM_DIAL => "dial",
        ROLE_SYSTEM_HOTKEYFIELD => "hotkeyfield",
        ROLE_SYSTEM_SLIDER => "slider",
        ROLE_SYSTEM_SPINBUTTON => "spinbutton",
        ROLE_SYSTEM_DIAGRAM => "diagram",
        ROLE_SYSTEM_ANIMATION => "animation",
        ROLE_SYSTEM_BUTTONDROPDOWN => "buttondropdown",
        ROLE_SYSTEM_BUTTONMENU => "buttonmenu",
        ROLE_SYSTEM_BUTTONDROPDOWNGRID => "buttondropdowngrid",
        ROLE_SYSTEM_WHITESPACE => "whitespace",
        ROLE_SYSTEM_PAGETABLIST => "pagetablist",
        ROLE_SYSTEM_CLOCK => "clock",
        ROLE_SYSTEM_SPLITBUTTON => "splitbutton",
        ROLE_SYSTEM_IPADDRESS => "ipaddress",
        ROLE_SYSTEM_OUTLINEBUTTON => "outlinebutton",
        _ => "unknown",
    }
    .to_string()
}
