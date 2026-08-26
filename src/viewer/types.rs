use crate::library::models::{MediaId, MediaRecord};
use crate::media::MediaFormat;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::time::Instant;

pub type TabId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabContentType {
    Image,
    Video,
}

#[derive(Clone)]
pub struct ImageTabPayload {
    pub high_res_pixels: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub is_loading: bool,
    pub error_message: String,
    pub orig_width: u32,
    pub orig_height: u32,
}

impl Default for ImageTabPayload {
    fn default() -> Self {
        Self {
            high_res_pixels: None,
            is_loading: true,
            error_message: String::new(),
            orig_width: 0,
            orig_height: 0,
        }
    }
}

#[derive(Clone)]
pub struct VideoTabPayload {
    pub position_ms: u64,
    pub is_playing: bool,
    pub volume: f32,
    pub duration_ms: u64,
}

impl Default for VideoTabPayload {
    fn default() -> Self {
        Self {
            position_ms: 0,
            is_playing: false,
            volume: 1.0,
            duration_ms: 0,
        }
    }
}

#[derive(Clone)]
pub enum TabPayload {
    Image(ImageTabPayload),
    Video(VideoTabPayload),
}

#[derive(Clone)]
pub struct MediaTabSession {
    pub id: TabId,
    pub media_id: MediaId,
    pub file_path: PathBuf,
    pub filename: String,
    pub format: MediaFormat,
    pub record: MediaRecord,
    pub payload: TabPayload,
    pub created_at: Instant,
    pub last_accessed: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerAction {
    NextItem,
    PreviousItem,
    CloseActiveTab,
    RefreshActiveTab,
}
