mod click;
mod type_text;
mod press;
mod hotkey;
mod scroll;
mod hover;
mod drag;
mod focus;
mod launch;
mod window;
mod mouse;
mod keyboard;

use async_trait::async_trait;
use tracing::debug;

use crate::action::{
    ActionError, ActionProvider, ActionResult, ClickAction, DragAction, FocusAction,
    HotkeyAction, HoverAction, LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction,
};

use super::perception::WindowsPerceptionProvider;

pub struct WindowsActionProvider {
    perception: WindowsPerceptionProvider,
}

impl WindowsActionProvider {
    pub fn new() -> Self {
        Self {
            perception: WindowsPerceptionProvider::new(),
        }
    }
}

#[async_trait]
impl ActionProvider for WindowsActionProvider {
    async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError> {
        debug!("Performing click action");
        click::perform_click(action, &self.perception).await
    }

    async fn r#type(&self, action: &TypeAction) -> Result<ActionResult, ActionError> {
        debug!("Performing type action");
        type_text::perform_type(action, &self.perception).await
    }

    async fn press(&self, action: &PressAction) -> Result<ActionResult, ActionError> {
        debug!("Performing press action");
        press::perform_press(action)
    }

    async fn hotkey(&self, action: &HotkeyAction) -> Result<ActionResult, ActionError> {
        debug!("Performing hotkey action");
        hotkey::perform_hotkey(action)
    }

    async fn scroll(&self, action: &ScrollAction) -> Result<ActionResult, ActionError> {
        debug!("Performing scroll action");
        scroll::perform_scroll(action, &self.perception).await
    }

    async fn hover(&self, action: &HoverAction) -> Result<ActionResult, ActionError> {
        debug!("Performing hover action");
        hover::perform_hover(action, &self.perception).await
    }

    async fn drag(&self, action: &DragAction) -> Result<ActionResult, ActionError> {
        debug!("Performing drag action");
        drag::perform_drag(action, &self.perception).await
    }

async fn focus(&self, action: &FocusAction) -> Result<ActionResult, ActionError> {
        focus::perform_focus(action)
    }

    async fn launch(&self, action: &LaunchAction) -> Result<ActionResult, ActionError> {
        launch::perform_launch(action)
    }

    async fn window_op(&self, action: &WindowAction) -> Result<ActionResult, ActionError> {
        window::perform_window_op(action)
    }
}

    async fn window_op(&self, action: &WindowAction) -> Result<ActionResult, ActionError> {
        debug!("Performing window operation");
        window::perform_window_op(action)
    }
}