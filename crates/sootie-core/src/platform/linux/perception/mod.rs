mod context;
mod find;
mod screenshot;

use async_trait::async_trait;
use tracing::debug;

use crate::cdp::try_find_via_cdp;
use crate::perception::{
    Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::{Bounds, Selector};

pub struct LinuxPerceptionProvider;

impl LinuxPerceptionProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PerceptionProvider for LinuxPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        debug!("Getting Linux context");
        context::get_running_apps()
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
        let resolved = self.find(selector).await?;
        if resolved.elements.is_empty() {
            return Err(PerceptionError::TargetNotFound(
                "no element matches selector".to_string(),
            ));
        }
        let element = resolved.elements[0].clone();
        Ok(DeepInspection {
            element,
            children: vec![],
            backend: "xdotool".to_string(),
            actions: vec!["click".to_string(), "type".to_string()],
            raw_metadata: None,
        })
    }

    async fn wait(
        &self,
        selector: &Selector,
        condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        debug!("Waiting for element");
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let timeout = Duration::from_millis(condition.timeout_ms);

        loop {
            let result = self.find(selector).await?;
            if !result.elements.is_empty() {
                let element = &result.elements[0];
                let want_visible = condition.state.get("visible").and_then(|v| v.as_bool());
                let want_enabled = condition.state.get("enabled").and_then(|v| v.as_bool());
                let want_focused = condition.state.get("focused").and_then(|v| v.as_bool());
                let visible_ok = want_visible.map_or(true, |v| element.state.visible == v);
                let enabled_ok = want_enabled.map_or(true, |v| element.state.enabled == Some(v));
                let focused_ok = want_focused.map_or(true, |v| element.state.focused == Some(v));
                if visible_ok && enabled_ok && focused_ok {
                    return Ok(WaitResult {
                        matched: true,
                        element: Some(element.clone()),
                        timed_out: false,
                    });
                }
            }

            if start.elapsed() >= timeout {
                return Ok(WaitResult {
                    matched: false,
                    element: None,
                    timed_out: true,
                });
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        region: Option<&Bounds>,
        display_id: Option<u32>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        screenshot::take_screenshot(region, display_id)
    }

    async fn find_apps(
        &self,
        _pattern: &str,
        _limit: Option<u32>,
    ) -> Result<FindAppsResult, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "find_apps not implemented for Linux".to_string(),
        ))
    }
}
