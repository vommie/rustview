use super::mpv_engine::MpvPlayer;
use super::multiview::instance::SharedInstance;
use super::x11_embed::X11VideoSubwindow;
use crate::library::models::{MediaId, MediaRecord};
use crate::media::{MediaFormat, MediaType};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub type TabId = u64;

/// What a viewer tab shows: one image, one video or a running Multiview (several media).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabContentType {
    Image,
    Video,
    Multiview,
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

impl std::fmt::Debug for ImageTabPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageTabPayload")
            .field("has_high_res_pixels", &self.high_res_pixels.is_some())
            .field("is_loading", &self.is_loading)
            .field("error_message", &self.error_message)
            .field("orig_width", &self.orig_width)
            .field("orig_height", &self.orig_height)
            .finish()
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

/// A running Multiview owned by a tab. The instance is the only copy of its state; the tab just keeps it alive, and the last
/// owner to let go tears the players down in the safe order.
#[derive(Clone)]
pub struct MultiviewTabPayload {
    pub instance: SharedInstance,
    pub instance_id: u64,
}

impl std::fmt::Debug for MultiviewTabPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiviewTabPayload")
            .field("instance_id", &self.instance_id)
            .finish()
    }
}

#[derive(Clone)]
pub enum TabPayload {
    Image(ImageTabPayload),
    Video(VideoTabPayload),
    Multiview(MultiviewTabPayload),
}

impl std::fmt::Debug for TabPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Image(img) => f.debug_tuple("Image").field(img).finish(),
            Self::Video(vid) => f.debug_tuple("Video").field(vid).finish(),
            Self::Multiview(mv) => f.debug_tuple("Multiview").field(mv).finish(),
        }
    }
}

impl TabPayload {
    pub fn content_type(&self) -> TabContentType {
        match self {
            Self::Image(_) => TabContentType::Image,
            Self::Video(_) => TabContentType::Video,
            Self::Multiview(_) => TabContentType::Multiview,
        }
    }

    pub fn multiview(&self) -> Option<&MultiviewTabPayload> {
        match self {
            Self::Multiview(mv) => Some(mv),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct MediaTabSession {
    pub id: TabId,
    pub media_id: MediaId,
    pub file_path: PathBuf,
    pub filename: String,
    pub title: String,
    pub format: MediaFormat,
    pub record: MediaRecord,
    pub payload: TabPayload,
    pub created_at: Instant,
    pub last_accessed: Instant,
}

impl std::fmt::Debug for MediaTabSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaTabSession")
            .field("id", &self.id)
            .field("media_id", &self.media_id)
            .field("filename", &self.filename)
            .field("title", &self.title)
            .field("format", &self.format)
            .field("file_path", &self.file_path)
            .field("payload", &self.payload)
            .field("created_at", &self.created_at)
            .field("last_accessed", &self.last_accessed)
            .finish()
    }
}

/// `media_id` of a Multiview tab: real media ids start at 1, so lookups by media id never match a Multiview tab.
pub const MULTIVIEW_TAB_MEDIA_ID: MediaId = 0;

impl MediaTabSession {
    /// A tab that owns a running Multiview. The scalar fields are placeholders (no media, no path); everything that is
    /// specific to the Multiview is asked through the helpers below.
    pub fn for_multiview(id: TabId, title: String, instance: SharedInstance) -> Self {
        let instance_id = instance.lock().id();
        let now = Instant::now();
        Self {
            id,
            media_id: MULTIVIEW_TAB_MEDIA_ID,
            file_path: PathBuf::new(),
            filename: title.clone(),
            format: MediaFormat::Unknown,
            record: MediaRecord {
                id: MULTIVIEW_TAB_MEDIA_ID,
                folder_id: 0,
                filename: title.clone(),
                file_hash: [0; 16],
                file_size: 0,
                modification_time: 0,
                media_type: MediaType::Image,
                format: MediaFormat::Unknown,
                width: 0,
                height: 0,
                rating: 0,
                last_seen: 0,
                is_deleted: false,
                creation_time: 0,
                duration_ms: 0,
                is_corrupted: false,
                source: None,
            },
            title,
            payload: TabPayload::Multiview(MultiviewTabPayload {
                instance,
                instance_id,
            }),
            created_at: now,
            last_accessed: now,
        }
    }

    pub fn kind(&self) -> TabContentType {
        self.payload.content_type()
    }

    /// A tab with exactly one video (a Multiview is not a video tab).
    pub fn is_video(&self) -> bool {
        self.kind() == TabContentType::Video
    }

    pub fn is_image(&self) -> bool {
        self.kind() == TabContentType::Image
    }

    pub fn is_multiview(&self) -> bool {
        self.kind() == TabContentType::Multiview
    }

    pub fn multiview(&self) -> Option<&MultiviewTabPayload> {
        self.payload.multiview()
    }

    /// The one medium of a single-media tab; `None` for a Multiview.
    pub fn single_media_id(&self) -> Option<MediaId> {
        (!self.is_multiview()).then_some(self.media_id)
    }

    /// Every medium the tab shows (one for a single-media tab, the tiles for a Multiview).
    pub fn member_ids(&self) -> Vec<MediaId> {
        match &self.payload {
            TabPayload::Multiview(mv) => mv.instance.lock().member_ids(),
            _ => vec![self.media_id],
        }
    }

    pub fn has_member(&self, media_id: MediaId) -> bool {
        match &self.payload {
            TabPayload::Multiview(mv) => mv.instance.lock().has_member(media_id),
            _ => self.media_id == media_id,
        }
    }

    /// Pixel size the tab content is laid out in: the media size, or the virtual display of a Multiview.
    pub fn display_size(&self) -> (u32, u32) {
        match &self.payload {
            TabPayload::Image(img) => (img.orig_width, img.orig_height),
            TabPayload::Video(_) => (self.record.width, self.record.height),
            TabPayload::Multiview(mv) => {
                let container = mv.instance.lock().container();
                (container.width as u32, container.height as u32)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerAction {
    NextItem,
    PreviousItem,
    CloseActiveTab,
    RefreshActiveTab,
    ToggleFullscreen,
}
