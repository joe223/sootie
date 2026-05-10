use std::fs;
use std::path::{Path, PathBuf};

use crate::perception::{AppInfo, FindAppsResult};

pub fn find_installed_apps(pattern: &str, limit: Option<u32>) -> FindAppsResult {
    let mut apps = Vec::new();
    for dir in application_dirs() {
        collect_apps(&dir, pattern, &mut apps);
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
    let mut dirs = Vec::new();
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        dirs.push(PathBuf::from(program_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(app_data) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(app_data).join("Microsoft\\Windows\\Start Menu\\Programs"));
    }
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(program_files));
    }
    if let Some(program_files_x86) = std::env::var_os("ProgramFiles(x86)") {
        dirs.push(PathBuf::from(program_files_x86));
    }
    dirs
}

fn collect_apps(dir: &Path, pattern: &str, apps: &mut Vec<AppInfo>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_apps(&path, pattern, apps);
            continue;
        }

        if !is_app_candidate(&path) {
            continue;
        }

        let Some(app) = app_info_from_path(&path) else {
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

fn is_app_candidate(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(|ext| ext.to_lowercase()),
        Some(ext) if ext == "lnk" || ext == "exe"
    )
}

fn app_info_from_path(path: &Path) -> Option<AppInfo> {
    let stem = path.file_stem()?.to_str()?.to_string();
    Some(AppInfo {
        name: stem,
        bundle_id: path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_string),
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
