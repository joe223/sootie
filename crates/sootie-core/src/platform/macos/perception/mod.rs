mod context;
mod find;
mod inspect;
mod wait;
mod screenshot;
mod utils;

use async_trait::async_trait;
use tracing::debug;

use crate::perception::{
    Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData,
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

    async fn find(&self, selector: &Selector) -> Result<crate::selector::ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_provider_creation() {
        let provider = MacPerceptionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }
}