pub fn normalize_role(ax_role: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_role() {
        assert_eq!(normalize_role("AXButton"), "button");
        assert_eq!(normalize_role("AXTextField"), "textfield");
        assert_eq!(normalize_role("AXTextArea"), "textfield");
        assert_eq!(normalize_role("AXLink"), "link");
        assert_eq!(normalize_role("AXCheckBox"), "checkbox");
        assert_eq!(normalize_role("AXRadioButton"), "radio");
        assert_eq!(normalize_role("AXPopUpButton"), "combobox");
        assert_eq!(normalize_role("AXComboBox"), "combobox");
        assert_eq!(normalize_role("AXStaticText"), "text");
        assert_eq!(normalize_role("AXImage"), "image");
        assert_eq!(normalize_role("AXList"), "list");
        assert_eq!(normalize_role("AXRow"), "listitem");
        assert_eq!(normalize_role("AXTab"), "tab");
        assert_eq!(normalize_role("AXMenu"), "menu");
        assert_eq!(normalize_role("AXMenuItem"), "menuitem");
        assert_eq!(normalize_role("AXDialog"), "dialog");
        assert_eq!(normalize_role("AXSheet"), "dialog");
        assert_eq!(normalize_role("AXToolbar"), "toolbar");
        assert_eq!(normalize_role("AXWindow"), "window");
        assert_eq!(normalize_role("AXGroup"), "group");
        assert_eq!(normalize_role("AXScrollArea"), "scrollarea");
        assert_eq!(normalize_role("AXSlider"), "slider");
        assert_eq!(normalize_role("AXProgressIndicator"), "progressbar");
        assert_eq!(normalize_role("AXBusyIndicator"), "busyindicator");
    }

    #[test]
    fn test_normalize_role_unknown() {
        assert_eq!(normalize_role("AXCustomElement"), "customelement");
        assert_eq!(normalize_role("AXUnknown"), "unknown");
        assert_eq!(normalize_role("Custom"), "custom");
    }

    #[test]
    fn test_normalize_role_case_insensitive() {
        assert_eq!(normalize_role("axbutton"), "button");
        assert_eq!(normalize_role("AXBUTTON"), "button");
        assert_eq!(normalize_role("axTextField"), "textfield");
    }
}