mod context;
mod find;

use async_trait::async_trait;
use tracing::debug;

use crate::cdp::try_find_via_cdp;
use crate::perception::{
    Context, DeepInspection, FindAppsResult, PerceptionError, PerceptionProvider, ScreenshotData,
    WaitCondition, WaitResult,
};
use crate::selector::Selector;

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

    async fn find(
        &self,
        selector: &Selector,
    ) -> Result<crate::selector::ResolvedTarget, PerceptionError> {
        debug!("Finding elements with selector: {:?}", selector);
        if let Ok(Some(result)) = try_find_via_cdp(selector).await {
            return Ok(result);
        }
        find::find_elements(selector)
    }

    async fn inspect(&self, selector: &Selector) -> Result<DeepInspection, PerceptionError> {
        debug!("Inspecting element");
        let resolved = self.find(selector).await?;
        if resolved.elements.is_empty() {
            return Err(PerceptionError::TargetNotFound(
                "no element matches selector".to_string(),
            ));
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
                let want_enabled = condition.state.get("enabled").and_then(|v| v.as_bool());
                let want_focused = condition.state.get("focused").and_then(|v| v.as_bool());
                let visible_ok = want_visible.map_or(true, |v| element.state.visible == v);
                let enabled_ok = want_enabled.map_or(true, |v| element.state.enabled == Some(v));
                let focused_ok = want_focused.map_or(true, |v| element.state.focused == Some(v));
                if visible_ok && enabled_ok && focused_ok {
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
        _display_id: Option<u32>,
    ) -> Result<ScreenshotData, PerceptionError> {
        debug!("Taking screenshot");
        use image::codecs::png::PngEncoder;
        use image::{ColorType, ImageEncoder};
        use windows::Win32::Foundation::*;
        use windows::Win32::Graphics::Gdi::*;

        let hwnd = if let Some(b) = region {
            HWND(0)
        } else {
            GetDesktopWindow()
        };

        let hdc = GetDC(hwnd);
        let width = region
            .map(|b| b.width as i32)
            .unwrap_or_else(|| GetSystemMetrics(SM_CXSCREEN));
        let height = region
            .map(|b| b.height as i32)
            .unwrap_or_else(|| GetSystemMetrics(SM_CYSCREEN));

        let memdc = CreateCompatibleDC(hdc);
        let hbitmap = CreateCompatibleBitmap(hdc, width, height);
        let old_bitmap = SelectObject(memdc, hbitmap);

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

        let scanlines = GetDIBits(
            memdc,
            hbitmap,
            0,
            height as u32,
            Some(data.as_mut_ptr() as *mut _),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        SelectObject(memdc, old_bitmap);
        DeleteDC(memdc);
        ReleaseDC(hwnd, hdc);
        DeleteObject(hbitmap);

        if scanlines == 0 {
            return Err(PerceptionError::ScreenshotFailed(
                "GetDIBits returned 0 scanlines".to_string(),
            ));
        }

        for pixel in data.chunks_exact_mut(4) {
            pixel.swap(0, 2);
        }

        let mut png_data = Vec::new();
        PngEncoder::new(&mut png_data)
            .write_image(&data, width as u32, height as u32, ColorType::Rgba8.into())
            .map_err(|e| PerceptionError::ScreenshotFailed(format!("PNG encode failed: {}", e)))?;

        Ok(ScreenshotData {
            format: crate::perception::ScreenshotFormat::Png,
            data: png_data,
            bounds: region.cloned(),
        })
    }

    async fn find_apps(
        &self,
        _pattern: &str,
        _limit: Option<u32>,
    ) -> Result<FindAppsResult, PerceptionError> {
        Err(PerceptionError::NotImplemented(
            "find_apps not implemented for Windows".to_string(),
        ))
    }
}
