use super::error::ViewerError;
use super::mpv_engine::MpvPlayer;
use super::tab_item::{ImageBufferEvictor, TabCollection, TabSessionFactory, TabTitleFormatter};
use super::types::{ImageTabPayload, MediaTabSession, TabId, TabPayload, VideoTabPayload};
use super::x11_embed::{X11VideoSubwindow, XRectangle};
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

/// High-level workspace coordinator managing open tabs, player lifecycles, and window embedding (SRP/OCP).
pub struct ViewerTabManager {
    tabs: RwLock<TabCollection>,
    active_tab_id: RwLock<Option<TabId>>,
    tab_counter: AtomicU64,
    library: Arc<Library>,
    main_window_xid: RwLock<Option<u64>>,
    cached_viewport_rect: RwLock<(i32, i32, u32, u32)>,
}

impl ViewerTabManager {
    pub fn new(library: Arc<Library>) -> Self {
        Self {
            tabs: RwLock::new(TabCollection::new()),
            active_tab_id: RwLock::new(None),
            tab_counter: AtomicU64::new(1),
            library,
            main_window_xid: RwLock::new(None),
            cached_viewport_rect: RwLock::new((0, 64, 800, 600)),
        }
    }

    // =========================================================================
    // Viewport & Window Handle Configuration
    // =========================================================================

    /// Stores the main window XID and latest viewport geometry for initializing video tabs.
    pub fn set_main_viewport_info(&self, main_xid: u64, x: i32, y: i32, w: u32, h: u32) {
        *self.main_window_xid.write() = Some(main_xid);
        *self.cached_viewport_rect.write() = (x, y, w, h);
    }

    pub fn get_viewport_rect(&self) -> (i32, i32, u32, u32) {
        *self.cached_viewport_rect.read()
    }

    pub fn get_main_window_xid(&self) -> Option<u64> {
        *self.main_window_xid.read()
    }

    // =========================================================================
    // Active Player & Subwindow Queries & Clipping
    // =========================================================================

    /// Returns the MPV player associated with the currently active tab if it is a video tab.
    pub fn get_active_player(&self) -> Option<Arc<MpvPlayer>> {
        let active = (*self.active_tab_id.read())?;
        let tabs = self.tabs.read();
        let tab = tabs.get(active)?;
        if let TabPayload::Video(ref vid) = tab.payload {
            Some(vid.player.clone())
        } else {
            None
        }
    }

    #[inline]
    pub fn player(&self) -> Option<Arc<MpvPlayer>> {
        self.get_active_player()
    }

    /// Returns the X11 subwindow container associated with the currently active tab if it is a video tab.
    pub fn get_active_subwindow(&self) -> Option<Arc<Mutex<Option<X11VideoSubwindow>>>> {
        let active = (*self.active_tab_id.read())?;
        let tabs = self.tabs.read();
        let tab = tabs.get(active)?;
        if let TabPayload::Video(ref vid) = tab.payload {
            Some(vid.subwindow.clone())
        } else {
            None
        }
    }

    /// Clips out a cutout shape from the active video subwindow so context menus display seamlessly on top.
    /// The video continues playing and is visible everywhere outside the cutout!
    pub fn clip_video_subwindow(&self, rects: &[XRectangle]) {
        if let Some(sub_slot) = self.get_active_subwindow()
            && let Some(ref mut sub) = *sub_slot.lock()
        {
            sub.clip_cutout_rectangles(rects);
        }
    }

    /// Resets the active video subwindow shape to full bounding rectangle.
    pub fn reset_video_subwindow_clip(&self) {
        if let Some(sub_slot) = self.get_active_subwindow()
            && let Some(ref mut sub) = *sub_slot.lock()
        {
            sub.reset_clip_shape();
        }
    }

    // =========================================================================
    // Queries & SPoT Title Calculation
    // =========================================================================

    pub fn list_tabs(&self) -> Vec<MediaTabSession> {
        self.tabs.read().to_vec()
    }

    #[inline]
    pub fn tab_count(&self) -> usize {
        self.tabs.read().len()
    }

    #[inline]
    pub fn active_tab_id(&self) -> Option<TabId> {
        *self.active_tab_id.read()
    }

    pub fn get_active_tab(&self) -> Option<MediaTabSession> {
        let active = (*self.active_tab_id.read())?;
        self.tabs.read().get(active).cloned()
    }

    pub fn get_active_media_id(&self) -> Option<MediaId> {
        self.get_active_tab().map(|t| t.media_id)
    }

    pub fn get_tab(&self, tab_id: TabId) -> Option<MediaTabSession> {
        self.tabs.read().get(tab_id).cloned()
    }

    pub fn get_tab_index(&self, tab_id: TabId) -> Option<usize> {
        self.tabs.read().index_of(tab_id)
    }

    pub fn get_tab_by_media_id(&self, media_id: MediaId) -> Option<MediaTabSession> {
        self.tabs.read().get_by_media_id(media_id).cloned()
    }

    pub fn get_tabs_by_media_id(&self, media_id: MediaId) -> Vec<MediaTabSession> {
        self.tabs.read().get_all_by_media_id(media_id)
    }

    /// Single Point of Truth delegating tab title formatting to `TabTitleFormatter`.
    #[inline]
    pub fn compute_tab_title(tabs: &[MediaTabSession], tab_id: TabId) -> String {
        TabTitleFormatter::compute_title(tabs, tab_id)
    }

    // =========================================================================
    // Tab Movement & Repositioning Operations (Modular API)
    // =========================================================================

    pub fn move_tab(&self, tab_id: TabId, target_index: usize) -> bool {
        let mut tabs = self.tabs.write();
        let res = tabs.move_tab(tab_id, target_index);
        if res {
            tracing::info!(
                tab_id,
                target_index,
                "[ViewerTabManager: MoveTab] Reordered tab to target index"
            );
        }
        res
    }

    pub fn move_tab_to_start(&self, tab_id: TabId) -> bool {
        self.move_tab(tab_id, 0)
    }

    pub fn move_tab_left(&self, tab_id: TabId) -> bool {
        let pos = match self.get_tab_index(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos > 0 {
            self.move_tab(tab_id, pos - 1)
        } else {
            false
        }
    }

    pub fn move_tab_right(&self, tab_id: TabId) -> bool {
        let pos = match self.get_tab_index(tab_id) {
            Some(p) => p,
            None => return false,
        };
        let count = self.tab_count();
        if pos + 1 < count {
            self.move_tab(tab_id, pos + 1)
        } else {
            false
        }
    }

    pub fn move_tab_to_end(&self, tab_id: TabId) -> bool {
        let count = self.tab_count();
        if count > 0 {
            self.move_tab(tab_id, count - 1)
        } else {
            false
        }
    }

    // =========================================================================
    // Tab Duplication, Opening & In-Place Navigation
    // =========================================================================

    /// Duplicates an existing tab, placing the clone immediately adjacent to the source tab.
    pub fn duplicate_tab(&self, source_tab_id: TabId) -> Result<TabId, ViewerError> {
        let source_session = self
            .get_tab(source_tab_id)
            .ok_or(ViewerError::MediaNotFound(source_tab_id))?;

        let (new_tab_id, _) = self.open_or_focus_media(source_session.media_id, true)?;

        if let Some(source_index) = self.get_tab_index(source_tab_id) {
            self.move_tab(new_tab_id, source_index + 1);
        }
        self.set_active_tab_internal(new_tab_id);

        tracing::info!(
            source_tab_id,
            new_tab_id,
            media_id = source_session.media_id,
            "[ViewerTabManager: Duplicate] Duplicated tab session placed adjacent to source"
        );

        Ok(new_tab_id)
    }

    /// Opens a media item in a viewer tab. Reuses existing tab if `allow_multiple` is false.
    pub fn open_or_focus_media(
        &self,
        media_id: MediaId,
        allow_multiple: bool,
    ) -> Result<(TabId, bool), ViewerError> {
        let mut tabs = self.tabs.write();

        // 1. Focus existing tab if duplicates are disabled
        if !allow_multiple
            && let Some(existing) = tabs.get_by_media_id_mut(media_id)
        {
            let id = existing.id;
            existing.last_accessed = Instant::now();
            drop(tabs);

            self.set_active_tab_internal(id);
            tracing::info!(
                tab_id = id,
                media_id,
                "[ViewerTabManager: Focus] Focused existing tab session"
            );
            return Ok((id, false));
        }

        // 2. Resolve media record and path
        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;

        let file_path = self.library.resolve_media_path(media_id)?;
        let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);
        let main_xid_opt = *self.main_window_xid.read();
        let viewport_rect = *self.cached_viewport_rect.read();

        // 3. Pause active video tab prior to activating the new one
        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.get_mut(current_id)
        {
            Self::pause_session_playback(current_tab);
        }

        // 4. Allocate a permanent unique display title for the new tab session
        let title = TabTitleFormatter::allocate_tab_title(
            tabs.all(),
            media_id,
            &record.filename,
        );

        // 5. Construct session via TabSessionFactory (OCP)
        let session = TabSessionFactory::create_session(
            new_id,
            record,
            file_path.clone(),
            title,
            main_xid_opt,
            viewport_rect,
        )?;

        tabs.push(session);
        *self.active_tab_id.write() = Some(new_id);

        tracing::info!(
            tab_id = new_id,
            media_id,
            total_open_tabs = tabs.len(),
            file_path = %LogObscurer::path(&file_path),
            "[ViewerTabManager: Open] Created new viewer tab session"
        );

        Ok((new_id, true))
    }

    /// Replaces the media content of the active tab in-place (used for Prev/Next navigation).
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
        let main_xid_opt = *self.main_window_xid.read();
        let viewport_rect = *self.cached_viewport_rect.read();

        let mut tabs = self.tabs.write();

        // Allocate a new permanent title excluding the active tab being updated
        let remaining_tabs: Vec<MediaTabSession> = tabs
            .all()
            .iter()
            .filter(|t| t.id != active_id)
            .cloned()
            .collect();

        let new_title = TabTitleFormatter::allocate_tab_title(
            &remaining_tabs,
            media_id,
            &record.filename,
        );

        if let Some(tab) = tabs.get_mut(active_id) {
            Self::update_session_media(tab, record, file_path, new_title, main_xid_opt, viewport_rect)?;
        }

        tracing::info!(
            tab_id = active_id,
            media_id,
            "[ViewerTabManager: NavigateInPlace] Navigated active tab in-place"
        );

        Ok(active_id)
    }

    // =========================================================================
    // Image Buffer & Error Management
    // =========================================================================

    pub fn update_tab_image_buffer(
        &self,
        tab_id: TabId,
        width: u32,
        height: u32,
        pixel_buffer: SharedPixelBuffer<Rgb8Pixel>,
    ) {
        let active_id = *self.active_tab_id.read();
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.get_mut(tab_id)
            && let TabPayload::Image(ref mut img) = tab.payload
        {
            img.high_res_pixels = Some(pixel_buffer);
            img.is_loading = false;
            img.error_message.clear();
            img.orig_width = width;
            img.orig_height = height;
            tab.last_accessed = Instant::now();
        }

        ImageBufferEvictor::evict_excess(
            &mut tabs.to_vec(),
            active_id,
            MAX_CACHED_HIGH_RES_BUFFERS,
        );
    }

    pub fn set_tab_error(&self, tab_id: TabId, error_msg: String) {
        let mut tabs = self.tabs.write();
        if let Some(tab) = tabs.get_mut(tab_id) {
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

    // =========================================================================
    // Tab Activation, Browsing & Closing
    // =========================================================================

    pub fn set_active_tab(&self, tab_id: TabId) -> bool {
        self.set_active_tab_internal(tab_id)
    }

    fn set_active_tab_internal(&self, tab_id: TabId) -> bool {
        let current_active = *self.active_tab_id.read();
        if current_active == Some(tab_id) {
            return true;
        }

        let main_xid_opt = *self.main_window_xid.read();
        let viewport_rect = *self.cached_viewport_rect.read();

        let mut tabs = self.tabs.write();

        // 1. Auto-pause previous video tab
        if let Some(prev_id) = current_active
            && let Some(prev_tab) = tabs.get_mut(prev_id)
        {
            Self::pause_session_playback(prev_tab);
        }

        // 2. Activate target tab
        if let Some(new_tab) = tabs.get_mut(tab_id) {
            new_tab.last_accessed = Instant::now();
            *self.active_tab_id.write() = Some(tab_id);
            Self::resume_session_playback(new_tab, main_xid_opt, viewport_rect);
            tracing::info!(tab_id, "[ViewerTabManager: Switch] Activated viewer tab");
            true
        } else {
            false
        }
    }

    pub fn activate_browse(&self) {
        let mut tabs = self.tabs.write();
        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.get_mut(current_id)
        {
            Self::pause_session_playback(current_tab);
        }
        *self.active_tab_id.write() = None;
        tracing::info!("[ViewerTabManager: Browse] Returned to library media browser");
    }

    pub fn close_tab(&self, tab_id: TabId) -> Option<TabId> {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let (pos, removed) = tabs.remove(tab_id)?;
        Self::cleanup_session_resources(&removed);

        tracing::info!(
            tab_id = removed.id,
            remaining = tabs.len(),
            "[ViewerTabManager: Close] Closed tab session"
        );

        if *active == Some(tab_id) {
            *active = Self::resolve_next_active_tab(&mut tabs, pos, *self.main_window_xid.read(), *self.cached_viewport_rect.read());
        }

        *active
    }

    pub fn close_all_tabs(&self) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let evicted = tabs.clear();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        *active = None;
        tracing::info!("[ViewerTabManager: CloseAll] Closed all open viewer tabs");
    }

    pub fn close_other_tabs(&self, keep_tab_id: TabId) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let evicted = tabs.retain_only(keep_tab_id);
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        if tabs.is_empty() {
            *active = None;
        } else {
            *active = Some(keep_tab_id);
        }
    }

    // =========================================================================
    // Reactive Event Synchronization
    // =========================================================================

    pub fn notify_media_renamed(&self, media_id: MediaId, new_filename: &str) {
        self.tabs.write().update_filename(media_id, new_filename);
    }

    pub fn notify_media_deleted(&self, media_id: MediaId) {
        self.tabs.write().mark_deleted(media_id);
    }

    pub fn notify_media_refreshed(&self, media_id: MediaId, updated_record: &MediaRecord) {
        self.tabs.write().update_record(media_id, updated_record);
    }

    // =========================================================================
    // Internal Helper Methods
    // =========================================================================

    fn pause_session_playback(tab: &mut MediaTabSession) {
        if let TabPayload::Video(ref vid) = tab.payload {
            vid.player.set_view_active(false);
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }
    }

    fn resume_session_playback(
        tab: &mut MediaTabSession,
        main_xid_opt: Option<u64>,
        (x, y, w, h): (i32, i32, u32, u32),
    ) {
        if let TabPayload::Video(ref vid) = tab.payload {
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

    fn cleanup_session_resources(session: &MediaTabSession) {
        if let TabPayload::Video(ref vid) = session.payload {
            vid.player.stop();
            if let Some(ref mut sub) = *vid.subwindow.lock() {
                sub.hide();
            }
        }
    }

    fn resolve_next_active_tab(
        tabs: &mut TabCollection,
        closed_index: usize,
        main_xid_opt: Option<u64>,
        viewport_rect: (i32, i32, u32, u32),
    ) -> Option<TabId> {
        if tabs.is_empty() {
            return None;
        }

        let next_idx = if closed_index < tabs.len() {
            closed_index
        } else {
            closed_index.saturating_sub(1)
        };

        let next_tab = tabs.get_at(next_idx)?;
        let next_id = next_tab.id;

        if let Some(tab_mut) = tabs.get_mut(next_id) {
            Self::resume_session_playback(tab_mut, main_xid_opt, viewport_rect);
        }

        Some(next_id)
    }

    fn update_session_media(
        tab: &mut MediaTabSession,
        record: MediaRecord,
        file_path: std::path::PathBuf,
        title: String,
        main_xid_opt: Option<u64>,
        (x, y, w, h): (i32, i32, u32, u32),
    ) -> Result<(), ViewerError> {
        let is_video = matches!(record.media_type, MediaType::Video);
        let filename = record.filename.clone();
        let format = record.format;

        tab.media_id = record.id;
        tab.filename = filename;
        tab.title = title;
        tab.format = format;
        tab.file_path = file_path.clone();
        tab.record = record.clone();
        tab.last_accessed = Instant::now();

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

        Ok(())
    }
}
