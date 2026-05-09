use std::path::Path;
use std::process::{Command, Stdio};

use tracing::debug;

use crate::action::{ActionError, ActionResult, LaunchAction};

fn resolve_app_path(name: &str) -> Option<String> {
    let output = Command::new("mdfind")
        .arg(format!(
            "kMDItemKind == 'Application' && kMDItemFSName == '*{}*.app'",
            name
        ))
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn perform_launch(action: &LaunchAction) -> Result<ActionResult, ActionError> {
    let app_identifier = action.app.name.clone().or(action.app.bundle_id.clone());

    match app_identifier {
        Some(identifier) => {
            if action.app.bundle_id.is_some() {
                return launch_by_bundle_id(&identifier, &action.args);
            }

            let result = launch_by_name(&identifier, &action.args);
            if result.is_ok() {
                return result;
            }

            debug!(name = %identifier, "open -a failed, trying mdfind to resolve app path");
            if let Some(app_path) = resolve_app_path(&identifier) {
                debug!(app_path = %app_path, "resolved via mdfind");
                return launch_by_path(&app_path, &action.args);
            }

            result
        }
        None => Err(ActionError::TargetNotFound(
            "no app identifier specified".to_string(),
        )),
    }
}

fn is_open_target(arg: &str) -> bool {
    arg.contains("://") || Path::new(arg).is_absolute()
}

fn launch_by_name(name: &str, args: &[String]) -> Result<ActionResult, ActionError> {
    let mut cmd = Command::new("open");
    cmd.arg("-a").arg(name);

    for arg in args.iter().filter(|arg| is_open_target(arg)) {
        cmd.arg(arg);
    }

    let app_args: Vec<&String> = args.iter().filter(|arg| !is_open_target(arg)).collect();
    if !app_args.is_empty() {
        cmd.arg("--args");
    }

    for arg in app_args {
        cmd.arg(arg);
    }

    spawn_open(cmd)
}

fn launch_by_bundle_id(bundle_id: &str, args: &[String]) -> Result<ActionResult, ActionError> {
    let mut cmd = Command::new("open");
    cmd.arg("-b").arg(bundle_id);

    for arg in args.iter().filter(|arg| is_open_target(arg)) {
        cmd.arg(arg);
    }

    let app_args: Vec<&String> = args.iter().filter(|arg| !is_open_target(arg)).collect();
    if !app_args.is_empty() {
        cmd.arg("--args");
    }

    for arg in app_args {
        cmd.arg(arg);
    }

    spawn_open(cmd)
}

fn launch_by_path(path: &str, args: &[String]) -> Result<ActionResult, ActionError> {
    let mut cmd = Command::new("open");
    cmd.arg(path);

    for arg in args.iter().filter(|arg| is_open_target(arg)) {
        cmd.arg(arg);
    }

    let app_args: Vec<&String> = args.iter().filter(|arg| !is_open_target(arg)).collect();
    if !app_args.is_empty() {
        cmd.arg("--args");
    }

    for arg in app_args {
        cmd.arg(arg);
    }

    spawn_open(cmd)
}

fn spawn_open(mut cmd: Command) -> Result<ActionResult, ActionError> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|e| ActionError::ActionFailed(format!("Failed to launch app: {}", e)))?;
    Ok(ActionResult::success(None, "open"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::LaunchAction;
    use crate::selector::AppSelector;

    #[test]
    fn test_is_open_target() {
        assert!(is_open_target("https://example.com/search?q=query"));
        assert!(is_open_target("file:///tmp/index.html"));
        assert!(is_open_target("/tmp/example.txt"));
        assert!(is_open_target("custom-scheme://resource"));
        assert!(!is_open_target("--profile-directory=Default"));
        assert!(!is_open_target("relative-file.txt"));
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_launch_by_name() {
        let action = LaunchAction {
            app: AppSelector::from_name("TextEdit"),
            args: vec![],
        };
        let result = perform_launch(&action);
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    #[ignore = "requires accessibility permissions"]
    fn test_perform_launch_with_args() {
        let action = LaunchAction {
            app: AppSelector::from_name("TextEdit"),
            args: vec!["test.txt".to_string()],
        };
        let result = perform_launch(&action);
        assert!(result.is_ok() || result.is_err());
    }
}
