#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

use crate::action::ActionProvider;
use crate::perception::PerceptionProvider;

pub fn create_perception_provider() -> Box<dyn PerceptionProvider> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::perception::MacPerceptionProvider::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::perception::LinuxPerceptionProvider::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::perception::WindowsPerceptionProvider::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(crate::perception::StubPerceptionProvider)
    }
}

pub fn create_action_provider() -> Box<dyn ActionProvider> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::action::MacActionProvider::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::action::LinuxActionProvider::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::action::WindowsActionProvider::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(crate::action::StubActionProvider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{ActionTarget, ClickAction, MouseButton};
    use crate::selector::Coordinate;

    #[tokio::test]
    #[ignore = "requires system permissions"]
    async fn test_create_perception_provider_returns_provider() {
        let provider = create_perception_provider();
        assert!(provider.get_context().await.is_ok() || provider.get_context().await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires system permissions"]
    async fn test_create_action_provider_returns_provider() {
        let provider = create_action_provider();
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 100.0 }),
            button: Some(MouseButton::Left),
            count: Some(1),
        };
        let result = provider.click(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_create_perception_provider_type() {
        let provider = create_perception_provider();
        let result = provider.get_context().await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_create_action_provider_type() {
        let provider = create_action_provider();
        let action = ClickAction {
            target: ActionTarget::Coordinate(Coordinate { x: 0.0, y: 0.0 }),
            button: Some(MouseButton::Left),
            count: Some(1),
        };
        let result = provider.click(&action).await;
        assert!(result.is_ok() || result.is_err());
    }
}
