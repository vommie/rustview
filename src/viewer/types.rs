use super::mpv_engine::MpvPlayer;
use super::x11_embed::X11VideoSubwindow;
use crate::library::models::{MediaId, MediaRecord};
use crate::media::MediaFormat;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub type TabId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabContentType {
    Image,
    Video,
}

/// Live real-time snapshot of the embedded MPV player playback state with user-pause tracking.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MpvPlaybackState {
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_playing: bool,
    pub is_paused: bool,
    pub is_user_paused: bool,
    pub volume: f64,
    pub is_eof: bool,
    pub is_idle: bool,
    pub filename: String,
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

/// Dedicated, isolated video tab payload retaining its own independent MPV player and X11 subwindow instance.
#[derive(Clone)]
pub struct VideoTabPayload {
    pub player: Arc<MpvPlayer>,
    pub subwindow: Arc<Mutex<Option<X11VideoSubwindow>>>,
    pub position_ms: u64,
    pub is_playing: bool,
    pub volume: f32,
    pub duration_ms: u64,
    pub is_loading: bool,
    pub error_message: String,
}

impl std::fmt::Debug for VideoTabPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoTabPayload")
            .field("position_ms", &self.position_ms)
            .field("is_playing", &self.is_playing)
            .field("volume", &self.volume)
            .field("duration_ms", &self.duration_ms)
            .field("is_loading", &self.is_loading)
            .field("error_message", &self.error_message)
            .finish()
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
    ToggleFullscreen,
}
