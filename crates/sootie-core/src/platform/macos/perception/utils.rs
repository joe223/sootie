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
        assert_eq!(normalize_role("AXLink"), "link");
        assert_eq!(normalize_role("AXCheckBox"), "checkbox");
        assert_eq!(normalize_role("AXWindow"), "window");
        assert_eq!(normalize_role("AXGroup"), "group");
    }
}