mod context;
mod find;
mod screenshot;

use async_trait::async_trait;
use tracing::debug;

use crate::perception::{
    Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData,
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

    async fn find(&self, selector: &Selector) -> Result<crate::selector::ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
        find::find_elements(selector)
    }

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        debug!("Inspecting element");
        let resolved = self.find(selector).await?;
        if resolved.elements.is_empty() {
            return Err(PerceptionError::TargetNotFound("no element matches selector".to_string()));
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
                let visible_ok = want_visible.map_or(true, |v| element.state.visible == v);
                if visible_ok {
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
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        screenshot::take_screenshot(region)
    }
}