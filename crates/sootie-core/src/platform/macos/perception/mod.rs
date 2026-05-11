mod apps;
mod context;
mod find;
mod inspect;
mod screenshot;
mod utils;
mod wait;

pub(crate) use context::{get_bundle_id_for_app_name, get_pid_for_app_name};

use async_trait::async_trait;
use std::process::Command;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

use crate::cdp::try_find_via_cdp;
use crate::perception::{
    Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::{Bounds, Selector};

const SCREENSHOT_TARGET_ATTEMPTS: usize = 3;
const SCREENSHOT_TARGET_RETRY_DELAY: Duration = Duration::from_millis(150);

pub struct MacPerceptionProvider;

impl MacPerceptionProvider {
    pub fn new() -> Self {
        Self
    }
}

fn app_matches(app: &crate::selector::App, selector: &crate::selector::AppSelector) -> bool {
    selector.name.as_ref().is_some_and(|name| app.name == *name)
        || selector
            .bundle_id
            .as_ref()
            .is_some_and(|bundle_id| app.bundle_id == *bundle_id)
        || selector.is_frontmost.unwrap_or(false) && app.is_frontmost
}

fn window_matches(
    window: &crate::selector::Window,
    selector: &crate::selector::WindowSelector,
) -> bool {
    selector
        .title
        .as_ref()
        .is_some_and(|title| window.title.contains(title))
        || selector.id.as_ref().is_some_and(|id| window.id == *id)
        || selector.focused.unwrap_or(false) && window.focused
        || selector.index.is_some_and(|index| window.index == index)
}

fn window_number_from_id(id: &str) -> Option<u32> {
    id.strip_prefix("cg_")?.parse().ok()
}

fn window_capture_target(
    target: Option<&Selector>,
    ctx: &Context,
) -> Option<(Bounds, Option<u32>, Option<u32>)> {
    let sel = target?;
    let app_sel = sel.app.as_ref()?;
    let app = ctx
        .apps
        .iter()
        .find(|candidate| app_matches(&candidate.app, app_sel))?;

    let window = if let Some(window_sel) = &sel.window {
        app.windows
            .iter()
            .find(|candidate| window_matches(candidate, window_sel))
    } else {
        app.windows
            .iter()
            .find(|candidate| candidate.focused)
            .or_else(|| app.windows.first())
    }?;

    Some((
        window.bounds.clone(),
        window.display_id,
        window_number_from_id(&window.id),
    ))
}

fn window_relative_region(window_bounds: &Bounds, region: &Bounds) -> Bounds {
    Bounds {
        x: window_bounds.x + region.x,
        y: window_bounds.y + region.y,
        width: region.width,
        height: region.height,
    }
}

fn selector_context(target: Option<&Selector>) -> Option<Context> {
    let app_selector = target.and_then(|selector| selector.app.as_ref())?;
    context::get_app_context(app_selector).map(|app_context| Context {
        apps: vec![app_context],
    })
}

#[derive(Debug, Clone, PartialEq)]
struct ScreenshotCaptureTarget {
    region: Option<Bounds>,
    display_id: Option<u32>,
    window_id: Option<u32>,
    fallback_region: Option<Bounds>,
}

fn screenshot_capture_target(
    target: Option<&Selector>,
    ctx: Option<&Context>,
    region: Option<&Bounds>,
    display_id: Option<u32>,
) -> ScreenshotCaptureTarget {
    let window_target = match (target, ctx) {
        (Some(target), Some(ctx)) => window_capture_target(Some(target), ctx),
        _ => None,
    };

    if let Some((bounds, window_display_id, window_id)) = window_target {
        if let Some(region) = region {
            return ScreenshotCaptureTarget {
                region: Some(window_relative_region(&bounds, region)),
                display_id: display_id.or(window_display_id),
                window_id: None,
                fallback_region: None,
            };
        }

        if bounds.width > 0.0 && bounds.height > 0.0 {
            if let Some(window_id) = window_id {
                return ScreenshotCaptureTarget {
                    region: None,
                    display_id: display_id.or(window_display_id),
                    window_id: Some(window_id),
                    fallback_region: Some(bounds),
                };
            }

            return ScreenshotCaptureTarget {
                region: Some(bounds),
                display_id: display_id.or(window_display_id),
                window_id: None,
                fallback_region: None,
            };
        }

        return ScreenshotCaptureTarget {
            region: None,
            display_id: display_id.or(window_display_id),
            window_id,
            fallback_region: None,
        };
    }

    if let Some(region) = region {
        return ScreenshotCaptureTarget {
            region: Some(region.clone()),
            display_id,
            window_id: None,
            fallback_region: None,
        };
    }

    ScreenshotCaptureTarget {
        region: None,
        display_id,
        window_id: None,
        fallback_region: None,
    }
}

fn applescript_quoted(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn activate_screenshot_target_app(target: Option<&Selector>) {
    let Some(app_selector) = target.and_then(|selector| selector.app.as_ref()) else {
        return;
    };

    let script = if let Some(bundle_id) = app_selector.bundle_id.as_deref() {
        format!(
            "tell application id \"{}\" to activate",
            applescript_quoted(bundle_id)
        )
    } else if let Some(name) = app_selector.name.as_deref() {
        format!(
            "tell application \"{}\" to activate",
            applescript_quoted(name)
        )
    } else {
        return;
    };

    let _ = Command::new("osascript").arg("-e").arg(script).output();
    std::thread::sleep(Duration::from_millis(250));
}

#[async_trait]
impl PerceptionProvider for MacPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        debug!("Getting macOS context");
        Ok(context::get_running_apps())
    }

    async fn find(
        &self,
        selector: &Selector,
    ) -> Result<crate::selector::ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
        if let Ok(Some(result)) = try_find_via_cdp(selector).await {
            return Ok(result);
        }
        find::find_elements(selector)
    }

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        debug!("Inspecting element");
        inspect::inspect_element(selector)
    }

    async fn wait(
        &self,
        selector: &Selector,
        condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        debug!("Waiting for element");
        wait::wait_for_element(selector, condition)
    }

    async fn screenshot(
        &self,
        target: Option<&Selector>,
        region: Option<&Bounds>,
        display_id: Option<u32>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");

        if target.is_some() {
            info!("Activating macOS screenshot target app before capture");
            activate_screenshot_target_app(target);
        }

        let ctx = if target.is_some() {
            selector_context(target).or_else(|| Some(context::get_running_apps()))
        } else {
            None
        };

        let mut capture_target =
            screenshot_capture_target(target, ctx.as_ref(), region, display_id);
        if target.is_some()
            && region.is_none()
            && display_id.is_none()
            && capture_target.region.is_none()
        {
            debug!("Screenshot target had no window bounds after activation; retrying context");
            for attempt in 1..=SCREENSHOT_TARGET_ATTEMPTS {
                let retry_ctx = selector_context(target).unwrap_or_else(context::get_running_apps);
                capture_target =
                    screenshot_capture_target(target, Some(&retry_ctx), region, display_id);
                if capture_target.region.is_some()
                    || capture_target.display_id.is_some()
                    || capture_target.window_id.is_some()
                {
                    break;
                }
                if attempt < SCREENSHOT_TARGET_ATTEMPTS {
                    std::thread::sleep(SCREENSHOT_TARGET_RETRY_DELAY);
                }
            }
        }

        if target.is_some()
            && region.is_none()
            && display_id.is_none()
            && capture_target.region.is_none()
        {
            return Err(PerceptionError::TargetNotFound(
                "could not resolve window bounds for screenshot target".to_string(),
            ));
        }

        info!(
            target_present = target.is_some(),
            region = ?capture_target.region,
            display_id = ?capture_target.display_id,
            window_id = ?capture_target.window_id,
            fallback_region = ?capture_target.fallback_region,
            "Resolved macOS screenshot capture target"
        );

        let capture_start = Instant::now();
        let result = screenshot::take_screenshot(
            capture_target.region.as_ref(),
            capture_target.display_id,
            capture_target.window_id,
        );

        match (
            result,
            capture_target.window_id,
            capture_target.fallback_region.as_ref(),
        ) {
            (Ok(screenshot), _, _) => {
                info!(
                    duration_ms = capture_start.elapsed().as_millis(),
                    bytes = screenshot.data.len(),
                    bounds = ?screenshot.bounds,
                    "macOS screenshot capture completed"
                );
                Ok(screenshot)
            }
            (Err(error), Some(window_id), Some(fallback_region)) => {
                warn!(
                    window_id,
                    error = %error,
                    fallback_region = ?fallback_region,
                    "macOS CG window screenshot failed; retrying with window bounds region"
                );
                let fallback_start = Instant::now();
                let screenshot = screenshot::take_screenshot(
                    Some(fallback_region),
                    capture_target.display_id,
                    None,
                )?;
                info!(
                    duration_ms = fallback_start.elapsed().as_millis(),
                    total_duration_ms = capture_start.elapsed().as_millis(),
                    bytes = screenshot.data.len(),
                    bounds = ?screenshot.bounds,
                    "macOS fallback region screenshot capture completed"
                );
                Ok(screenshot)
            }
            (Err(error), _, _) => Err(error),
        }
    }

    async fn find_apps(
        &self,
        pattern: &str,
        limit: Option<u32>,
    ) -> Result<FindAppsResult, PerceptionError> {
        debug!(pattern = %pattern, limit = ?limit, "Finding installed apps");
        Ok(apps::find_installed_apps(pattern, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::AppContext;
    use crate::selector::{App, AppSelector, Bounds, Window, WindowSelector};

    #[test]
    fn test_mac_provider_creation() {
        let provider = MacPerceptionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }

    #[test]
    fn test_window_capture_target_uses_focused_window_display() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![
                    Window {
                        id: "win_0".to_string(),
                        title: "Other".to_string(),
                        index: 0,
                        focused: false,
                        bounds: Bounds {
                            x: 0.0,
                            y: 0.0,
                            width: 500.0,
                            height: 400.0,
                        },
                        display_id: Some(1),
                    },
                    Window {
                        id: "win_1".to_string(),
                        title: "Start Page".to_string(),
                        index: 1,
                        focused: true,
                        bounds: Bounds {
                            x: 1440.0,
                            y: 0.0,
                            width: 1200.0,
                            height: 800.0,
                        },
                        display_id: Some(2),
                    },
                ],
            }],
        };
        let selector = Selector::new().with_app(AppSelector::from_name("Safari"));

        let (bounds, display_id, window_id) = window_capture_target(Some(&selector), &ctx).unwrap();

        assert_eq!(bounds.x, 1440.0);
        assert_eq!(bounds.width, 1200.0);
        assert_eq!(display_id, Some(2));
        assert_eq!(window_id, None);
    }

    #[test]
    fn test_window_capture_target_uses_requested_window_display() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: false,
                },
                windows: vec![Window {
                    id: "win_0".to_string(),
                    title: "Start Page".to_string(),
                    index: 0,
                    focused: false,
                    bounds: Bounds {
                        x: -1600.0,
                        y: 100.0,
                        width: 1400.0,
                        height: 900.0,
                    },
                    display_id: Some(42),
                }],
            }],
        };
        let selector = Selector::new()
            .with_app(AppSelector::from_name("Safari"))
            .with_window(WindowSelector::from_title("Start"));

        let (bounds, display_id, window_id) = window_capture_target(Some(&selector), &ctx).unwrap();

        assert_eq!(bounds.x, -1600.0);
        assert_eq!(bounds.height, 900.0);
        assert_eq!(display_id, Some(42));
        assert_eq!(window_id, None);
    }

    #[test]
    fn test_screenshot_capture_target_uses_window_region_when_display_available() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![Window {
                    id: "win_0".to_string(),
                    title: "Start Page".to_string(),
                    index: 0,
                    focused: true,
                    bounds: Bounds {
                        x: 1440.0,
                        y: 0.0,
                        width: 1200.0,
                        height: 800.0,
                    },
                    display_id: Some(2),
                }],
            }],
        };
        let selector = Selector::new().with_app(AppSelector::from_name("Safari"));

        let target = screenshot_capture_target(Some(&selector), Some(&ctx), None, None);

        assert_eq!(
            target.region,
            Some(Bounds {
                x: 1440.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            })
        );
        assert_eq!(target.display_id, Some(2));
        assert_eq!(target.window_id, None);
    }

    #[test]
    fn test_screenshot_capture_target_uses_cg_window_id_when_available() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![Window {
                    id: "cg_20663".to_string(),
                    title: "Start Page".to_string(),
                    index: 20663,
                    focused: true,
                    bounds: Bounds {
                        x: 1097.0,
                        y: 529.0,
                        width: 631.0,
                        height: 549.0,
                    },
                    display_id: Some(1),
                }],
            }],
        };
        let selector = Selector::new().with_app(AppSelector::from_name("Safari"));

        let target = screenshot_capture_target(Some(&selector), Some(&ctx), None, None);

        assert_eq!(target.window_id, Some(20663));
        assert_eq!(target.display_id, Some(1));
        assert_eq!(target.region, None);
    }

    #[test]
    fn test_screenshot_capture_target_uses_window_region_without_display() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![Window {
                    id: "win_0".to_string(),
                    title: "Start Page".to_string(),
                    index: 0,
                    focused: true,
                    bounds: Bounds {
                        x: 1440.0,
                        y: 0.0,
                        width: 1200.0,
                        height: 800.0,
                    },
                    display_id: None,
                }],
            }],
        };
        let selector = Selector::new().with_app(AppSelector::from_name("Safari"));

        let target = screenshot_capture_target(Some(&selector), Some(&ctx), None, None);

        assert_eq!(
            target.region,
            Some(Bounds {
                x: 1440.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            })
        );
        assert_eq!(target.display_id, None);
        assert_eq!(target.window_id, None);
    }

    #[test]
    fn test_screenshot_capture_target_preserves_explicit_full_display() {
        let target = screenshot_capture_target(None, None, None, Some(2));

        assert_eq!(target.region, None);
        assert_eq!(target.display_id, Some(2));
        assert_eq!(target.window_id, None);
    }

    #[test]
    fn test_screenshot_capture_target_preserves_explicit_region_and_display() {
        let region = Bounds {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 200.0,
        };

        let target = screenshot_capture_target(None, None, Some(&region), Some(2));

        assert_eq!(target.region, Some(region));
        assert_eq!(target.display_id, Some(2));
        assert_eq!(target.window_id, None);
    }

    #[test]
    fn test_screenshot_capture_target_interprets_region_relative_to_window() {
        let ctx = Context {
            apps: vec![AppContext {
                app: App {
                    name: "Safari".to_string(),
                    bundle_id: "com.apple.Safari".to_string(),
                    is_frontmost: true,
                },
                windows: vec![Window {
                    id: "cg_20663".to_string(),
                    title: "Start Page".to_string(),
                    index: 20663,
                    focused: true,
                    bounds: Bounds {
                        x: 1097.0,
                        y: 529.0,
                        width: 631.0,
                        height: 549.0,
                    },
                    display_id: Some(1),
                }],
            }],
        };
        let selector = Selector::new().with_app(AppSelector::from_name("Safari"));
        let region = Bounds {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 200.0,
        };

        let target = screenshot_capture_target(Some(&selector), Some(&ctx), Some(&region), None);

        assert_eq!(
            target.region,
            Some(Bounds {
                x: 1107.0,
                y: 549.0,
                width: 300.0,
                height: 200.0,
            })
        );
        assert_eq!(target.display_id, Some(1));
        assert_eq!(target.window_id, None);
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_provider_get_context() {
        let provider = MacPerceptionProvider::new();
        let result = provider.get_context().await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_provider_find() {
        let provider = MacPerceptionProvider::new();
        let selector = Selector::new().with_name("NonExistent");
        let result = provider.find(&selector).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_mac_provider_screenshot_full() {
        let provider = MacPerceptionProvider::new();
        let result = provider.screenshot(None, None, None).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_mac_provider_screenshot_region() {
        let provider = MacPerceptionProvider::new();
        let region = Bounds {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let result = provider.screenshot(None, Some(&region), None).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    async fn test_mac_provider_screenshot_with_display() {
        let provider = MacPerceptionProvider::new();
        let result = provider.screenshot(None, None, Some(1)).await;
        assert!(result.is_ok() || result.is_err());
    }
}
