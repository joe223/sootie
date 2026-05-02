use tracing::{debug, warn};

use crate::action::{ActionError, ActionTarget};
use crate::perception::{PerceptionError, PerceptionProvider};
use crate::selector::{Coordinate, Selector};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backend {
    AtTree,
    Cdp,
    Vision,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::AtTree => write!(f, "at_tree"),
            Backend::Cdp => write!(f, "cdp"),
            Backend::Vision => write!(f, "vision"),
        }
    }
}

pub struct Cascade<P: PerceptionProvider> {
    perception: P,
}

impl<P: PerceptionProvider> Cascade<P> {
    pub fn new(perception: P) -> Self {
        Self { perception }
    }

    pub async fn resolve_coordinate(
        &self,
        target: &ActionTarget,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        match target {
            ActionTarget::Coordinate(coord) => Ok((coord.clone(), None)),
            ActionTarget::Selector(selector) => {
                self.resolve_selector_coordinate(selector).await
            }
        }
    }

    async fn resolve_selector_coordinate(
        &self,
        selector: &Selector,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        debug!("Attempting structured resolution for selector");

        match self.perception.find(selector).await {
            Ok(result) if result.status == crate::selector::MatchStatus::Unique => {
                if let Some(element) = result.elements.first() {
                    let center = element.bounds.center();
                    debug!(
                        "Structured resolution succeeded at ({}, {})",
                        center.0, center.1
                    );
                    return Ok((
                        Coordinate {
                            x: center.0,
                            y: center.1,
                        },
                        Some(Backend::AtTree),
                    ));
                }
                Err(ActionError::TargetNotFound(
                    "found target but no elements".to_string(),
                ))
            }
            Ok(_) => {
                debug!("Structured resolution returned no unique match, falling back to vision");
                self.resolve_via_vision(selector).await
            }
            Err(PerceptionError::TargetNotFound(_)) => {
                debug!("Target not found in AT tree, falling back to vision");
                self.resolve_via_vision(selector).await
            }
            Err(e) => {
                warn!("Structured resolution error: {}, falling back to vision", e);
                self.resolve_via_vision(selector).await
            }
        }
    }

    async fn resolve_via_vision(
        &self,
        _selector: &Selector,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        debug!("Vision fallback not yet implemented");
        Err(ActionError::NotImplemented(
            "vision fallback requires vision provider".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::{
        Context, ScreenshotData, StubPerceptionProvider, WaitCondition, WaitResult,
    };
    use crate::selector::*;

    struct MockPerceptionProvider {
        find_result: Option<ResolvedTarget>,
    }

    impl MockPerceptionProvider {
        fn with_find_result(result: ResolvedTarget) -> Self {
            Self {
                find_result: Some(result),
            }
        }

        fn empty() -> Self {
            Self { find_result: None }
        }
    }

    #[async_trait::async_trait]
    impl PerceptionProvider for MockPerceptionProvider {
        async fn get_context(&self) -> Result<Context, PerceptionError> {
            Ok(Context { apps: vec![] })
        }

        async fn find(&self, _selector: &Selector) -> Result<ResolvedTarget, PerceptionError> {
            match &self.find_result {
                Some(r) => Ok(r.clone()),
                None => Err(PerceptionError::TargetNotFound("not found".to_string())),
            }
        }

        async fn inspect(&self, _selector: &Selector) -> Result<DeepInspection, PerceptionError> {
            Err(PerceptionError::NotImplemented("mock".to_string()))
        }

        async fn wait(
            &self,
            _selector: &Selector,
            _condition: &WaitCondition,
        ) -> Result<WaitResult, PerceptionError> {
            Err(PerceptionError::NotImplemented("mock".to_string()))
        }

        async fn screenshot(
            &self,
            _target: Option<&Selector>,
            _region: Option<&Bounds>,
        ) -> Result<ScreenshotData, PerceptionError> {
            Err(PerceptionError::NotImplemented("mock".to_string()))
        }
    }

    fn make_element(x: f64, y: f64, w: f64, h: f64) -> Element {
        Element {
            role: "button".to_string(),
            name: "Test".to_string(),
            text: None,
            id: None,
            state: ElementState {
                visible: true,
                focused: None,
                enabled: Some(true),
            },
            bounds: Bounds {
                x,
                y,
                width: w,
                height: h,
            },
            index: 0,
        }
    }

    fn make_resolved_target(element: Element) -> ResolvedTarget {
        ResolvedTarget {
            status: MatchStatus::Unique,
            total_matches: 1,
            app: None,
            window: None,
            elements: vec![element],
        }
    }

#[tokio::test]
    async fn test_resolve_selector_with_vision_fallback() {
        let provider = MockPerceptionProvider::empty();
        let cascade = Cascade::new(provider);

        let selector = Selector::new().with_name("Submit");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ActionError::NotImplemented(msg) => {
                assert!(msg.contains("vision"));
            }
            _ => panic!("Expected NotImplemented error"),
        }
    }

    #[tokio::test]
    async fn test_resolve_selector_no_unique_match() {
        let element = make_element(100.0, 200.0, 50.0, 30.0);
        let resolved = ResolvedTarget {
            status: MatchStatus::None,
            total_matches: 0,
            app: None,
            window: None,
            elements: vec![element],
        };
        let provider = MockPerceptionProvider::with_find_result(resolved);
        let cascade = Cascade::new(provider);

        let selector = Selector::new().with_role("button");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_selector_perception_error() {
        let provider = MockPerceptionProvider::empty();
        let cascade = Cascade::new(provider);

        let selector = Selector::new().with_name("NonExistent");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_backend_display() {
        assert_eq!(Backend::AtTree.to_string(), "at_tree");
        assert_eq!(Backend::Cdp.to_string(), "cdp");
        assert_eq!(Backend::Vision.to_string(), "vision");
    }

    #[tokio::test]
    async fn test_resolve_selector_unique_match() {
        let element = make_element(100.0, 200.0, 50.0, 20.0);
        let target = make_resolved_target(element);

        let provider = MockPerceptionProvider::with_find_result(target);
        let cascade = Cascade::new(provider);

        let selector = Selector::new().with_role("button").with_name("Test");
        let action_target = ActionTarget::Selector(selector);

        let (coord, backend) = cascade.resolve_coordinate(&action_target).await.unwrap();
        assert_eq!(coord.x, 125.0);
        assert_eq!(coord.y, 210.0);
        assert_eq!(backend, Some(Backend::AtTree));
    }

    #[tokio::test]
    async fn test_resolve_selector_fallback_on_not_found() {
        let provider = MockPerceptionProvider::empty();
        let cascade = Cascade::new(provider);

        let selector = Selector::new().with_role("button").with_name("Missing");
        let action_target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&action_target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_click_with_coordinate() {
        let provider = StubPerceptionProvider;
        let cascade = Cascade::new(provider);

        let target = ActionTarget::Coordinate(Coordinate { x: 50.0, y: 75.0 });
        let action = crate::action::ClickAction {
            target,
            button: Some(crate::action::MouseButton::Left),
            count: Some(1),
        };

        let (coord, _) = cascade.resolve_coordinate(&action.target).await.unwrap();
        assert_eq!(coord.x, 50.0);
        assert_eq!(coord.y, 75.0);
    }

    #[tokio::test]
    async fn test_resolve_hover_with_selector() {
        let element = make_element(300.0, 400.0, 80.0, 30.0);
        let target = make_resolved_target(element);

        let provider = MockPerceptionProvider::with_find_result(target);
        let cascade = Cascade::new(provider);

        let selector = Selector::new()
            .with_app(AppSelector::from_name("Chrome"))
            .with_role("link")
            .with_name("Home");

        let action_target = ActionTarget::Selector(selector);
        let (coord, backend) = cascade.resolve_coordinate(&action_target).await.unwrap();

        assert_eq!(coord.x, 340.0);
        assert_eq!(coord.y, 415.0);
        assert_eq!(backend, Some(Backend::AtTree));
    }
}
