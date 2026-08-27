use super::error::ViewerError;
use super::mpv_engine::MpvPlayer;
use super::types::{ImageTabPayload, MediaTabSession, TabId, TabPayload, VideoTabPayload};
use super::x11_embed::X11VideoSubwindow;
use crate::library::models::{MediaId, MediaRecord};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::LogObscurer;
use parking_lot::{Mutex, RwLock};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Maximum number of uncompressed high-resolution full-image buffers retained in RAM simultaneously.
pub const MAX_CACHED_HIGH_RES_BUFFERS: usize = 6;

/// Thread-safe manager coordinating open media tabs, isolated per-tab MPV player instances, and image buffers.
pub struct ViewerTabManager {
    tabs: RwLock<Vec<MediaTabSession>>,
    active_tab_id: RwLock<Option<TabId>>,
    tab_counter: AtomicU64,
    library: Arc<Library>,
    main_window_xid: RwLock<Option<u64>>,
    cached_viewport_rect: RwLock<(i32, i32, u32, u32)>,
}

impl ViewerTabManager {
    pub fn new(library: Arc<Library>) -> Self {
        Self {
            tabs: RwLock::new(Vec::new()),
            active_tab_id: RwLock::new(None),
            tab_counter: AtomicU64::new(1),
            library,
            main_window_xid: RwLock::new(None),
            cached_viewport_rect: RwLock::new((0, 64, 800, 600)),
        }
    }

    /// Stores the main window XID and latest viewport geometry for initializing new video tabs.
    pub fn set_main_viewport_info(&self, main_xid: u64, x: i32, y: i32, w: u32, h: u32) {
        *self.main_window_xid.write() = Some(main_xid);
        *self.cached_viewport_rect.write() = (x, y, w, h);
    }

    /// Returns the MPV player associated with the currently active tab if it is a video tab.
    pub fn get_active_player(&self) -> Option<Arc<MpvPlayer>> {
        let active = (*self.active_tab_id.read())?;
        let tabs = self.tabs.read();
        let tab = tabs.iter().find(|t| t.id == active)?;
        if let TabPayload::Video(ref vid) = tab.payload {
            Some(vid.player.clone())
        } else {
            None
        }
    }

    /// Convenience alias returning the active video player.
    #[inline]
    pub fn player(&self) -> Option<Arc<MpvPlayer>> {
        self.get_active_player()
    }

    /// Returns the X11 subwindow container associated with the currently active tab if it is a video tab.
    pub fn get_active_subwindow(&self) -> Option<Arc<Mutex<Option<X11VideoSubwindow>>>> {
        let active = (*self.active_tab_id.read())?;
        let tabs = self.tabs.read();
        let tab = tabs.iter().find(|t| t.id == active)?;
        if let TabPayload::Video(ref vid) = tab.payload {
            Some(vid.subwindow.clone())
        } else {
            None
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

    /// Retrieves all tab sessions referencing the given `MediaId`.
    pub fn get_tabs_by_media_id(&self, media_id: MediaId) -> Vec<MediaTabSession> {
        self.tabs
            .read()
            .iter()
            .filter(|t| t.media_id == media_id)
            .cloned()
            .collect()
    }

    /// Computes the formatted display title for a tab, adding an iteration suffix `(1)`, `(2)`, etc. if multiple tabs exist for the same media item.
    pub fn compute_tab_title(tabs: &[MediaTabSession], tab_id: TabId) -> String {
        let Some(current_tab) = tabs.iter().find(|t| t.id == tab_id) else {
            return String::new();
        };

        let duplicate_index = tabs
            .iter()
            .filter(|t| t.media_id == current_tab.media_id)
            .position(|t| t.id == tab_id)
            .unwrap_or(0);

        if duplicate_index > 0 {
            format!("{} ({})", current_tab.filename, duplicate_index)
        } else {
            current_tab.filename.clone()
        }
    }

    /// Opens a media item in a viewer tab.
    /// If `allow_multiple` is false and a tab for this `media_id` already exists, focuses it without reloading.
    /// If `allow_multiple` is true, creates a completely new independent tab session with separate MPV/image instances.
    /// Returns `(TabId, is_new_tab)`.
    pub fn open_or_focus_media(
        &self,
        media_id: MediaId,
        allow_multiple: bool,
    ) -> Result<(TabId, bool), ViewerError> {
        let now = Instant::now();
        let mut tabs = self.tabs.write();

        // 1. Focus existing tab if already open and multiple tabs are NOT allowed
        if !allow_multiple
            && let Some(existing) = tabs.iter_mut().find(|t| t.media_id == media_id)
        {
            let id = existing.id;
            existing.last_accessed = now;
            let is_video = matches!(existing.payload, TabPayload::Video(_));
            drop(tabs);

            self.set_active_tab_internal(id);

            tracing::info!(
                tab_id = id,
                media_id = media_id,
                is_video,
                "[ViewerTabManager: Focus] Reused and focused existing tab session with preserved state"
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
            let player = Arc::new(MpvPlayer::new()?);
            let subwindow_slot = Arc::new(Mutex::new(None));

            let main_xid_opt = *self.main_window_xid.read();
            let (x, y, w, h) = *self.cached_viewport_rect.read();

            if let Some(main_xid) = main_xid_opt
                && let Ok(mut sub) = X11VideoSubwindow::new(main_xid, x, y, w, h)
            {
                let wid = sub.xid();
                let _ = player.attach_to_window(wid);
                sub.show();
                *subwindow_slot.lock() = Some(sub);
            }

            let _ = player.load_file(&file_path);

            TabPayload::Video(VideoTabPayload {
                player,
                subwindow: subwindow_slot,
                position_ms: 0,
                is_playing: true,
                volume: 1.0,
                duration_ms: record.duration_ms,
                is_loading: false,
                error_message: String::new(),
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

        // Auto-pause previously active video tab before activating the new tab
        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.iter_mut().find(|t| t.id == current_id)
            && let TabPayload::Video(ref vid) = current_tab.payload
        {
            vid.player.set_view_active(false);
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }

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
            is_video,
            allow_multiple,
            filename = %LogObscurer::filename(&filename),
            file_path = %LogObscurer::path(&file_path),
            total_open_tabs = tabs.len(),
            "[ViewerTabManager: Open] Created and activated new viewer tab session"
        );

        Ok((new_id, true))
    }

    /// Updates the currently active tab session with a new media item in-place (used for Prev/Next navigation).
    pub fn navigate_active_tab_to_media(&self, media_id: MediaId) -> Result<TabId, ViewerError> {
        let active_id = match *self.active_tab_id.read() {
            Some(id) => id,
            None => {
                let (id, _) = self.open_or_focus_media(media_id, false)?;
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
            tab.file_path = file_path.clone();
            tab.record = record.clone();
            tab.last_accessed = now;

            if is_video {
                if let TabPayload::Video(ref mut vid) = tab.payload {
                    let _ = vid.player.load_file(&file_path);
                    vid.player.set_view_active(true);
                    if let Some(ref mut sub) = *vid.subwindow.lock() {
                        sub.show();
                    }
                } else {
                    let player = Arc::new(MpvPlayer::new()?);
                    let subwindow_slot = Arc::new(Mutex::new(None));
                    let main_xid_opt = *self.main_window_xid.read();
                    let (x, y, w, h) = *self.cached_viewport_rect.read();

                    if let Some(main_xid) = main_xid_opt
                        && let Ok(mut sub) = X11VideoSubwindow::new(main_xid, x, y, w, h)
                    {
                        let wid = sub.xid();
                        let _ = player.attach_to_window(wid);
                        sub.show();
                        *subwindow_slot.lock() = Some(sub);
                    }

                    let _ = player.load_file(&file_path);

                    tab.payload = TabPayload::Video(VideoTabPayload {
                        player,
                        subwindow: subwindow_slot,
                        position_ms: 0,
                        is_playing: true,
                        volume: 1.0,
                        duration_ms: record.duration_ms,
                        is_loading: false,
                        error_message: String::new(),
                    });
                }
            } else {
                if let TabPayload::Video(ref vid) = tab.payload {
                    vid.player.stop();
                    if let Some(ref mut sub) = *vid.subwindow.lock() {
                        sub.hide();
                    }
                }

                tab.payload = TabPayload::Image(ImageTabPayload {
                    high_res_pixels: None,
                    is_loading: true,
                    error_message: String::new(),
                    orig_width: record.width,
                    orig_height: record.height,
                });
            }
        }

        tracing::info!(
            tab_id = active_id,
            media_id = media_id,
            is_video,
            filename = %LogObscurer::filename(&filename),
            "[ViewerTabManager: NavigateInPlace] Replaced active tab content in-place"
        );

        Ok(active_id)
    }

    /// Stores a loaded high-resolution pixel buffer into the specified tab.
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

    /// Sets an error message on a tab when media decoding fails.
    pub fn set_tab_error(&self, tab_id: TabId, error_msg: String) {
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
            match tab.payload {
                TabPayload::Image(ref mut img) => {
                    img.is_loading = false;
                    img.error_message = error_msg;
                }
                TabPayload::Video(ref mut vid) => {
                    vid.is_loading = false;
                    vid.error_message = error_msg;
                }
            }
        }
    }

    /// Sets the active tab by `TabId`, auto-pausing the previous tab and auto-resuming the new tab if not user-paused.
    pub fn set_active_tab(&self, tab_id: TabId) -> bool {
        self.set_active_tab_internal(tab_id)
    }

    fn set_active_tab_internal(&self, tab_id: TabId) -> bool {
        let mut tabs = self.tabs.write();
        let current_active = *self.active_tab_id.read();

        if current_active == Some(tab_id) {
            return true;
        }

        // 1. Auto-pause current active video tab without resetting playback position
        if let Some(prev_id) = current_active
            && let Some(prev_tab) = tabs.iter_mut().find(|t| t.id == prev_id)
            && let TabPayload::Video(ref vid) = prev_tab.payload
        {
            vid.player.set_view_active(false);
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }

        // 2. Activate target tab (auto-resumes video ONLY if it was not user-paused)
        if let Some(new_tab) = tabs.iter_mut().find(|t| t.id == tab_id) {
            new_tab.last_accessed = Instant::now();
            *self.active_tab_id.write() = Some(tab_id);

            if let TabPayload::Video(ref vid) = new_tab.payload {
                let main_xid_opt = *self.main_window_xid.read();
                let (x, y, w, h) = *self.cached_viewport_rect.read();

                let mut sub_guard = vid.subwindow.lock();
                if let Some(ref mut sub) = *sub_guard {
                    sub.move_resize(x, y, w, h);
                    sub.show();
                } else if let Some(main_xid) = main_xid_opt
                    && let Ok(mut sub) = X11VideoSubwindow::new(main_xid, x, y, w, h)
                {
                    let wid = sub.xid();
                    let _ = vid.player.attach_to_window(wid);
                    sub.show();
                    *sub_guard = Some(sub);
                }

                vid.player.set_view_active(true);
            }

            tracing::info!(
                tab_id,
                "[ViewerTabManager: Switch] Switched active tab with intelligent user-pause vs auto-resume handling"
            );
            true
        } else {
            false
        }
    }

    /// Switches the workspace view back to the media browser (Browse mode), cleanly auto-pausing video tabs.
    pub fn activate_browse(&self) {
        let mut tabs = self.tabs.write();
        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.iter_mut().find(|t| t.id == current_id)
            && let TabPayload::Video(ref vid) = current_tab.payload
        {
            vid.player.set_view_active(false);
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }
        *self.active_tab_id.write() = None;
        tracing::info!("[ViewerTabManager: Browse] Switched active view to Browse mode (all video tabs auto-paused in RAM)");
    }

    /// Closes a tab by its `TabId`. Only on close are the tab's MPV and X11 subwindow resources destroyed.
    pub fn close_tab(&self, tab_id: TabId) -> Option<TabId> {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let pos = tabs.iter().position(|t| t.id == tab_id)?;
        let removed = tabs.remove(pos);

        // Only on actual tab close: stop player and hide/destroy subwindow
        if let TabPayload::Video(ref vid) = removed.payload {
            vid.player.stop();
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }

        tracing::info!(
            tab_id = removed.id,
            media_id = removed.media_id,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: Close] Closed viewer tab and freed associated resources"
        );

        if *active == Some(tab_id) {
            if tabs.is_empty() {
                *active = None;
            } else {
                let next_idx = if pos < tabs.len() { pos } else { pos - 1 };
                let next_id = tabs[next_idx].id;
                *active = Some(next_id);

                if let TabPayload::Video(ref vid) = tabs[next_idx].payload {
                    let main_xid_opt = *self.main_window_xid.read();
                    let (x, y, w, h) = *self.cached_viewport_rect.read();

                    let mut sub_guard = vid.subwindow.lock();
                    if let Some(ref mut sub) = *sub_guard {
                        sub.move_resize(x, y, w, h);
                        sub.show();
                    } else if let Some(main_xid) = main_xid_opt
                        && let Ok(mut sub) = X11VideoSubwindow::new(main_xid, x, y, w, h)
                    {
                        let wid = sub.xid();
                        let _ = vid.player.attach_to_window(wid);
                        sub.show();
                        *sub_guard = Some(sub);
                    }

                    vid.player.set_view_active(true);
                }
            }
        }

        *active
    }

    /// Closes all currently open tabs and returns to Browse mode.
    pub fn close_all_tabs(&self) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        for tab in tabs.iter() {
            if let TabPayload::Video(ref vid) = tab.payload {
                vid.player.stop();
                if let Some(ref mut sub) = *vid.subwindow.lock() {
                    sub.hide();
                }
            }
        }

        tabs.clear();
        *active = None;
        tracing::info!("[ViewerTabManager: CloseAll] Closed all open viewer tabs and freed resources");
    }

    /// Closes all tabs except the specified `keep_tab_id`.
    pub fn close_other_tabs(&self, keep_tab_id: TabId) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        for tab in tabs.iter() {
            if tab.id != keep_tab_id
                && let TabPayload::Video(ref vid) = tab.payload
            {
                vid.player.stop();
                if let Some(ref mut sub) = *vid.subwindow.lock() {
                    sub.hide();
                }
            }
        }

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
                match tab.payload {
                    TabPayload::Image(ref mut img) => {
                        img.orig_width = updated_record.width;
                        img.orig_height = updated_record.height;
                    }
                    TabPayload::Video(ref mut vid) => {
                        vid.duration_ms = updated_record.duration_ms;
                    }
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
