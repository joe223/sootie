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
mod utils;
mod keyboard;
mod mouse;

use async_trait::async_trait;

use crate::action::{
    ActionError, ActionProvider, ActionResult, ClickAction, DragAction, FocusAction,
    HotkeyAction, HoverAction, LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction,
};

use super::perception::MacPerceptionProvider;

pub struct MacActionProvider {
    perception: MacPerceptionProvider,
}

impl MacActionProvider {
    pub fn new() -> Self {
        Self {
            perception: MacPerceptionProvider::new(),
        }
    }
}

#[async_trait]
impl ActionProvider for MacActionProvider {
    async fn click(&self, action: &ClickAction) -> Result<ActionResult, ActionError> {
        click::perform_click(action, &self.perception).await
    }

    async fn r#type(&self, action: &TypeAction) -> Result<ActionResult, ActionError> {
        type_text::perform_type(action, &self.perception).await
    }

    async fn press(&self, action: &PressAction) -> Result<ActionResult, ActionError> {
        press::perform_press(action)
    }

    async fn hotkey(&self, action: &HotkeyAction) -> Result<ActionResult, ActionError> {
        hotkey::perform_hotkey(action)
    }

    async fn scroll(&self, action: &ScrollAction) -> Result<ActionResult, ActionError> {
        scroll::perform_scroll(action, &self.perception).await
    }

    async fn hover(&self, action: &HoverAction) -> Result<ActionResult, ActionError> {
        hover::perform_hover(action, &self.perception).await
    }

    async fn drag(&self, action: &DragAction) -> Result<ActionResult, ActionError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mac_action_provider_creation() {
        let provider = MacActionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }
}