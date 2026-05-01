use async_trait::async_trait;

use crate::perception::{
    Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData, WaitCondition,
    WaitResult,
};
use crate::selector::{Bounds, ResolvedTarget, Selector};

pub struct WindowsPerceptionProvider;

impl WindowsPerceptionProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PerceptionProvider for WindowsPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "Windows UI Automation not yet implemented".to_string(),
        ))
    }

    async fn find(&self, _selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "Windows UI Automation not yet implemented".to_string(),
        ))
    }

    async fn inspect(&self, _selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "Windows UI Automation not yet implemented".to_string(),
        ))
    }

    async fn wait(
        &self,
        _selector: &Selector,
        _condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "Windows UI Automation not yet implemented".to_string(),
        ))
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        _region: Option<&Bounds>,
    ) -> Result<ScreenshotData, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "Windows screenshot not yet implemented".to_string(),
        ))
    }
}

pub struct WindowsActionProvider;

impl WindowsActionProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl crate::action::ActionProvider for WindowsActionProvider {
    async fn click(
        &self,
        _action: &crate::action::ClickAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn r#type(
        &self,
        _action: &crate::action::TypeAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn press(
        &self,
        _action: &crate::action::PressAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn hotkey(
        &self,
        _action: &crate::action::HotkeyAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn scroll(
        &self,
        _action: &crate::action::ScrollAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn hover(
        &self,
        _action: &crate::action::HoverAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn drag(
        &self,
        _action: &crate::action::DragAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn focus(
        &self,
        _action: &crate::action::FocusAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }

    async fn window_op(
        &self,
        _action: &crate::action::WindowAction,
    ) -> Result<crate::action::ActionResult, crate::action::ActionError> {
        Err(crate::action::ActionError::NotImplemented(
            "Windows SendInput not yet implemented".to_string(),
        ))
    }
}
