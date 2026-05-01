mod context;
mod find;

use async_trait::async_trait;
use tracing::debug;

use crate::perception::{
    Context, DeepInspection, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::{Selector};

pub struct WindowsPerceptionProvider;

impl WindowsPerceptionProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PerceptionProvider for WindowsPerceptionProvider {
    async fn get_context(&self) -> Result<Context, PerceptionError> {
        debug!("Getting Windows context");
        context::get_running_apps()
    }

    async fn find(&self, selector: &Selector) -> Result<crate::selector::ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
        find::find_elements(selector)
    }

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        debug!("Inspecting element");
        let resolved = self.find(selector).await?;
        if resolved.elements.is_empty() {
            return Err(PerceptionError::TargetNotFound("no element matches selector".to_string()));
        }
        let element = resolved.elements[0].clone();
        Ok(DeepInspection {
            element,
            children: vec![],
            backend: "uiautomation".to_string(),
            actions: vec!["click".to_string(), "type".to_string()],
            raw_metadata: None,
        })
    }

    async fn wait(
        &self,
        selector: &Selector,
        condition: &WaitCondition,
    ) -> Result<WaitResult, PerceptionError> {
        debug!("Waiting for element");
        use std::time::{Duration, Instant};
        let start = Instant::now();
        let timeout = Duration::from_millis(condition.timeout_ms);

        loop {
            let result = self.find(selector).await?;
            if !result.elements.is_empty() {
                let element = &result.elements[0];
                let want_visible = condition.state.get("visible").and_then(|v| v.as_bool());
                let visible_ok = want_visible.map_or(true, |v| element.state.visible == v);
                if visible_ok {
                    return Ok(WaitResult {
                        matched: true,
                        element: Some(element.clone()),
                        timed_out: false,
                    });
                }
            }

            if start.elapsed() >= timeout {
                return Ok(WaitResult {
                    matched: false,
                    element: None,
                    timed_out: true,
                });
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn screenshot(
        &self,
        _target: Option<&Selector>,
        region: Option<&crate::selector::Bounds>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        use windows::Win32::Graphics::Gdi::*;
        use windows::Win32::Foundation::*;

        let hwnd = if let Some(b) = region {
            HWND(0)
        } else {
            GetDesktopWindow()
        };

        let hdc = GetDC(hwnd);
        let width = region.map(|b| b.width as i32).unwrap_or_else(|| {
            GetSystemMetrics(SM_CXSCREEN)
        });
        let height = region.map(|b| b.height as i32).unwrap_or_else(|| {
            GetSystemMetrics(SM_CYSCREEN)
        });

        let memdc = CreateCompatibleDC(hdc);
        let hbitmap = CreateCompatibleBitmap(hdc, width, height);
        SelectObject(memdc, hbitmap);

        BitBlt(
            memdc,
            0,
            0,
            width,
            height,
            hdc,
            region.map(|b| b.x as i32).unwrap_or(0),
            region.map(|b| b.y as i32).unwrap_or(0),
            SRCCOPY,
        );

        ReleaseDC(hwnd, hdc);
        DeleteDC(memdc);

        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
            bmiColors: [0; 1],
        };

        let size = (width * height * 4) as usize;
        let mut data = vec![0u8; size];

        GetDIBits(
            memdc,
            hbitmap,
            0,
            height as u32,
            Some(data.as_mut_ptr() as *mut _),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        DeleteObject(hbitmap);

        Ok(ScreenshotData {
            format: crate::perception::ScreenshotFormat::Png,
            data,
            bounds: region.cloned(),
        })
    }
}