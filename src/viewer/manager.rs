use super::error::ViewerError;
use super::types::{ImageTabPayload, MediaTabSession, TabId, TabPayload};
use crate::library::models::{MediaId, MediaRecord};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::LogObscurer;
use parking_lot::RwLock;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Maximum number of uncompressed high-resolution full-image buffers retained in RAM simultaneously.
/// Inactive tabs exceeding this threshold have their RAM pixel buffers evicted to prevent memory bloat.
pub const MAX_CACHED_HIGH_RES_BUFFERS: usize = 6;

/// Thread-safe manager coordinating open media tabs, active sessions, and image buffers.
pub struct ViewerTabManager {
    tabs: RwLock<Vec<MediaTabSession>>,
    active_tab_id: RwLock<Option<TabId>>,
    tab_counter: AtomicU64,
    library: Arc<Library>,
}

impl ViewerTabManager {
    pub fn new(library: Arc<Library>) -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            active_tab_id: RwLock::new(None),
            tab_counter: AtomicU64::new(1),
            library,
        }
    }

    /// Returns a snapshot of all currently open media tabs.
    pub fn list_tabs(&self) -> Vec<MediaTabSession> {
        self.tabs.read().clone()
    }

    /// Returns the active tab ID if a tab is currently selected, or `None` if in Browse mode.
    pub fn active_tab_id(&self) -> Option<TabId> {
        *self.active_tab_id.read()
    }

    /// Retrieves a cloned session of the currently active tab.
    pub fn get_active_tab(&self) -> Option<MediaTabSession> {
        let active = (*self.active_tab_id.read())?;
        self.tabs.read().iter().find(|t| t.id == active).cloned()
    }

    /// Returns the `MediaId` associated with the currently active tab.
    pub fn get_active_media_id(&self) -> Option<MediaId> {
        self.get_active_tab().map(|t| t.media_id)
    }

    /// Retrieves a tab session by its unique `TabId`.
    pub fn get_tab(&self, tab_id: TabId) -> Option<MediaTabSession> {
        self.tabs.read().iter().find(|t| t.id == tab_id).cloned()
    }

    /// Retrieves a tab session by its underlying `MediaId`.
    pub fn get_tab_by_media_id(&self, media_id: MediaId) -> Option<MediaTabSession> {
        self.tabs.read().iter().find(|t| t.media_id == media_id).cloned()
    }

    /// Opens a media item in a viewer tab. If a tab for this `media_id` already exists,
    /// it focuses that tab. Returns `(TabId, is_new_tab)`.
    pub fn open_or_focus_media(&self, media_id: MediaId) -> Result<(TabId, bool), ViewerError> {
        let now = Instant::now();
        let mut tabs = self.tabs.write();

        // 1. Focus existing tab if already open
        if let Some(existing) = tabs.iter_mut().find(|t| t.media_id == media_id) {
            let id = existing.id;
            existing.last_accessed = now;
            drop(tabs);

            *self.active_tab_id.write() = Some(id);
            tracing::info!(
                tab_id = id,
                media_id = media_id,
                "[ViewerTabManager: Focus] Reused and focused existing tab session"
            );
            return Ok((id, false));
        }

        // 2. Create new tab session from database record
        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;

        let file_path = self.library.resolve_media_path(media_id)?;

        let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);
        let filename = record.filename.clone();
        let format = record.format;
        let is_video = matches!(record.media_type, MediaType::Video);

        let payload = if is_video {
            TabPayload::Video(super::types::VideoTabPayload {
                position_ms: 0,
                is_playing: false,
                volume: 1.0,
                duration_ms: record.duration_ms,
            })
        } else {
            TabPayload::Image(ImageTabPayload {
                high_res_pixels: None,
                is_loading: true,
                error_message: String::new(),
                orig_width: record.width,
                orig_height: record.height,
            })
        };

        let session = MediaTabSession {
            id: new_id,
            media_id,
            filename: filename.clone(),
            format,
            file_path: file_path.clone(),
            record,
            payload,
            created_at: now,
            last_accessed: now,
        };

        tabs.push(session);
        *self.active_tab_id.write() = Some(new_id);

        tracing::info!(
            tab_id = new_id,
            media_id = media_id,
            filename = %LogObscurer::filename(&filename),
            file_path = %LogObscurer::path(&file_path),
            total_open_tabs = tabs.len(),
            "[ViewerTabManager: Open] Created and activated new viewer tab"
        );

        Ok((new_id, true))
    }

    /// Updates the currently active tab session with a new media item in-place (used for Prev/Next navigation without creating extra tabs).
    pub fn navigate_active_tab_to_media(&self, media_id: MediaId) -> Result<TabId, ViewerError> {
        let active_id = match *self.active_tab_id.read() {
            Some(id) => id,
            None => {
                let (id, _) = self.open_or_focus_media(media_id)?;
                return Ok(id);
            }
        };

        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;

        let file_path = self.library.resolve_media_path(media_id)?;
        let filename = record.filename.clone();
        let format = record.format;
        let is_video = matches!(record.media_type, MediaType::Video);
        let now = Instant::now();

        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == active_id) {
            tab.media_id = media_id;
            tab.filename = filename.clone();
            tab.format = format;
            tab.file_path = file_path;
            tab.record = record.clone();
            tab.last_accessed = now;

            tab.payload = if is_video {
                TabPayload::Video(super::types::VideoTabPayload {
                    position_ms: 0,
                    is_playing: false,
                    volume: 1.0,
                    duration_ms: record.duration_ms,
                })
            } else {
                TabPayload::Image(ImageTabPayload {
                    high_res_pixels: None,
                    is_loading: true,
                    error_message: String::new(),
                    orig_width: record.width,
                    orig_height: record.height,
                })
            };
        }

        tracing::info!(
            tab_id = active_id,
            media_id = media_id,
            filename = %LogObscurer::filename(&filename),
            "[ViewerTabManager: NavigateInPlace] Replaced active tab content in-place"
        );

        Ok(active_id)
    }

    /// Stores a loaded high-resolution pixel buffer into the specified tab and evicts oldest inactive buffers if exceeding RAM limit.
    pub fn update_tab_image_buffer(
        &self,
        tab_id: TabId,
        width: u32,
        height: u32,
        pixel_buffer: SharedPixelBuffer<Rgb8Pixel>,
    ) {
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id)
            && let TabPayload::Image(ref mut img) = tab.payload
        {
            img.high_res_pixels = Some(pixel_buffer);
            img.is_loading = false;
            img.error_message.clear();
            img.orig_width = width;
            img.orig_height = height;
            tab.last_accessed = Instant::now();
        }
        self.prune_inactive_high_res_buffers_locked(&mut tabs);
    }

    /// Sets an error message on a tab when image decoding fails.
    pub fn set_tab_error(&self, tab_id: TabId, error_msg: String) {
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id)
            && let TabPayload::Image(ref mut img) = tab.payload
        {
            img.is_loading = false;
            img.error_message = error_msg;
        }
    }

    /// Sets the active tab by `TabId`. Returns `true` if the tab exists and was activated.
    pub fn set_active_tab(&self, tab_id: TabId) -> bool {
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
            tab.last_accessed = Instant::now();
            *self.active_tab_id.write() = Some(tab_id);
            tracing::info!(tab_id, "[ViewerTabManager: Switch] Switched active tab");
            true
        } else {
            false
        }
    }

    /// Switches the workspace view back to the media browser (Browse mode).
    pub fn activate_browse(&self) {
        *self.active_tab_id.write() = None;
        tracing::info!("[ViewerTabManager: Browse] Switched active view to Browse mode");
    }

    /// Closes a tab by its `TabId`. If the closed tab was active, it selects an adjacent tab
    /// or falls back to Browse mode (`None`) if no tabs remain. Returns the new active `Option<TabId>`.
    pub fn close_tab(&self, tab_id: TabId) -> Option<TabId> {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let pos = tabs.iter().position(|t| t.id == tab_id)?;
        let removed = tabs.remove(pos);

        tracing::info!(
            tab_id = removed.id,
            media_id = removed.media_id,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: Close] Closed viewer tab"
        );

        if *active == Some(tab_id) {
            if tabs.is_empty() {
                *active = None;
            } else if pos < tabs.len() {
                *active = Some(tabs[pos].id);
            } else {
                *active = Some(tabs[pos - 1].id);
            }
        }

        *active
    }

    /// Closes all currently open tabs and returns to Browse mode.
    pub fn close_all_tabs(&self) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();
        tabs.clear();
        *active = None;
        tracing::info!("[ViewerTabManager: CloseAll] Closed all open viewer tabs");
    }

    /// Closes all tabs except the specified `keep_tab_id`.
    pub fn close_other_tabs(&self, keep_tab_id: TabId) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();
        tabs.retain(|t| t.id == keep_tab_id);
        if tabs.is_empty() {
            *active = None;
        } else {
            *active = Some(keep_tab_id);
        }
        tracing::info!(keep_tab_id, "[ViewerTabManager: CloseOthers] Closed other viewer tabs");
    }

    /// Updates file path, filename, and record references across open tabs when a media file is renamed.
    pub fn notify_media_renamed(&self, media_id: MediaId, new_filename: &str) {
        let mut tabs = self.tabs.write();
        for tab in tabs.iter_mut() {
            if tab.media_id == media_id {
                tab.filename = new_filename.to_string();
                if let Some(parent) = tab.file_path.parent() {
                    tab.file_path = parent.join(new_filename);
                }
                tab.record.filename = new_filename.to_string();
                tracing::info!(
                    tab_id = tab.id,
                    media_id = media_id,
                    new_filename = %LogObscurer::filename(new_filename),
                    "[ViewerTabManager: RenameSync] Synchronized renamed media filename in open tab session"
                );
            }
        }
    }

    /// Marks open tab sessions as deleted if their underlying media item is deleted.
    pub fn notify_media_deleted(&self, media_id: MediaId) {
        let mut tabs = self.tabs.write();
        for tab in tabs.iter_mut() {
            if tab.media_id == media_id {
                tab.record.is_deleted = true;
                tracing::info!(
                    tab_id = tab.id,
                    media_id = media_id,
                    "[ViewerTabManager: DeleteSync] Marked open tab record as deleted"
                );
            }
        }
    }

    /// Synchronizes updated record attributes (rating, dimensions, duration) in open tab sessions.
    pub fn notify_media_refreshed(&self, media_id: MediaId, updated_record: &MediaRecord) {
        let mut tabs = self.tabs.write();
        for tab in tabs.iter_mut() {
            if tab.media_id == media_id {
                tab.record = updated_record.clone();
                if let TabPayload::Image(ref mut img) = tab.payload {
                    img.orig_width = updated_record.width;
                    img.orig_height = updated_record.height;
                }
                tracing::info!(
                    tab_id = tab.id,
                    media_id = media_id,
                    "[ViewerTabManager: RefreshSync] Synchronized refreshed record attributes in open tab session"
                );
            }
        }
    }

    /// Evicts full-resolution image buffers of oldest inactive tabs when exceeding RAM capacity limit.
    fn prune_inactive_high_res_buffers_locked(&self, tabs: &mut [MediaTabSession]) {
        let active_id = *self.active_tab_id.read();
        let cached_count = tabs
            .iter()
            .filter(|t| matches!(&t.payload, TabPayload::Image(img) if img.high_res_pixels.is_some()))
            .count();

        if cached_count <= MAX_CACHED_HIGH_RES_BUFFERS {
            return;
        }

        let mut inactive_indices: Vec<(usize, Instant)> = tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                Some(t.id) != active_id
                    && matches!(&t.payload, TabPayload::Image(img) if img.high_res_pixels.is_some())
            })
            .map(|(idx, t)| (idx, t.last_accessed))
            .collect();

        inactive_indices.sort_by_key(|&(_, last)| last);

        let to_evict_count = cached_count.saturating_sub(MAX_CACHED_HIGH_RES_BUFFERS);
        for &(idx, _) in inactive_indices.iter().take(to_evict_count) {
            if let TabPayload::Image(ref mut img) = tabs[idx].payload {
                img.high_res_pixels = None;
                img.is_loading = false;
                tracing::debug!(
                    tab_id = tabs[idx].id,
                    media_id = tabs[idx].media_id,
                    "[ViewerTabManager: Evict] Evicted inactive high-resolution pixel buffer from RAM"
                );
            }
        }
    }
}
