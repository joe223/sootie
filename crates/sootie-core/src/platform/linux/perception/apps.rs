use std::fs;
use std::path::{Path, PathBuf};

use crate::perception::{AppInfo, FindAppsResult};

pub fn find_installed_apps(pattern: &str, limit: Option<u32>) -> FindAppsResult {
    let mut apps = Vec::new();
    for dir in application_dirs() {
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("desktop") {
                continue;
            }
            let Some(app) = parse_desktop_file(&path) else {
                continue;
            };
            if matches_pattern(pattern, &app.name)
                || app
                    .bundle_id
                    .as_deref()
                    .is_some_and(|id| matches_pattern(pattern, id))
            {
                apps.push(app);
            }
        }
    }

    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.path == b.path);

    let total = apps.len() as u32;
    let apps = if let Some(limit) = limit {
        apps.into_iter().take(limit as usize).collect()
    } else {
        apps
    };

    FindAppsResult { apps, total }
}

fn application_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/usr/share/applications")];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    dirs
}

fn parse_desktop_file(path: &Path) -> Option<AppInfo> {
    let content = fs::read_to_string(path).ok()?;
    parse_desktop_entry(&content, path)
}

fn parse_desktop_entry(content: &str, path: &Path) -> Option<AppInfo> {
    let mut name = None;
    let mut no_display = false;
    let mut hidden = false;

    for line in content.lines().map(str::trim) {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix("Name=") {
            name = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("NoDisplay=") {
            no_display = value.eq_ignore_ascii_case("true");
        } else if let Some(value) = line.strip_prefix("Hidden=") {
            hidden = value.eq_ignore_ascii_case("true");
        }
    }

    if no_display || hidden {
        return None;
    }

    let name = name?;
    let bundle_id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::to_string);
    Some(AppInfo {
        name,
        bundle_id,
        path: path.to_string_lossy().to_string(),
    })
}

fn matches_pattern(pattern: &str, value: &str) -> bool {
    let pattern = pattern.trim_matches('*').to_lowercase();
    if pattern.is_empty() {
        return true;
    }
    value.to_lowercase().contains(&pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_desktop_entry() {
        let app = parse_desktop_entry(
            "[Desktop Entry]\nName=Visual Studio Code\nExec=code\n",
            Path::new("/usr/share/applications/code.desktop"),
        )
        .unwrap();

        assert_eq!(app.name, "Visual Studio Code");
        assert_eq!(app.bundle_id, Some("code".to_string()));
        assert_eq!(app.path, "/usr/share/applications/code.desktop");
    }

    #[test]
    fn test_parse_desktop_entry_skips_hidden_apps() {
        let app = parse_desktop_entry(
            "[Desktop Entry]\nName=Hidden App\nNoDisplay=true\n",
            Path::new("/usr/share/applications/hidden.desktop"),
        );

        assert!(app.is_none());
    }

    #[test]
    fn test_matches_pattern_accepts_wildcards_as_substrings() {
        assert!(matches_pattern("*code*", "Visual Studio Code"));
        assert!(matches_pattern("Code", "Visual Studio Code"));
        assert!(!matches_pattern("Safari", "Visual Studio Code"));
    }
}
