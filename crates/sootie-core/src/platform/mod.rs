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
        Box::new(linux::LinuxPerceptionProvider::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPerceptionProvider::new())
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
        Box::new(linux::LinuxActionProvider::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsActionProvider::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Box::new(crate::action::StubActionProvider)
    }
}
