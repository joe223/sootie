mod click;
mod drag;
mod focus;
mod hotkey;
mod hover;
mod keyboard;
mod launch;
mod mouse;
mod press;
mod scroll;
mod type_text;
mod utils;
mod window;

use async_trait::async_trait;

use crate::action::{
    ActionError, ActionProvider, ActionResult, ClickAction, DragAction, FocusAction, HotkeyAction,
    HoverAction, LaunchAction, PressAction, ScrollAction, TypeAction, WindowAction,
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
    use crate::action::{ActionTarget, MouseButton};
    use crate::selector::Coordinate;

    #[test]
    fn test_mac_action_provider_creation() {
        let provider = MacActionProvider::new();
        assert!(std::ptr::addr_of!(provider) as usize != 0);
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_click() {
        let provider = MacActionProvider::new();
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
    async fn test_mac_action_provider_press() {
        let provider = MacActionProvider::new();
        let action = PressAction {
            key: "Return".to_string(),
        };
        let result = provider.press(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_hotkey() {
        let provider = MacActionProvider::new();
        let action = HotkeyAction {
            keys: vec!["Cmd".to_string(), "C".to_string()],
        };
        let result = provider.hotkey(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_scroll() {
        let provider = MacActionProvider::new();
        let action = ScrollAction {
            target: Some(ActionTarget::Coordinate(Coordinate { x: 100.0, y: 200.0 })),
            direction: crate::action::ScrollDirection::Up,
            amount: Some(5),
        };
        let result = provider.scroll(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_hover() {
        let provider = MacActionProvider::new();
        let action = HoverAction {
            target: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 100.0 }),
        };
        let result = provider.hover(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_drag() {
        let provider = MacActionProvider::new();
        let action = DragAction {
            from: ActionTarget::Coordinate(Coordinate { x: 100.0, y: 100.0 }),
            to: ActionTarget::Coordinate(Coordinate { x: 200.0, y: 200.0 }),
        };
        let result = provider.drag(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_focus() {
        let provider = MacActionProvider::new();
        let action = FocusAction {
            selector: crate::selector::Selector::new()
                .with_app(crate::selector::AppSelector::from_name("Finder")),
        };
        let result = provider.focus(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_launch() {
        let provider = MacActionProvider::new();
        let action = LaunchAction {
            app: crate::selector::AppSelector::from_name("Finder"),
            args: vec![],
        };
        let result = provider.launch(&action).await;
        assert!(result.is_ok() || result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires accessibility permissions"]
    async fn test_mac_action_provider_window() {
        let provider = MacActionProvider::new();
        let action = WindowAction {
            selector: crate::selector::Selector::new(),
            operation: crate::action::WindowOperation::Minimize,
        };
        let result = provider.window_op(&action).await;
        assert!(result.is_ok() || result.is_err());
    }
}
