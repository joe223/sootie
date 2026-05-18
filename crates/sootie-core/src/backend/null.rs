use crate::backend::{unsupported, DesktopBackend};
use crate::types::{
    ActionResult, AppInfo, Bounds, ContextSnapshot, ElementInfo, FindQuery, Screenshot,
    SootieResult, WindowCommand,
};

pub struct NullBackend;

impl DesktopBackend for NullBackend {
    fn platform(&self) -> &'static str {
        "unsupported"
    }
    fn context(&self, _app: Option<&str>) -> SootieResult<ContextSnapshot> {
        Err(unsupported(self.platform(), "context"))
    }
    fn state(&self, _app: Option<&str>) -> SootieResult<Vec<AppInfo>> {
        Ok(vec![])
    }
    fn find(&self, _query: &FindQuery) -> SootieResult<Vec<ElementInfo>> {
        Ok(vec![])
    }
    fn read(
        &self,
        _app: Option<&str>,
        _query: Option<&str>,
        _depth: Option<u32>,
    ) -> SootieResult<String> {
        Ok(String::new())
    }
    fn inspect(&self, _query: &FindQuery) -> SootieResult<Option<ElementInfo>> {
        Ok(None)
    }
    fn element_at(&self, _x: f64, _y: f64) -> SootieResult<Option<ElementInfo>> {
        Ok(None)
    }
    fn screenshot(&self, _app: Option<&str>, _full_resolution: bool) -> SootieResult<Screenshot> {
        Err(unsupported(self.platform(), "screenshot"))
    }
    fn click(
        &self,
        _x: Option<f64>,
        _y: Option<f64>,
        _query: &FindQuery,
        _button: &str,
        _count: u32,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "click"))
    }
    fn hover(
        &self,
        _x: Option<f64>,
        _y: Option<f64>,
        _query: &FindQuery,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "hover"))
    }
    fn long_press(
        &self,
        _x: Option<f64>,
        _y: Option<f64>,
        _query: &FindQuery,
        _duration_secs: f64,
        _button: &str,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "long_press"))
    }
    fn drag(
        &self,
        _from: Option<(f64, f64)>,
        _to: (f64, f64),
        _query: &FindQuery,
        _duration_secs: f64,
        _hold_duration_secs: f64,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "drag"))
    }
    fn type_text(
        &self,
        _text: &str,
        _target: &FindQuery,
        _clear: bool,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "type_text"))
    }
    fn press(
        &self,
        _key: &str,
        _modifiers: &[String],
        _app: Option<&str>,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "press"))
    }
    fn hotkey(&self, _keys: &[String], _app: Option<&str>) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "hotkey"))
    }
    fn scroll(
        &self,
        _direction: &str,
        _amount: i32,
        _app: Option<&str>,
        _at: Option<(f64, f64)>,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "scroll"))
    }
    fn focus(
        &self,
        _app: &str,
        _platform_app_id: Option<&str>,
        _window: Option<&str>,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "focus"))
    }
    fn window(
        &self,
        _command: WindowCommand,
        _app: &str,
        _platform_app_id: Option<&str>,
        _window: Option<&str>,
        _bounds: Option<Bounds>,
    ) -> SootieResult<ActionResult> {
        Err(unsupported(self.platform(), "window"))
    }
}
