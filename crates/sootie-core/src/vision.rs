use async_trait::async_trait;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{ColorType, GenericImageView, ImageEncoder, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;

use crate::perception::ScreenshotData;
use crate::selector::{Bounds, Coordinate};

const GROUNDING_IMAGE_DIR: &str = "/tmp/sootie";
const MAX_GROUNDING_IMAGE_WIDTH: u32 = 1600;
const GROUNDING_IMAGE_SAVE_ATTEMPTS: usize = 10;
const GROUNDING_IMAGE_SAVE_WAIT: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionRequest {
    pub screenshot: ScreenshotData,
    pub target_description: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisionResult {
    pub coordinate: Coordinate,
    #[serde(default)]
    pub bounds: Option<Bounds>,
    pub confidence: f64,
    pub model_used: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudVlmConfig {
    pub api_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("model inference failed: {0}")]
    InferenceFailed(String),

    #[error("element not detected: {0}")]
    NotDetected(String),

    #[error("low confidence: {confidence:.2} for '{target}'")]
    LowConfidence { target: String, confidence: f64 },

    #[error("model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[async_trait]
pub trait VisionProvider: Send + Sync {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError>;
}

pub struct CloudVlmProvider {
    _config: CloudVlmConfig,
}

impl CloudVlmProvider {
    pub fn new(config: CloudVlmConfig) -> Self {
        Self { _config: config }
    }
}

#[async_trait]
impl VisionProvider for CloudVlmProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented(
            "cloud VLM provider not yet implemented".to_string(),
        ))
    }
}

#[derive(Debug, Deserialize)]
struct SidecarResponse {
    matches: Vec<SidecarGroundingMatch>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct SidecarGroundingMatch {
    label: String,
    confidence: f64,
    point: SidecarGroundingPoint,
    bbox: SidecarBoundingBox,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct SidecarGroundingPoint {
    x: f64,
    y: f64,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct SidecarBoundingBox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

#[derive(Debug, Serialize)]
struct SidecarRequest {
    task_desc: String,
    local_image_path: String,
}

struct PreparedGroundingImage {
    path: PathBuf,
    offset_x: f64,
    offset_y: f64,
    coordinate_width: f64,
    coordinate_height: f64,
    prepared_width: f64,
    prepared_height: f64,
}

#[derive(Clone, Copy)]
struct PixelRect {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

fn save_rgba_png_stripped(path: &PathBuf, canvas: &RgbaImage) -> Result<(), VisionError> {
    let file = File::create(path)
        .map_err(|e| VisionError::InferenceFailed(format!("Failed to create PNG image: {}", e)))?;
    PngEncoder::new(file)
        .write_image(
            canvas.as_raw(),
            canvas.width(),
            canvas.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(|e| VisionError::InferenceFailed(format!("Failed to encode PNG image: {}", e)))
}

static UI_ELEMENT_PROMPTS: &[(&str, &str)] = &[
    ("address bar", "the browser URL/address input field at the top of the window, showing the current webpage URL"),
    ("url bar", "the browser URL/address input field at the top of the window"),
    ("search box", "the search input field, typically with a magnifying glass icon or placeholder text like 'Search'"),
    ("search bar", "the search input field, typically with a search icon"),
    ("search input", "the search text input field"),
    ("login button", "the login/sign in button, typically labeled 'Login', 'Sign In', or 'Log In'"),
    ("sign in button", "the sign in/login button"),
    ("submit button", "the submit/send button, typically at the bottom or right of a form"),
    ("send button", "the send/submit button, typically with an arrow or paper plane icon"),
    ("compose button", "the compose/new message button, typically labeled 'Compose' or '+'"),
    ("new button", "the create new button, typically labeled 'New' or '+'"),
    ("close button", "the close button, typically an 'X' icon at the top right corner"),
    ("back button", "the navigation back button, typically a left arrow icon or labeled 'Back'"),
    ("forward button", "the navigation forward button, typically a right arrow icon"),
    ("next button", "the next/continue button, typically labeled 'Next' or 'Continue'"),
    ("previous button", "the previous/back button"),
    ("menu button", "the menu/hamburger button, typically three horizontal lines"),
    ("hamburger menu", "the hamburger menu button with three horizontal lines"),
    ("settings button", "the settings/preferences button, typically a gear/cog icon"),
    ("settings", "the settings/preferences button or menu, typically with a gear icon"),
    ("download button", "the download button, typically labeled 'Download' or with a downward arrow"),
    ("upload button", "the upload button, typically labeled 'Upload' or with an upward arrow"),
    ("play button", "the play button, typically a triangle/play icon"),
    ("pause button", "the pause button, typically two vertical bars"),
    ("volume button", "the volume/speaker control, typically a speaker icon"),
    ("fullscreen button", "the fullscreen button, typically at the bottom right corner of video"),
    ("fullscreen", "the fullscreen button or control"),
    ("minimize button", "the window minimize button, typically a minus/horizontal line at top right"),
    ("maximize button", "the window maximize button, typically a square icon at top right"),
    ("refresh button", "the refresh/reload button, typically a circular arrow icon"),
    ("reload button", "the reload/refresh button"),
    ("home button", "the home button, typically a house icon"),
    ("bookmark button", "the bookmark/star button, typically a star icon"),
    ("share button", "the share button, typically with an arrow or share icon"),
    ("copy button", "the copy button, typically two rectangles or labeled 'Copy'"),
    ("paste button", "the paste button"),
    ("cut button", "the cut button"),
    ("undo button", "the undo button, typically a curved left arrow"),
    ("redo button", "the redo button, typically a curved right arrow"),
    ("save button", "the save button, typically labeled 'Save' or with a disk icon"),
    ("open button", "the open button, typically labeled 'Open'"),
    ("edit button", "the edit button, typically a pencil icon"),
    ("delete button", "the delete button, typically labeled 'Delete' or with a trash icon"),
    ("trash button", "the delete/trash button"),
    ("add button", "the add button, typically a plus '+' icon"),
    ("remove button", "the remove button, typically a minus '-' icon"),
    ("cancel button", "the cancel button, typically labeled 'Cancel'"),
    ("ok button", "the OK/confirm button"),
    ("confirm button", "the confirm/OK button"),
    ("accept button", "the accept/confirm button"),
    ("reject button", "the reject/cancel button"),
    ("like button", "the like button, typically a heart or thumbs-up icon"),
    ("dislike button", "the dislike button, typically a thumbs-down icon"),
    ("favorite button", "the favorite/bookmark button, typically a star or heart icon"),
    ("follow button", "the follow button, typically labeled 'Follow'"),
    ("subscribe button", "the subscribe button, typically labeled 'Subscribe'"),
    ("notification button", "the notification button, typically a bell icon"),
    ("bell", "the notification bell icon"),
    ("profile button", "the profile/user button, typically a person icon or avatar"),
    ("avatar", "the user avatar/profile image"),
    ("user button", "the user/profile button"),
    ("help button", "the help button, typically a question mark '?' icon"),
    ("info button", "the info button, typically an 'i' or question mark icon"),
    ("question button", "the help/question button"),
    ("expand button", "the expand/maximize button"),
    ("collapse button", "the collapse/minimize button"),
    ("zoom in button", "the zoom in button, typically a plus icon with magnifier"),
    ("zoom out button", "the zoom out button"),
    ("print button", "the print button, typically a printer icon"),
    ("email button", "the email/mail button, typically an envelope icon"),
    ("mail button", "the mail/email button"),
    ("attachment button", "the attachment button, typically a paperclip icon"),
    ("link button", "the link button, typically a chain link icon"),
    ("image button", "the image/gallery button"),
    ("video button", "the video button"),
    ("audio button", "the audio/sound button"),
    ("file button", "the file/document button"),
    ("folder button", "the folder/directory button"),
    ("document button", "the document/file button"),
    ("text input", "any text input field with a rectangular border"),
    ("input field", "any input field"),
    ("text box", "a text input box"),
    ("textarea", "a multiline text input area"),
    ("checkbox", "a checkbox, typically a small square"),
    ("radio button", "a radio button, typically a small circle"),
    ("dropdown", "a dropdown/select menu, typically with a downward arrow"),
    ("select box", "a dropdown/select input"),
    ("slider", "a slider control, typically a horizontal bar with a handle"),
    ("toggle", "a toggle switch, typically showing on/off state"),
    ("tab", "a browser or application tab, typically at the top"),
    ("sidebar", "the sidebar navigation panel, typically on the left"),
    ("navigation", "the navigation menu or sidebar"),
    ("toolbar", "the toolbar at the top of the window"),
    ("header", "the header area at the top of the page/window"),
    ("footer", "the footer area at the bottom of the page"),
    ("status bar", "the status bar at the bottom of the window"),
    ("title bar", "the window title bar at the top"),
    ("window controls", "the window control buttons (close, minimize, maximize)"),
    ("scrollbar", "the scrollbar, typically a vertical bar on the right side"),
    ("breadcrumb", "the breadcrumb navigation path"),
    ("pagination", "the pagination controls showing page numbers"),
    ("progress bar", "the progress bar showing completion status"),
    ("loading indicator", "the loading spinner or progress indicator"),
    ("spinner", "the loading spinner animation"),
    ("tooltip", "a tooltip popup message"),
    ("popup", "a popup window or dialog"),
    ("modal", "a modal dialog overlay"),
    ("dialog", "a dialog box or modal window"),
    ("alert", "an alert/notification popup"),
];

fn apply_prompt_template(description: &str) -> String {
    let desc_lower = description.to_lowercase();

    for (keyword, template) in UI_ELEMENT_PROMPTS {
        if desc_lower.contains(keyword) {
            return template.to_string();
        }
    }

    description.to_string()
}

fn build_task_desc(request: &VisionRequest) -> String {
    let enhanced_desc = apply_prompt_template(&request.target_description);

    match request.context.as_deref() {
        Some(context) if !context.is_empty() => {
            format!("{}\nContext: {}", enhanced_desc, context)
        }
        _ => enhanced_desc,
    }
}

fn prepare_grounding_image(
    screenshot: &ScreenshotData,
) -> Result<PreparedGroundingImage, VisionError> {
    let image = image::load_from_memory(&screenshot.data)
        .map_err(|e| VisionError::InferenceFailed(format!("Failed to decode screenshot: {}", e)))?;

    let (original_width, original_height) = image.dimensions();
    if original_width == 0 || original_height == 0 {
        return Err(VisionError::InferenceFailed(
            "Screenshot has invalid dimensions".to_string(),
        ));
    }

    let (prepared_width, prepared_height, prepared_image) =
        if original_width > MAX_GROUNDING_IMAGE_WIDTH {
            let prepared_width = MAX_GROUNDING_IMAGE_WIDTH;
            let prepared_height = ((original_height as f64 * prepared_width as f64)
                / original_width as f64)
                .round()
                .max(1.0) as u32;
            (
                prepared_width,
                prepared_height,
                image.resize_exact(prepared_width, prepared_height, FilterType::Lanczos3),
            )
        } else {
            (original_width, original_height, image)
        };

    let dir = PathBuf::from(GROUNDING_IMAGE_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| {
        VisionError::InferenceFailed(format!("Failed to create grounding image directory: {}", e))
    })?;
    let file_name = format!(
        "{}.png",
        chrono::Local::now().format("%Y-%m-%d-%H-%M-%S-%9f")
    );
    let path = dir.join(file_name);
    save_rgba_png_stripped(&path, &prepared_image.to_rgba8())?;
    for attempt in 1..=GROUNDING_IMAGE_SAVE_ATTEMPTS {
        if path.is_file() {
            break;
        }
        if attempt == GROUNDING_IMAGE_SAVE_ATTEMPTS {
            return Err(VisionError::InferenceFailed(format!(
                "Grounding image was not readable after save: {}",
                path.display()
            )));
        }
        std::thread::sleep(GROUNDING_IMAGE_SAVE_WAIT);
    }

    Ok(PreparedGroundingImage {
        path,
        offset_x: screenshot
            .bounds
            .as_ref()
            .map(|bounds| bounds.x)
            .unwrap_or(0.0),
        offset_y: screenshot
            .bounds
            .as_ref()
            .map(|bounds| bounds.y)
            .unwrap_or(0.0),
        coordinate_width: screenshot
            .bounds
            .as_ref()
            .map(|bounds| bounds.width)
            .unwrap_or(original_width as f64),
        coordinate_height: screenshot
            .bounds
            .as_ref()
            .map(|bounds| bounds.height)
            .unwrap_or(original_height as f64),
        prepared_width: prepared_width as f64,
        prepared_height: prepared_height as f64,
    })
}

fn normalize_dimension(value: f64, full_size: f64) -> f64 {
    value * full_size
}

fn map_sidecar_point(point: &SidecarGroundingPoint, image: &PreparedGroundingImage) -> Coordinate {
    Coordinate {
        x: image.offset_x + point.x * image.coordinate_width,
        y: image.offset_y + point.y * image.coordinate_height,
    }
}

fn coordinate_from_match(
    grounding_match: &SidecarGroundingMatch,
    image: &PreparedGroundingImage,
) -> Coordinate {
    map_sidecar_point(&grounding_match.point, image)
}

fn bounds_from_match(
    grounding_match: &SidecarGroundingMatch,
    image: &PreparedGroundingImage,
) -> Option<Bounds> {
    let bbox = &grounding_match.bbox;
    if bbox.width <= 0.0 || bbox.height <= 0.0 {
        return None;
    }

    Some(Bounds {
        x: image.offset_x + bbox.x * image.coordinate_width,
        y: image.offset_y + bbox.y * image.coordinate_height,
        width: bbox.width * image.coordinate_width,
        height: bbox.height * image.coordinate_height,
    })
}

fn prepared_bbox_pixels(
    bbox: &SidecarBoundingBox,
    image: &PreparedGroundingImage,
) -> Option<PixelRect> {
    let prepared_width = image.prepared_width.max(1.0);
    let prepared_height = image.prepared_height.max(1.0);
    let left = normalize_dimension(bbox.x, prepared_width).round();
    let top = normalize_dimension(bbox.y, prepared_height).round();
    let width = normalize_dimension(bbox.width, prepared_width).round();
    let height = normalize_dimension(bbox.height, prepared_height).round();
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let max_x = prepared_width.round().max(1.0) as u32 - 1;
    let max_y = prepared_height.round().max(1.0) as u32 - 1;
    let left = left.clamp(0.0, f64::from(max_x)) as u32;
    let top = top.clamp(0.0, f64::from(max_y)) as u32;
    let right = (left as f64 + width - 1.0).clamp(f64::from(left), f64::from(max_x)) as u32;
    let bottom = (top as f64 + height - 1.0).clamp(f64::from(top), f64::from(max_y)) as u32;

    Some(PixelRect {
        left,
        top,
        right,
        bottom,
    })
}

fn draw_rect_outline(canvas: &mut RgbaImage, rect: PixelRect, color: Rgba<u8>) {
    let thickness = 2;
    for offset in 0..thickness {
        let left = rect.left.saturating_sub(offset);
        let top = rect.top.saturating_sub(offset);
        let right = rect
            .right
            .saturating_add(offset)
            .min(canvas.width().saturating_sub(1));
        let bottom = rect
            .bottom
            .saturating_add(offset)
            .min(canvas.height().saturating_sub(1));

        for x in left..=right {
            canvas.put_pixel(x, top, color);
            canvas.put_pixel(x, bottom, color);
        }

        for y in top..=bottom {
            canvas.put_pixel(left, y, color);
            canvas.put_pixel(right, y, color);
        }
    }
}

fn fill_rect(canvas: &mut RgbaImage, rect: PixelRect, color: Rgba<u8>) {
    for y in rect.top..=rect.bottom {
        for x in rect.left..=rect.right {
            canvas.put_pixel(x, y, color);
        }
    }
}

fn glyph_rows(ch: char) -> [u8; 7] {
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'J' => [0x1F, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x14, 0x04, 0x04, 0x04, 0x1F],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x06, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x06],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        ':' => [0x00, 0x06, 0x06, 0x00, 0x06, 0x06, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        ' ' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        _ => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
    }
}

fn draw_text(canvas: &mut RgbaImage, x: u32, y: u32, text: &str, color: Rgba<u8>) {
    let scale = 2;
    for (char_index, ch) in text.chars().enumerate() {
        let rows = glyph_rows(ch);
        let char_x = x + char_index as u32 * 12;
        for (row_index, row) in rows.iter().enumerate() {
            for col_index in 0..5 {
                if row & (1 << (4 - col_index)) == 0 {
                    continue;
                }

                for dx in 0..scale {
                    for dy in 0..scale {
                        let px = char_x + col_index * scale + dx;
                        let py = y + row_index as u32 * scale + dy;
                        if px < canvas.width() && py < canvas.height() {
                            canvas.put_pixel(px, py, color);
                        }
                    }
                }
            }
        }
    }
}

fn draw_target_title(canvas: &mut RgbaImage, target_description: &str) {
    let title_text = format!("Target: {}", target_description);
    let text_width = title_text.chars().count() as u32 * 12;
    let text_height = 14;
    let padding = 4;
    let label_width = (text_width + padding * 2).min(canvas.width());
    let label_height = text_height + padding * 2;

    let background = PixelRect {
        left: 0,
        top: 0,
        right: label_width.saturating_sub(1),
        bottom: label_height.saturating_sub(1),
    };

    let title_color = Rgba([255, 0, 0, 255]);
    fill_rect(canvas, background, title_color);
    draw_text(
        canvas,
        background.left + padding,
        background.top + padding,
        &title_text,
        Rgba([255, 255, 255, 255]),
    );
}

fn build_match_label(index: usize, grounding_match: &SidecarGroundingMatch) -> String {
    let mut parts = vec![format!("#{}", index + 1)];
    let compact = grounding_match.label.trim();
    if !compact.is_empty() {
        parts.push(compact.chars().take(24).collect());
    }
    parts.push(format!("{:.2}", grounding_match.confidence));
    parts.join(" ")
}

fn draw_match_label(canvas: &mut RgbaImage, rect: PixelRect, text: &str, color: Rgba<u8>) {
    let text_width = text.chars().count() as u32 * 12;
    let text_height = 14;
    let padding = 2;
    let label_width = (text_width + padding * 2).min(canvas.width());
    let label_height = text_height + padding * 2;
    let left = rect.left.min(canvas.width().saturating_sub(label_width));
    let top = if rect.top > label_height + 4 {
        rect.top - label_height - 4
    } else {
        (rect.bottom + 4).min(canvas.height().saturating_sub(label_height))
    };
    let background = PixelRect {
        left,
        top,
        right: left + label_width.saturating_sub(1),
        bottom: top + label_height.saturating_sub(1),
    };
    fill_rect(canvas, background, color);
    draw_text(
        canvas,
        left + padding,
        top + padding,
        text,
        Rgba([255, 255, 255, 255]),
    );
}

fn save_annotated_image(path: &PathBuf, canvas: RgbaImage) -> Result<(), VisionError> {
    let image = image::DynamicImage::ImageRgba8(canvas);
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("png") => {
            save_rgba_png_stripped(path, &image.to_rgba8())
        }
        _ => {
            let file = File::create(path).map_err(|e| {
                VisionError::InferenceFailed(format!("Failed to overwrite annotated image: {}", e))
            })?;
            let mut encoder = JpegEncoder::new_with_quality(file, 95);
            encoder.encode_image(&image.to_rgb8()).map_err(|e| {
                VisionError::InferenceFailed(format!("Failed to encode annotated image: {}", e))
            })
        }
    }
}

fn annotate_grounding_image(
    image: &PreparedGroundingImage,
    response: &SidecarResponse,
    target_description: &str,
) -> Result<(), VisionError> {
    let mut canvas = image::open(&image.path)
        .map_err(|e| {
            VisionError::InferenceFailed(format!("Failed to open grounding image: {}", e))
        })?
        .to_rgba8();

    draw_target_title(&mut canvas, target_description);

    let color = Rgba([255, 0, 0, 255]);
    let mut drew_any = false;

    for (index, grounding_match) in response.matches.iter().enumerate() {
        let Some(rect) = prepared_bbox_pixels(&grounding_match.bbox, image) else {
            continue;
        };

        draw_rect_outline(&mut canvas, rect, color);
        draw_match_label(
            &mut canvas,
            rect,
            &build_match_label(index, grounding_match),
            color,
        );
        drew_any = true;
    }

    if !drew_any {
        return Ok(());
    }

    save_annotated_image(&image.path, canvas)
}

fn map_sidecar_coordinate(
    grounding_match: &SidecarGroundingMatch,
    image: &PreparedGroundingImage,
) -> Coordinate {
    coordinate_from_match(grounding_match, image)
}

pub struct SidecarVisionProvider {
    base_url: String,
    client: reqwest::Client,
    auth_token: Option<String>,
}

impl SidecarVisionProvider {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            auth_token: std::env::var("SOOTIE_SIDECAR_AUTH_TOKEN").ok(),
        }
    }

    pub async fn health_check(&self) -> Result<bool, VisionError> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

#[async_trait]
impl VisionProvider for SidecarVisionProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        let grounding_image = prepare_grounding_image(&request.screenshot)?;

        let body = SidecarRequest {
            task_desc: build_task_desc(request),
            local_image_path: grounding_image.path.display().to_string(),
        };

        let url = format!("{}/ground", self.base_url);
        let mut request_builder = self.client.post(&url).json(&body);
        if let Some(token) = self.auth_token.as_deref() {
            request_builder = request_builder.header("X-Sootie-Auth", token);
        }
        let resp = request_builder.send().await.map_err(|e| {
            VisionError::NetworkError(format!("Sidecar unreachable on {}: {}", self.base_url, e))
        })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(VisionError::InferenceFailed(format!(
                "Sidecar returned {}: {}",
                status, body
            )));
        }

        let result: SidecarResponse = resp.json().await.map_err(|e| {
            VisionError::InferenceFailed(format!("Failed to parse sidecar response: {}", e))
        })?;

        if let Some(err) = result.error {
            return Err(VisionError::InferenceFailed(err));
        }

        let primary_match = result.matches.first().ok_or_else(|| {
            VisionError::InferenceFailed("Sidecar returned no grounding matches".to_string())
        })?;

        annotate_grounding_image(&grounding_image, &result, &request.target_description)?;

        let coordinate = map_sidecar_coordinate(primary_match, &grounding_image);
        let bounds = bounds_from_match(primary_match, &grounding_image);

        Ok(VisionResult {
            coordinate,
            bounds,
            confidence: primary_match.confidence,
            model_used: "showui-2b".to_string(),
        })
    }
}

pub struct StubVisionProvider;

#[async_trait]
impl VisionProvider for StubVisionProvider {
    async fn detect(&self, _request: &VisionRequest) -> Result<VisionResult, VisionError> {
        Err(VisionError::NotImplemented("stub provider".to_string()))
    }
}

pub enum RuntimeVisionProvider {
    Sidecar(SidecarVisionProvider),
    Stub(StubVisionProvider),
}

impl RuntimeVisionProvider {
    pub fn from_env() -> Self {
        if let Some(model_path) = std::env::var_os("SOOTIE_VISION_MODEL_PATH") {
            if std::path::Path::new(&model_path).exists() {
                let port = std::env::var("SOOTIE_SIDECAR_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(9876);
                return Self::Sidecar(SidecarVisionProvider::new(port));
            }
        }
        Self::Stub(StubVisionProvider)
    }
}

#[async_trait]
impl VisionProvider for RuntimeVisionProvider {
    async fn detect(&self, request: &VisionRequest) -> Result<VisionResult, VisionError> {
        match self {
            RuntimeVisionProvider::Sidecar(provider) => provider.detect(request).await,
            RuntimeVisionProvider::Stub(provider) => provider.detect(request).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perception::ScreenshotFormat;
    use crate::selector::Bounds;
    use image::ImageFormat;
    use std::io::Cursor;

    #[test]
    fn test_sidecar_request_serialize() {
        let request = SidecarRequest {
            task_desc: "Compose button".to_string(),
            local_image_path: "/tmp/input.jpg".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("task_desc"));
        assert!(json.contains("local_image_path"));
    }

    #[test]
    fn test_build_task_desc_includes_context() {
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50],
                bounds: None,
            },
            target_description: "Compose button".to_string(),
            context: Some("Gmail inbox".to_string()),
        };

        let desc = build_task_desc(&request);
        assert!(desc.contains("the compose/new message button"));
        assert!(desc.contains("Context: Gmail inbox"));
    }

    #[test]
    fn test_apply_prompt_template_address_bar() {
        let result = apply_prompt_template("address bar");
        assert!(result.contains("browser URL/address input field"));
        assert!(result.contains("top of the window"));
    }

    #[test]
    fn test_apply_prompt_template_search_box() {
        let result = apply_prompt_template("search box in Gmail");
        assert!(result.contains("search input field"));
        assert!(result.contains("magnifying glass icon"));
    }

    #[test]
    fn test_apply_prompt_template_login_button() {
        let result = apply_prompt_template("login button");
        assert!(result.contains("login/sign in button"));
    }

    #[test]
    fn test_apply_prompt_template_close_button() {
        let result = apply_prompt_template("close button");
        assert!(result.contains("'X' icon"));
        assert!(result.contains("top right corner"));
    }

    #[test]
    fn test_apply_prompt_template_unknown_description() {
        let result = apply_prompt_template("custom widget element");
        assert_eq!(result, "custom widget element");
    }

    #[test]
    fn test_apply_prompt_template_case_insensitive() {
        let result1 = apply_prompt_template("Address Bar");
        let result2 = apply_prompt_template("ADDRESS BAR");
        assert!(result1.contains("browser URL"));
        assert!(result2.contains("browser URL"));
    }

    #[test]
    fn test_apply_prompt_template_with_partial_match() {
        let result = apply_prompt_template("the submit button for the form");
        assert!(result.contains("submit/send button"));
    }

    #[test]
    fn test_build_task_desc_without_context() {
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50],
                bounds: None,
            },
            target_description: "settings button".to_string(),
            context: None,
        };

        let desc = build_task_desc(&request);
        assert!(desc.contains("settings/preferences button"));
        assert!(desc.contains("gear"));
        assert!(!desc.contains("Context"));
    }

    #[test]
    fn test_build_task_desc_empty_context() {
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50],
                bounds: None,
            },
            target_description: "back button".to_string(),
            context: Some("".to_string()),
        };

        let desc = build_task_desc(&request);
        assert!(desc.contains("navigation back button"));
        assert!(!desc.contains("Context"));
    }

    #[test]
    fn test_vision_request_serialize() {
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![0x89, 0x50],
                bounds: Some(Bounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                }),
            },
            target_description: "Compose button".to_string(),
            context: Some("Gmail inbox".to_string()),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("Compose button"));
    }

    #[test]
    fn test_prepare_grounding_image_resizes_to_1600_width_png() {
        let image = image::DynamicImage::new_rgba8(2160, 1080);
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();

        let screenshot = ScreenshotData {
            format: ScreenshotFormat::Png,
            data: bytes,
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 2160.0,
                height: 1080.0,
            }),
        };

        let prepared = prepare_grounding_image(&screenshot).unwrap();
        let prepared_image = image::open(&prepared.path).unwrap();

        assert_eq!(prepared.coordinate_width, 2160.0);
        assert_eq!(prepared.coordinate_height, 1080.0);
        assert_eq!(prepared.prepared_width, 1600.0);
        assert_eq!(prepared_image.width(), 1600);
        assert_eq!(prepared_image.height(), 800);
        assert_eq!(
            prepared.path.extension().and_then(|ext| ext.to_str()),
            Some("png")
        );

        std::fs::remove_file(prepared.path).unwrap();
    }

    #[test]
    fn test_prepare_grounding_image_persists_under_tmp_sootie() {
        let image = image::DynamicImage::new_rgba8(200, 100);
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();

        let screenshot = ScreenshotData {
            format: ScreenshotFormat::Png,
            data: bytes,
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            }),
        };

        let prepared = prepare_grounding_image(&screenshot).unwrap();
        let path = prepared.path.clone();
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap();

        assert!(path.starts_with(std::path::Path::new("/tmp/sootie")));
        assert_eq!(path.parent(), Some(std::path::Path::new("/tmp/sootie")));
        assert_eq!(file_name.len(), "2026-05-09-20-10-11-123456789.png".len());
        assert!(file_name.ends_with(".png"));
        assert!(file_name[..file_name.len() - 4]
            .chars()
            .all(|ch| ch.is_ascii_digit() || ch == '-'));

        drop(prepared);

        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn test_map_sidecar_coordinate_from_first_match_point() {
        let response = SidecarGroundingMatch {
            label: "compose".to_string(),
            confidence: 0.9,
            point: SidecarGroundingPoint { x: 0.5, y: 0.25 },
            bbox: SidecarBoundingBox {
                x: 0.4,
                y: 0.2,
                width: 0.2,
                height: 0.1,
            },
        };
        let image = PreparedGroundingImage {
            path: std::env::temp_dir().join("unused.jpg"),
            offset_x: 10.0,
            offset_y: 20.0,
            coordinate_width: 1440.0,
            coordinate_height: 900.0,
            prepared_width: 1080.0,
            prepared_height: 675.0,
        };

        let coordinate = map_sidecar_coordinate(&response, &image);
        assert_eq!(coordinate.x, 730.0);
        assert_eq!(coordinate.y, 245.0);
    }

    #[test]
    fn test_vision_result_serialize() {
        let result = VisionResult {
            coordinate: Coordinate { x: 150.0, y: 300.0 },
            bounds: Some(Bounds {
                x: 100.0,
                y: 275.0,
                width: 100.0,
                height: 50.0,
            }),
            confidence: 0.95,
            model_used: "showui-2b".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: VisionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(result, deserialized);
    }

    #[test]
    fn test_sidecar_provider_url() {
        let provider = SidecarVisionProvider::new(9876);
        assert!(provider.base_url.contains("9876"));
    }

    #[test]
    fn test_sidecar_response_deserialize() {
        let json = r#"{
            "matches": [
                {
                    "label": "compose",
                    "confidence": 0.95,
                    "point": { "x": 0.4, "y": 0.6 },
                    "bbox": { "x": 0.3, "y": 0.5, "width": 0.2, "height": 0.1 }
                }
            ]
        }"#;
        let resp: SidecarResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.matches.len(), 1);
        assert_eq!(resp.matches[0].label, "compose");
        assert_eq!(resp.matches[0].confidence, 0.95);
    }

    #[test]
    fn test_sidecar_response_requires_matches() {
        let json = r#"{"x": 150.0, "y": 300.0, "confidence": 0.95}"#;
        let result = serde_json::from_str::<SidecarResponse>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_sidecar_response_deserializes_matches_with_bbox() {
        let json = r#"{
            "matches": [
                {
                    "label": "primary compose button",
                    "confidence": 0.91,
                    "point": { "x": 0.35, "y": 0.45 },
                    "bbox": { "x": 0.2, "y": 0.3, "width": 0.3, "height": 0.2 }
                },
                {
                    "label": "secondary compose button",
                    "confidence": 0.67,
                    "point": { "x": 0.65, "y": 0.45 },
                    "bbox": { "x": 0.55, "y": 0.3, "width": 0.2, "height": 0.2 }
                }
            ]
        }"#;

        let resp: SidecarResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.matches.len(), 2);
        assert_eq!(resp.matches[0].label, "primary compose button");
        assert_eq!(resp.matches[0].bbox.width, 0.3);
        assert_eq!(resp.matches[1].confidence, 0.67);
    }

    #[test]
    fn test_annotate_grounding_image_overwrites_with_red_boxes_and_labels() {
        let image = image::DynamicImage::new_rgba8(200, 120);
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();

        let screenshot = ScreenshotData {
            format: ScreenshotFormat::Png,
            data: bytes,
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 120.0,
            }),
        };

        let prepared = prepare_grounding_image(&screenshot).unwrap();
        let response = SidecarResponse {
            error: None,
            matches: vec![
                SidecarGroundingMatch {
                    label: "primary".to_string(),
                    confidence: 0.91,
                    point: SidecarGroundingPoint { x: 0.35, y: 0.45 },
                    bbox: SidecarBoundingBox {
                        x: 0.2,
                        y: 0.3,
                        width: 0.25,
                        height: 0.25,
                    },
                },
                SidecarGroundingMatch {
                    label: "secondary".to_string(),
                    confidence: 0.67,
                    point: SidecarGroundingPoint { x: 0.7, y: 0.45 },
                    bbox: SidecarBoundingBox {
                        x: 0.58,
                        y: 0.28,
                        width: 0.2,
                        height: 0.3,
                    },
                },
            ],
        };

        annotate_grounding_image(&prepared, &response, "Submit button").unwrap();

        let annotated = image::open(&prepared.path).unwrap().to_rgba8();
        let first_corner = annotated.get_pixel(40, 36);
        let second_corner = annotated.get_pixel(116, 34);
        let label_background = annotated.get_pixel(40, 24);

        assert!(first_corner[0] > 200 && first_corner[1] < 80 && first_corner[2] < 80);
        assert!(second_corner[0] > 200 && second_corner[1] < 80 && second_corner[2] < 80);
        assert!(label_background[0] > 200 && label_background[1] < 80 && label_background[2] < 80);

        std::fs::remove_file(prepared.path).unwrap();
    }

    #[test]
    fn test_vision_error_display() {
        let err = VisionError::LowConfidence {
            target: "Submit".to_string(),
            confidence: 0.3,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("0.30"));
        assert!(msg.contains("Submit"));
    }

    #[test]
    fn test_vision_error_network() {
        let err = VisionError::NetworkError("timeout".to_string());
        assert!(err.to_string().contains("network error"));
    }

    #[tokio::test]
    async fn test_stub_vision_provider_returns_not_implemented() {
        let provider = StubVisionProvider;
        let request = VisionRequest {
            screenshot: ScreenshotData {
                format: ScreenshotFormat::Png,
                data: vec![],
                bounds: None,
            },
            target_description: "test".to_string(),
            context: None,
        };
        let result = provider.detect(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_runtime_from_env_no_model_path() {
        let previous = std::env::var_os("SOOTIE_VISION_MODEL_PATH");
        if let Some(_path) = previous {
            std::env::remove_var("SOOTIE_VISION_MODEL_PATH");
        }
        let provider = RuntimeVisionProvider::from_env();
        match provider {
            RuntimeVisionProvider::Stub(_) => {}
            _ => panic!("expected Stub variant"),
        }
    }

    #[tokio::test]
    async fn test_runtime_from_env_with_valid_path() {
        use std::io::Write;
        let temp_dir = tempfile::tempdir().unwrap();
        let model_path = temp_dir.path().join("ShowUI-2B");
        std::fs::create_dir_all(&model_path).unwrap();
        let mut f = std::fs::File::create(model_path.join("model.safetensors")).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        std::fs::File::create(model_path.join("config.json")).unwrap();

        let previous = std::env::var_os("SOOTIE_VISION_MODEL_PATH");
        std::env::set_var("SOOTIE_VISION_MODEL_PATH", model_path.to_str().unwrap());
        std::env::set_var("SOOTIE_SIDECAR_PORT", "9876");

        let provider = RuntimeVisionProvider::from_env();
        match provider {
            RuntimeVisionProvider::Sidecar(ref p) => {
                assert!(p.base_url.contains("9876"));
            }
            _ => panic!("expected Sidecar variant with valid model path"),
        }

        if let Some(path) = previous {
            std::env::set_var("SOOTIE_VISION_MODEL_PATH", path);
        } else {
            std::env::remove_var("SOOTIE_VISION_MODEL_PATH");
        }
        std::env::remove_var("SOOTIE_SIDECAR_PORT");
    }
}
