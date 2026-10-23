use super::multiview::instance::SharedInstance;
use crate::library::models::{MediaId, MediaRecord};
use crate::media::{MediaFormat, MediaType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

pub type TabId = u64;

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
pub struct MediaTabSession {
    pub id: TabId,
    pub media_id: MediaId,
    pub file_path: PathBuf,
    pub filename: String,
    pub title: String,
    pub format: MediaFormat,
    pub record: MediaRecord,
    /// The Multiview the tab shows: one tile for a tab of one medium, up to four for a Multiview.
    pub multiview: MultiviewTabPayload,
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
            .field("multiview", &self.multiview)
            .field("created_at", &self.created_at)
            .field("last_accessed", &self.last_accessed)
            .finish()
    }
}

/// `media_id` of a tab that shows several media (or none): real media ids start at 1, so lookups by media id never match it.
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
            record: Self::placeholder_record(&title),
            title,
            multiview: MultiviewTabPayload {
                instance,
                instance_id,
            },
            created_at: now,
            last_accessed: now,
        }
    }

    /// A tab that shows one medium: a Multiview of exactly one tile. The scalar fields describe that medium (as long as the tab shows
    /// it; the manager keeps them in line with the tile, see `ViewerTabManager::sync_tab_identities`), so lookups by medium, the tab
    /// bar and the menus treat it like the tab of the medium it is.
    pub fn for_solo(id: TabId, title: String, record: MediaRecord, file_path: PathBuf, instance: SharedInstance) -> Self {
        let mut session = Self::for_multiview(id, title, instance);
        session.set_solo_identity(record, file_path);
        session
    }

    /// Makes the scalar fields describe the one medium of the tab.
    pub fn set_solo_identity(&mut self, record: MediaRecord, file_path: PathBuf) {
        self.media_id = record.id;
        self.filename = record.filename.clone();
        self.format = record.format;
        self.file_path = file_path;
        self.record = record;
    }

    /// Makes the scalar fields placeholders again (the tab shows several media, or none): no medium, no path.
    pub fn clear_solo_identity(&mut self) {
        self.media_id = MULTIVIEW_TAB_MEDIA_ID;
        self.file_path = PathBuf::new();
        self.format = MediaFormat::Unknown;
        self.filename = self.title.clone();
        self.record = Self::placeholder_record(&self.title);
    }

    fn placeholder_record(title: &str) -> MediaRecord {
        MediaRecord {
            id: MULTIVIEW_TAB_MEDIA_ID,
            folder_id: 0,
            filename: title.to_string(),
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
        }
    }

    /// The one medium of a tab that shows exactly one; `None` for a tab of several media.
    pub fn single_media_id(&self) -> Option<MediaId> {
        (self.media_id != MULTIVIEW_TAB_MEDIA_ID).then_some(self.media_id)
    }

    /// Every medium the tab shows (one for a tab of one medium, the tiles otherwise).
    pub fn member_ids(&self) -> Vec<MediaId> {
        self.multiview.instance.lock().member_ids()
    }

    pub fn has_member(&self, media_id: MediaId) -> bool {
        self.multiview.instance.lock().has_member(media_id)
    }

    /// Pixel size the tab content is laid out in: the virtual display of the Multiview.
    pub fn display_size(&self) -> (u32, u32) {
        let container = self.multiview.instance.lock().container();
        (container.width as u32, container.height as u32)
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
