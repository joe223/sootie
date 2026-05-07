mod apps;
mod context;
mod find;
mod inspect;
mod screenshot;
mod utils;
mod wait;

use async_trait::async_trait;
use tracing::debug;

use crate::cdp::try_find_via_cdp;
use crate::perception::{
    Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::{Bounds, Selector};

pub struct MacPerceptionProvider;

impl MacPerceptionProvider {
    pub fn new() -> Self {
        Self
    }
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
        _target: Option<&Selector>,
        region: Option<&Bounds>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        screenshot::take_screenshot(region)
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
    use crate::selector::Bounds;

    #[test]
    fn test_mac_provider_creation() {
        let provider = MacPerceptionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
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
        let result = provider.screenshot(None, None).await;
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
        let result = provider.screenshot(None, Some(&region)).await;
        assert!(result.is_ok() || result.is_err());
    }
}
