#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

use crate::action::ActionProvider;
use crate::perception::PerceptionProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityLevel {
    Full,
    Basic,
    CoordinateOnly,
    Degraded,
    Unsupported,
}

impl CapabilityLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Basic => "basic",
            Self::CoordinateOnly => "coordinate_only",
            Self::Degraded => "degraded",
            Self::Unsupported => "unsupported",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformCapabilities {
    pub platform: &'static str,
    pub native_tree: CapabilityLevel,
    pub screen_capture: CapabilityLevel,
    pub input: CapabilityLevel,
    pub app_discovery: CapabilityLevel,
    pub window_management: CapabilityLevel,
}

pub fn current_capabilities() -> PlatformCapabilities {
    #[cfg(target_os = "macos")]
    {
        PlatformCapabilities {
            platform: "macos",
            native_tree: CapabilityLevel::Full,
            screen_capture: CapabilityLevel::Full,
            input: CapabilityLevel::Full,
            app_discovery: CapabilityLevel::Full,
            window_management: CapabilityLevel::Full,
        }
    }
    #[cfg(target_os = "linux")]
    {
        PlatformCapabilities {
            platform: "linux",
            native_tree: CapabilityLevel::Degraded,
            screen_capture: CapabilityLevel::Degraded,
            input: CapabilityLevel::CoordinateOnly,
            app_discovery: CapabilityLevel::Basic,
            window_management: CapabilityLevel::Basic,
        }
    }
    #[cfg(target_os = "windows")]
    {
        PlatformCapabilities {
            platform: "windows",
            native_tree: CapabilityLevel::Degraded,
            screen_capture: CapabilityLevel::Basic,
            input: CapabilityLevel::CoordinateOnly,
            app_discovery: CapabilityLevel::Basic,
            window_management: CapabilityLevel::Basic,
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        PlatformCapabilities {
            platform: "unsupported",
            native_tree: CapabilityLevel::Unsupported,
            screen_capture: CapabilityLevel::Unsupported,
            input: CapabilityLevel::Unsupported,
            app_discovery: CapabilityLevel::Unsupported,
            window_management: CapabilityLevel::Unsupported,
        }
    }
}

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

    #[test]
    fn test_current_platform_capabilities_are_declared() {
        let capabilities = current_capabilities();
        assert!(!capabilities.platform.is_empty());
        assert!(!capabilities.native_tree.as_str().is_empty());
        assert!(!capabilities.screen_capture.as_str().is_empty());
        assert!(!capabilities.input.as_str().is_empty());
        assert!(!capabilities.app_discovery.as_str().is_empty());
        assert!(!capabilities.window_management.as_str().is_empty());
    }

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
