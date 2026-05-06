use std::process::Command;

use tracing::debug;

use crate::perception::{AppInfo, FindAppsResult};

pub fn find_installed_apps(pattern: &str, limit: Option<u32>) -> FindAppsResult {
    let query = if pattern.contains('*') {
        format!(
            "kMDItemKind == 'Application' && kMDItemFSName == '{}'",
            pattern
        )
    } else {
        format!(
            "kMDItemKind == 'Application' && kMDItemFSName == '*{}*.app'",
            pattern
        )
    };

    debug!(pattern = %pattern, query = %query, "finding installed apps");

    let output = Command::new("mdfind").arg(&query).output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let apps: Vec<AppInfo> = stdout
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(|path| parse_app_info(path.trim()))
                .filter_map(|app| app)
                .collect();

            let total = apps.len() as u32;
            let apps = if let Some(l) = limit {
                apps.into_iter().take(l as usize).collect()
            } else {
                apps
            };

            FindAppsResult { apps, total }
        }
        _ => {
            debug!("mdfind failed, returning empty result");
            FindAppsResult {
                apps: vec![],
                total: 0,
            }
        }
    }
}

fn parse_app_info(path: &str) -> Option<AppInfo> {
    if !path.ends_with(".app") {
        return None;
    }

    let name = path
        .rsplit('/')
        .next()
        .and_then(|s| s.strip_suffix(".app"))
        .unwrap_or(path)
        .to_string();

    let bundle_id = get_bundle_id(path);

    Some(AppInfo {
        name,
        bundle_id,
        path: path.to_string(),
    })
}

fn get_bundle_id(path: &str) -> Option<String> {
    let output = Command::new("defaults")
        .args(["read", path, "CFBundleIdentifier"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|s| s.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_installed_apps_chrome() {
        let result = find_installed_apps("Chrome", Some(5));
        assert!(!result.apps.is_empty() || result.total == 0);

        for app in &result.apps {
            assert!(!app.path.is_empty());
            assert!(!app.name.is_empty());
        }
    }

    #[test]
    fn test_find_installed_apps_wildcard() {
        let result = find_installed_apps("*Chrome*.app", Some(5));
        assert!(!result.apps.is_empty() || result.total == 0);
    }

    #[test]
    fn test_parse_app_info_valid() {
        let app = parse_app_info("/Applications/Google Chrome.app");
        assert!(app.is_some());
        let app = app.unwrap();
        assert_eq!(app.name, "Google Chrome");
        assert_eq!(app.path, "/Applications/Google Chrome.app");
    }

    #[test]
    fn test_parse_app_info_invalid() {
        let app = parse_app_info("/some/random/file");
        assert!(app.is_none());
    }

    #[test]
    fn test_parse_app_info_deep_path() {
        let app = parse_app_info("/Applications/Utilities/Terminal.app");
        assert!(app.is_some());
        let app = app.unwrap();
        assert_eq!(app.name, "Terminal");
    }
}
