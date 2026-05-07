use std::str::FromStr;
use tracing::{debug, warn};

use crate::action::{ActionError, ActionTarget};
use crate::cdp::try_find_via_cdp;
use crate::perception::{PerceptionError, PerceptionProvider};
use crate::selector::{Bounds, Coordinate, Selector};
use crate::vision::{RuntimeVisionProvider, VisionProvider, VisionRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Cdp,
    AtTree,
    Vision,
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Cdp => write!(f, "cdp"),
            Backend::AtTree => write!(f, "at_tree"),
            Backend::Vision => write!(f, "vision"),
        }
    }
}

impl FromStr for Backend {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "cdp" => Ok(Backend::Cdp),
            "at_tree" | "attree" | "at-tree" => Ok(Backend::AtTree),
            "vision" => Ok(Backend::Vision),
            _ => Err(format!("Unknown backend: {}", s)),
        }
    }
}

fn default_fallback_priority() -> Vec<Backend> {
    vec![Backend::Cdp, Backend::AtTree, Backend::Vision]
}

fn resolve_fallback_priority() -> Vec<Backend> {
    if let Ok(env_priority) = std::env::var("SOOTIE_FALLBACK_PRIORITY") {
        let parts: Vec<&str> = env_priority.split(',').map(|s| s.trim()).collect();
        let mut priority = Vec::new();
        for part in parts {
            if let Ok(backend) = Backend::from_str(part) {
                priority.push(backend);
            }
        }
        if !priority.is_empty() {
            return priority;
        }
    }
    default_fallback_priority()
}

pub struct Cascade<'a, P: PerceptionProvider, V: VisionProvider> {
    perception: &'a P,
    vision: Option<&'a V>,
    priority: Vec<Backend>,
}

impl<'a, P: PerceptionProvider, V: VisionProvider> Cascade<'a, P, V> {
    pub fn new(perception: &'a P, vision: Option<&'a V>) -> Self {
        Self {
            perception,
            vision,
            priority: resolve_fallback_priority(),
        }
    }

    pub fn with_priority(perception: &'a P, vision: Option<&'a V>, priority: Vec<Backend>) -> Self {
        Self {
            perception,
            vision,
            priority,
        }
    }

    pub async fn resolve_coordinate(
        &self,
        target: &ActionTarget,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        match target {
            ActionTarget::Coordinate(coord) => Ok((coord.clone(), None)),
            ActionTarget::Selector(selector) => self.resolve_selector_coordinate(selector).await,
        }
    }

    async fn resolve_selector_coordinate(
        &self,
        selector: &Selector,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        for backend in &self.priority {
            let result = self.try_backend(selector, backend).await;
            match result {
                Ok((coord, used_backend)) => {
                    if let Some(b) = used_backend {
                        debug!("Resolved via {} at ({}, {})", b, coord.x, coord.y);
                        return Ok((coord, Some(b)));
                    }
                }
                Err(ActionError::TargetNotFound(_)) => {
                    debug!("{} backend failed, trying next", backend);
                    continue;
                }
                Err(e) => {
                    warn!("{} backend error: {}, trying next", backend, e);
                    continue;
                }
            }
        }

        Err(ActionError::TargetNotFound(
            "All backends failed to resolve selector".to_string(),
        ))
    }

    async fn try_backend(
        &self,
        selector: &Selector,
        backend: &Backend,
    ) -> Result<(Coordinate, Option<Backend>), ActionError> {
        match backend {
            Backend::Cdp => self.try_cdp(selector).await,
            Backend::AtTree => self.try_at_tree(selector).await,
            Backend::Vision => self.try_vision(selector).await,
        }
    }

    async fn try_cdp(&self, selector: &Selector) -> Result<(Coordinate, Option<Backend>), ActionError> {
        debug!("Attempting CDP resolution for selector");

        match try_find_via_cdp(selector).await {
            Ok(Some(result)) if result.status == crate::selector::MatchStatus::Unique => {
                if let Some(element) = result.elements.first() {
                    let center = element.bounds.center();
                    return Ok((
                        Coordinate { x: center.0, y: center.1 },
                        Some(Backend::Cdp),
                    ));
                }
            }
            Ok(Some(_)) => {
                debug!("CDP returned no unique match");
            }
            Ok(None) => {}
            Err(error) => {
                warn!("CDP resolution error: {}", error);
            }
        }

        Err(ActionError::TargetNotFound("CDP failed".to_string()))
    }

    async fn try_at_tree(&self, selector: &Selector) -> Result<(Coordinate, Option<Backend>), ActionError> {
        debug!("Attempting AT tree resolution for selector");

        match self.perception.find(selector).await {
            Ok(result) if result.status == crate::selector::MatchStatus::Unique => {
                if let Some(element) = result.elements.first() {
                    let center = element.bounds.center();
                    return Ok((
                        Coordinate { x: center.0, y: center.1 },
                        Some(Backend::AtTree),
                    ));
                }
            }
            Ok(_) => {
                debug!("AT tree returned no unique match");
            }
            Err(PerceptionError::TargetNotFound(_)) => {}
            Err(e) => {
                warn!("AT tree error: {}", e);
            }
        }

        Err(ActionError::TargetNotFound("AT tree failed".to_string()))
    }

    async fn try_vision(&self, selector: &Selector) -> Result<(Coordinate, Option<Backend>), ActionError> {
        let Some(vision) = self.vision else {
            debug!("Vision backend not available");
            return Err(ActionError::NotImplemented(
                "vision backend requires vision provider".to_string(),
            ));
        };

        debug!("Attempting vision resolution for selector");

        let screenshot = self
            .perception
            .screenshot(Some(selector), selector_region(selector).as_ref(), None)
            .await
            .map_err(|e| ActionError::ActionFailed(format!("Screenshot failed: {}", e)))?;

        let request = VisionRequest {
            screenshot,
            target_description: describe_selector(selector),
            context: selector
                .app
                .as_ref()
                .and_then(|app| app.name.clone())
                .or_else(|| {
                    selector
                        .window
                        .as_ref()
                        .and_then(|window| window.title.clone())
                }),
        };

        let result = vision
            .detect(&request)
            .await
            .map_err(|e| ActionError::ActionFailed(format!("Vision failed: {}", e)))?;

        Ok((result.coordinate, Some(Backend::Vision)))
    }
}

pub async fn resolve_target_with_cascade<P: PerceptionProvider>(
    perception: &P,
    target: &ActionTarget,
) -> Result<(Coordinate, Option<Backend>), ActionError> {
    let vision = RuntimeVisionProvider::from_env();
    let cascade = Cascade::new(perception, Some(&vision));
    cascade.resolve_coordinate(target).await
}

pub async fn resolve_target_with_priority<P: PerceptionProvider>(
    perception: &P,
    target: &ActionTarget,
    priority: Vec<Backend>,
) -> Result<(Coordinate, Option<Backend>), ActionError> {
    let vision = RuntimeVisionProvider::from_env();
    let cascade = Cascade::with_priority(perception, Some(&vision), priority);
    cascade.resolve_coordinate(target).await
}

pub fn get_fallback_priority() -> Vec<Backend> {
    resolve_fallback_priority()
}

fn selector_region(selector: &Selector) -> Option<Bounds> {
    selector.window.as_ref().and_then(|window| {
        window.id.as_ref()?;
        None
    })
}

fn describe_selector(selector: &Selector) -> String {
    let mut parts = Vec::new();

    if let Some(app) = selector.app.as_ref().and_then(|app| app.name.as_ref()) {
        parts.push(format!("app={}", app));
    }
    if let Some(window) = selector
        .window
        .as_ref()
        .and_then(|window| window.title.as_ref())
    {
        parts.push(format!("window={}", window));
    }
    if let Some(role) = selector.element.role.as_ref() {
        parts.push(format!("role={}", role));
    }
    if let Some(name) = selector.element.name.as_ref() {
        parts.push(format!("name={}", name));
    }
    if let Some(text) = selector.element.text.as_ref() {
        parts.push(format!("text={}", text));
    }
    if let Some(id) = selector.element.id.as_ref() {
        parts.push(format!("id={}", id));
    }

    if parts.is_empty() {
        "unknown target".to_string()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::{Context, FindAppsResult, ScreenshotData, WaitCondition, WaitResult};
    use crate::selector::*;
    use crate::vision::{VisionError, VisionResult};

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
            _display_id: Option<u32>,
        ) -> Result<ScreenshotData, PerceptionError> {
            Ok(ScreenshotData {
                format: crate::perception::ScreenshotFormat::Png,
                data: Vec::new(),
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 400.0,
                    height: 300.0,
                }),
            })
        }

        async fn find_apps(
            &self,
            _pattern: &str,
            _limit: Option<u32>,
        ) -> Result<FindAppsResult, PerceptionError> {
            Err(PerceptionError::NotImplemented("mock".to_string()))
        }
    }

    struct MockVisionProvider {
        coordinate: Coordinate,
    }

    #[async_trait::async_trait]
    impl VisionProvider for MockVisionProvider {
        async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
            Ok(VisionResult {
                coordinate: self.coordinate.clone(),
                confidence: 0.9,
                model_used: "mock".to_string(),
            })
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

    #[test]
    fn test_backend_from_str() {
        assert_eq!(Backend::from_str("cdp").unwrap(), Backend::Cdp);
        assert_eq!(Backend::from_str("at_tree").unwrap(), Backend::AtTree);
        assert_eq!(Backend::from_str("vision").unwrap(), Backend::Vision);
        assert_eq!(Backend::from_str("CDP").unwrap(), Backend::Cdp);
        assert_eq!(Backend::from_str("AT-TREE").unwrap(), Backend::AtTree);
    }

    #[test]
    fn test_backend_display() {
        assert_eq!(Backend::Cdp.to_string(), "cdp");
        assert_eq!(Backend::AtTree.to_string(), "at_tree");
        assert_eq!(Backend::Vision.to_string(), "vision");
    }

    #[test]
    fn test_default_fallback_priority() {
        let priority = default_fallback_priority();
        assert_eq!(priority.len(), 3);
        assert_eq!(priority[0], Backend::Cdp);
        assert_eq!(priority[1], Backend::AtTree);
        assert_eq!(priority[2], Backend::Vision);
    }

    #[tokio::test]
    async fn test_resolve_selector_with_vision_priority() {
        let provider = MockPerceptionProvider::empty();
        let vision = MockVisionProvider {
            coordinate: Coordinate { x: 11.0, y: 22.0 },
        };
        let cascade = Cascade::with_priority(
            &provider,
            Some(&vision),
            vec![Backend::Vision, Backend::AtTree, Backend::Cdp],
        );

        let selector = Selector::new().with_name("Submit");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await.unwrap();
        assert_eq!(result.0.x, 11.0);
        assert_eq!(result.0.y, 22.0);
        assert_eq!(result.1, Some(Backend::Vision));
    }

    #[tokio::test]
    async fn test_resolve_selector_with_at_tree_priority() {
        let element = make_element(100.0, 200.0, 50.0, 20.0);
        let target = make_resolved_target(element);

        let provider = MockPerceptionProvider::with_find_result(target);
        let vision = MockVisionProvider {
            coordinate: Coordinate { x: 0.0, y: 0.0 },
        };
        let cascade = Cascade::with_priority(
            &provider,
            Some(&vision),
            vec![Backend::AtTree, Backend::Vision, Backend::Cdp],
        );

        let selector = Selector::new().with_role("button").with_name("Test");
        let action_target = ActionTarget::Selector(selector);

        let (coord, backend) = cascade.resolve_coordinate(&action_target).await.unwrap();
        assert_eq!(coord.x, 125.0);
        assert_eq!(coord.y, 210.0);
        assert_eq!(backend, Some(Backend::AtTree));
    }

    #[tokio::test]
    async fn test_resolve_selector_without_vision_provider() {
        let provider = MockPerceptionProvider::empty();
        let cascade = Cascade::<_, MockVisionProvider>::with_priority(
            &provider,
            None,
            vec![Backend::Vision, Backend::AtTree, Backend::Cdp],
        );

        let selector = Selector::new().with_name("Submit");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_click_with_coordinate() {
        let provider = MockPerceptionProvider::empty();
        let vision = MockVisionProvider {
            coordinate: Coordinate { x: 0.0, y: 0.0 },
        };
        let cascade = Cascade::new(&provider, Some(&vision));
        let action_target = ActionTarget::Coordinate(Coordinate { x: 10.0, y: 20.0 });

        let (coord, backend) = cascade.resolve_coordinate(&action_target).await.unwrap();
        assert_eq!(coord.x, 10.0);
        assert_eq!(coord.y, 20.0);
        assert_eq!(backend, None);
    }

    #[tokio::test]
    async fn test_resolve_selector_fallback_chain() {
        let provider = MockPerceptionProvider::empty();
        let vision = MockVisionProvider {
            coordinate: Coordinate { x: 13.0, y: 14.0 },
        };
        let cascade = Cascade::new(&provider, Some(&vision));

        let selector = Selector::new().with_name("NonExistent");
        let target = ActionTarget::Selector(selector);

        let result = cascade.resolve_coordinate(&target).await.unwrap();
        assert_eq!(result.0.x, 13.0);
        assert_eq!(result.0.y, 14.0);
        assert_eq!(result.1, Some(Backend::Vision));
    }
}