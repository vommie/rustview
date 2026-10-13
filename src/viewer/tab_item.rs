use super::error::ViewerError;
use super::mpv_engine::MpvPlayer;
use super::types::{ImageTabPayload, MediaTabSession, TabId, TabPayload, VideoTabPayload};
use super::x11_embed::X11VideoSubwindow;
use crate::library::models::{MediaId, MediaRecord};
use crate::media::MediaType;
use crate::settings::LogObscurer;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Centralized, single point of truth (SPoT) formatter for tab titles and duplicate iteration suffixes.
pub struct TabTitleFormatter;

impl TabTitleFormatter {
    /// Allocates a unique, permanent title for a new tab session referencing `media_id`.
    /// Reuses the base filename if not already taken; otherwise finds the lowest available `(1)`, `(2)`, etc.
    pub fn allocate_tab_title(
        existing_tabs: &[MediaTabSession],
        media_id: MediaId,
        base_filename: &str,
    ) -> String {
        let taken_titles: HashSet<&str> = existing_tabs
            .iter()
            .filter(|t| t.media_id == media_id)
            .map(|t| t.title.as_str())
            .collect();

        if !taken_titles.contains(base_filename) {
            return base_filename.to_string();
        }

        let mut counter = 1usize;
        loop {
            let candidate = format!("{} ({})", base_filename, counter);
            if !taken_titles.contains(candidate.as_str()) {
                return candidate;
            }
            counter += 1;
        }
    }

    /// Retrieves the permanent title of a tab by its `TabId`.
    #[inline]
    pub fn compute_title(tabs: &[MediaTabSession], tab_id: TabId) -> String {
        tabs.iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.title.clone())
            .unwrap_or_default()
    }
}

/// Encapsulated, ordered collection managing `MediaTabSession` entities.
/// Provides pure in-memory query, mutation, movement, and reordering operations (SRP).
#[derive(Debug, Default, Clone)]
pub struct TabCollection {
    sessions: Vec<MediaTabSession>,
}

impl TabCollection {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    // =========================================================================
    // Queries
    // =========================================================================

    #[inline]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    #[inline]
    pub fn all(&self) -> &[MediaTabSession] {
        &self.sessions
    }

    #[inline]
    pub fn to_vec(&self) -> Vec<MediaTabSession> {
        self.sessions.clone()
    }

    pub fn get(&self, tab_id: TabId) -> Option<&MediaTabSession> {
        self.sessions.iter().find(|t| t.id == tab_id)
    }

    pub fn get_mut(&mut self, tab_id: TabId) -> Option<&mut MediaTabSession> {
        self.sessions.iter_mut().find(|t| t.id == tab_id)
    }

    pub fn get_at(&self, index: usize) -> Option<&MediaTabSession> {
        self.sessions.get(index)
    }

    pub fn index_of(&self, tab_id: TabId) -> Option<usize> {
        self.sessions.iter().position(|t| t.id == tab_id)
    }

    pub fn get_by_media_id(&self, media_id: MediaId) -> Option<&MediaTabSession> {
        self.sessions.iter().find(|t| t.media_id == media_id)
    }

    pub fn get_by_media_id_mut(&mut self, media_id: MediaId) -> Option<&mut MediaTabSession> {
        self.sessions.iter_mut().find(|t| t.media_id == media_id)
    }

    pub fn get_all_by_media_id(&self, media_id: MediaId) -> Vec<MediaTabSession> {
        self.sessions
            .iter()
            .filter(|t| t.media_id == media_id)
            .cloned()
            .collect()
    }

    // =========================================================================
    // Mutations & Lifecycle
    // =========================================================================

    pub fn push(&mut self, session: MediaTabSession) {
        self.sessions.push(session);
    }

    /// Inserts a new session immediately after `after_tab_id` (used for tab duplication).
    pub fn insert_after(&mut self, after_tab_id: TabId, session: MediaTabSession) -> usize {
        let insert_idx = self
            .index_of(after_tab_id)
            .map(|idx| idx + 1)
            .unwrap_or(self.sessions.len());

        let clamped_idx = insert_idx.min(self.sessions.len());
        self.sessions.insert(clamped_idx, session);
        clamped_idx
    }

    /// Removes a tab session by its `TabId` and returns the removed session and its original 0-based index.
    pub fn remove(&mut self, tab_id: TabId) -> Option<(usize, MediaTabSession)> {
        let pos = self.index_of(tab_id)?;
        let removed = self.sessions.remove(pos);
        Some((pos, removed))
    }

    /// Clears all sessions and returns them for resource cleanup.
    pub fn clear(&mut self) -> Vec<MediaTabSession> {
        std::mem::take(&mut self.sessions)
    }

    /// Retains only the tab matching `keep_tab_id` and returns the evicted sessions for cleanup.
    pub fn retain_only(&mut self, keep_tab_id: TabId) -> Vec<MediaTabSession> {
        let mut evicted = Vec::new();
        let mut kept = Vec::new();

        for session in self.sessions.drain(..) {
            if session.id == keep_tab_id {
                kept.push(session);
            } else {
                evicted.push(session);
            }
        }

        self.sessions = kept;
        evicted
    }

    /// Removes all tabs positioned to the left of `tab_id` and returns the evicted sessions.
    pub fn remove_to_left(&mut self, tab_id: TabId) -> Vec<MediaTabSession> {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        if pos == 0 {
            return Vec::new();
        }

        self.sessions.drain(0..pos).collect()
    }

    /// Removes all tabs positioned to the right of `tab_id` and returns the evicted sessions.
    pub fn remove_to_right(&mut self, tab_id: TabId) -> Vec<MediaTabSession> {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        if pos + 1 >= self.sessions.len() {
            return Vec::new();
        }

        self.sessions.drain((pos + 1)..).collect()
    }

    // =========================================================================
    // Reordering & Movement Algorithms (Retains Permanent Tab Titles)
    // =========================================================================

    /// Relocates a tab to a target index position in the list without mutating its permanent title.
    pub fn move_tab(&mut self, tab_id: TabId, target_index: usize) -> bool {
        let current_pos = match self.index_of(tab_id) {
            Some(pos) => pos,
            None => return false,
        };

        let clamped_target = target_index.min(self.sessions.len().saturating_sub(1));
        if current_pos == clamped_target {
            return false;
        }

        let item = self.sessions.remove(current_pos);
        self.sessions.insert(clamped_target, item);
        true
    }

    pub fn move_to_start(&mut self, tab_id: TabId) -> bool {
        self.move_tab(tab_id, 0)
    }

    pub fn move_left(&mut self, tab_id: TabId) -> bool {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos > 0 {
            self.move_tab(tab_id, pos - 1)
        } else {
            false
        }
    }

    pub fn move_right(&mut self, tab_id: TabId) -> bool {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos + 1 < self.sessions.len() {
            self.move_tab(tab_id, pos + 1)
        } else {
            false
        }
    }

    pub fn move_to_end(&mut self, tab_id: TabId) -> bool {
        let count = self.sessions.len();
        if count > 0 {
            self.move_tab(tab_id, count - 1)
        } else {
            false
        }
    }

    // =========================================================================
    // Synchronous Reactive State Updates
    // =========================================================================

    pub fn update_filename(&mut self, media_id: MediaId, new_filename: &str) {
        for tab in self.sessions.iter_mut() {
            if tab.media_id == media_id {
                let old_base = &tab.filename;
                let new_title = if let Some(suffix) = tab.title.strip_prefix(old_base) {
                    format!("{}{}", new_filename, suffix)
                } else {
                    new_filename.to_string()
                };

                tab.filename = new_filename.to_string();
                tab.title = new_title;
                if let Some(parent) = tab.file_path.parent() {
                    tab.file_path = parent.join(new_filename);
                }
                tab.record.filename = new_filename.to_string();
            }
        }
        for mv in self.sessions.iter().filter_map(|t| t.payload.multiview()) {
            mv.instance.lock().member_renamed(media_id, new_filename);
        }
    }

    pub fn mark_deleted(&mut self, media_id: MediaId) {
        for tab in self.sessions.iter_mut() {
            if tab.media_id == media_id {
                tab.record.is_deleted = true;
                let missing_msg = format!(
                    "Media file not found on disk:\n{}",
                    tab.file_path.display()
                );
                match tab.payload {
                    TabPayload::Image(ref mut img) => {
                        img.is_loading = false;
                        img.high_res_pixels = None;
                        img.error_message = missing_msg;
                    }
                    TabPayload::Video(ref mut vid) => {
                        vid.is_loading = false;
                        vid.error_message = missing_msg;
                        vid.player.stop();
                        let mut sub = vid.subwindow.lock();
                        if let Some(ref mut s) = *sub {
                            s.hide();
                        }
                    }
                    TabPayload::Multiview(_) => {}
                }
            }
        }
        for mv in self.sessions.iter().filter_map(|t| t.payload.multiview()) {
            mv.instance.lock().member_deleted(media_id);
        }
    }

    pub fn update_record(&mut self, media_id: MediaId, updated_record: &MediaRecord) {
        for tab in self.sessions.iter_mut() {
            if tab.media_id == media_id {
                tab.record = updated_record.clone();
                let file_exists = tab.file_path.is_file();
                match tab.payload {
                    TabPayload::Image(ref mut img) => {
                        img.orig_width = updated_record.width;
                        img.orig_height = updated_record.height;
                        if file_exists && !updated_record.is_deleted {
                            img.error_message.clear();
                        } else if !file_exists {
                            img.error_message = format!(
                                "Media file not found on disk:\n{}",
                                tab.file_path.display()
                            );
                        }
                    }
                    TabPayload::Video(ref mut vid) => {
                        vid.duration_ms = updated_record.duration_ms;
                        if file_exists && !updated_record.is_deleted {
                            vid.error_message.clear();
                        } else if !file_exists {
                            vid.error_message = format!(
                                "Media file not found on disk:\n{}",
                                tab.file_path.display()
                            );
                        }
                    }
                    TabPayload::Multiview(_) => {}
                }
            }
        }
        for mv in self.sessions.iter().filter_map(|t| t.payload.multiview()) {
            mv.instance.lock().member_refreshed(media_id, updated_record);
        }
    }

    pub fn get_by_instance_id(&self, instance_id: u64) -> Option<&MediaTabSession> {
        self.sessions
            .iter()
            .find(|t| t.multiview().is_some_and(|mv| mv.instance_id == instance_id))
    }
}

/// Factory responsible for instantiating concrete `MediaTabSession` entities and underlying media payloads (OCP).
pub struct TabSessionFactory;

impl TabSessionFactory {
    /// Constructs a fully initialized `MediaTabSession` with its permanent display title.
    /// Performs a non-blocking physical existence check before initializing video rendering or player instances.
    pub fn create_session(
        id: TabId,
        record: MediaRecord,
        file_path: PathBuf,
        title: String,
        main_xid_opt: Option<u64>,
        viewport_rect: (i32, i32, u32, u32),
    ) -> Result<MediaTabSession, ViewerError> {
        let filename = record.filename.clone();
        let format = record.format;
        let is_video = matches!(record.media_type, MediaType::Video);
        let now = Instant::now();
        let file_exists = file_path.is_file();

        let payload = if is_video {
            if !file_exists {
                tracing::warn!(
                    tab_id = id,
                    media_id = record.id,
                    file_path = %LogObscurer::path(&file_path),
                    "[TabSessionFactory: Video] Target file does not exist on disk, initializing tab in missing-file fallback state"
                );
                TabPayload::Video(VideoTabPayload {
                    player: Arc::new(MpvPlayer::new()?),
                    subwindow: Arc::new(Mutex::new(None)),
                    position_ms: 0,
                    is_playing: false,
                    volume: 1.0,
                    duration_ms: record.duration_ms,
                    is_loading: false,
                    error_message: format!(
                        "Media file not found on disk:\n{}",
                        file_path.display()
                    ),
                })
            } else {
                Self::create_video_payload(&file_path, record.duration_ms, main_xid_opt, viewport_rect)?
            }
        } else if !file_exists {
            tracing::warn!(
                tab_id = id,
                media_id = record.id,
                file_path = %LogObscurer::path(&file_path),
                "[TabSessionFactory: Image] Target file does not exist on disk, initializing tab in missing-file fallback state"
            );
            TabPayload::Image(ImageTabPayload {
                high_res_pixels: None,
                is_loading: false,
                error_message: format!(
                    "Media file not found on disk:\n{}",
                    file_path.display()
                ),
                orig_width: record.width,
                orig_height: record.height,
            })
        } else {
            Self::create_image_payload(record.width, record.height)
        };

        Ok(MediaTabSession {
            id,
            media_id: record.id,
            filename,
            title,
            format,
            file_path,
            record,
            payload,
            created_at: now,
            last_accessed: now,
        })
    }

    fn create_video_payload(
        file_path: &Path,
        duration_ms: u64,
        main_xid_opt: Option<u64>,
        (x, y, w, h): (i32, i32, u32, u32),
    ) -> Result<TabPayload, ViewerError> {
        let player = Arc::new(MpvPlayer::new()?);
        let subwindow_slot = Arc::new(Mutex::new(None));

        if let Some(main_xid) = main_xid_opt
            && main_xid > 0
            && let Ok(mut sub) = X11VideoSubwindow::new(main_xid, x, y, w, h)
        {
            let wid = sub.xid();
            let _ = player.attach_to_window(wid);
            sub.hide(); // Keep initially hidden to eliminate visual flicker on background creation
            *subwindow_slot.lock() = Some(sub);
        }

        let _ = player.load_file(file_path);

        Ok(TabPayload::Video(VideoTabPayload {
            player,
            subwindow: subwindow_slot,
            position_ms: 0,
            is_playing: true,
            volume: 1.0,
            duration_ms,
            is_loading: false,
            error_message: String::new(),
        }))
    }

    fn create_image_payload(width: u32, height: u32) -> TabPayload {
        TabPayload::Image(ImageTabPayload {
            high_res_pixels: None,
            is_loading: true,
            error_message: String::new(),
            orig_width: width,
            orig_height: height,
        })
    }
}

/// Eviction policy coordinator managing uncompressed high-resolution full-image buffers in RAM.
pub struct ImageBufferEvictor;

impl ImageBufferEvictor {
    /// Evicts full-resolution image buffers of oldest inactive tabs when exceeding the memory capacity limit.
    pub fn evict_excess(tabs: &mut [MediaTabSession], active_id: Option<TabId>, max_buffers: usize) {
        let cached_count = tabs
            .iter()
            .filter(|t| matches!(&t.payload, TabPayload::Image(img) if img.high_res_pixels.is_some()))
            .count();

        if cached_count <= max_buffers {
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

        let to_evict_count = cached_count.saturating_sub(max_buffers);
        for &(idx, _) in inactive_indices.iter().take(to_evict_count) {
            if let TabPayload::Image(ref mut img) = tabs[idx].payload {
                img.high_res_pixels = None;
                img.is_loading = false;
                tracing::debug!(
                    tab_id = tabs[idx].id,
                    media_id = tabs[idx].media_id,
                    "[ImageBufferEvictor: Evict] Evicted inactive high-resolution pixel buffer from RAM"
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use crate::viewer::types::{MULTIVIEW_TAB_MEDIA_ID, TabContentType};

    fn instance(ids: &[MediaId]) -> SharedInstance {
        let seeds = ids
            .iter()
            .map(|&media_id| TileSeed {
                media_id,
                kind: TileKind::Image,
                aspect: 1.78,
                path: PathBuf::from(format!("/media/{media_id}.png")),
                filename: format!("{media_id}.png"),
            })
            .collect();
        let shared = MultiviewInstance::detached(seeds);
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn collection_with_multiview(ids: &[MediaId]) -> (TabCollection, SharedInstance) {
        let shared = instance(ids);
        let mut tabs = TabCollection::new();
        tabs.push(MediaTabSession::for_multiview(
            1,
            "Multiview".to_string(),
            shared.clone(),
        ));
        (tabs, shared)
    }

    #[test]
    fn a_multiview_tab_describes_itself_through_the_helpers() {
        let (tabs, _shared) = collection_with_multiview(&[5, 6]);
        let tab = tabs.get(1).expect("the tab exists");
        assert_eq!(tab.kind(), TabContentType::Multiview);
        assert!(tab.is_multiview() && !tab.is_video() && !tab.is_image());
        assert_eq!(tab.media_id, MULTIVIEW_TAB_MEDIA_ID);
        assert_eq!(tab.single_media_id(), None);
        assert_eq!(tab.member_ids(), vec![5, 6]);
        assert!(tab.has_member(6) && !tab.has_member(7));
        assert_eq!(tab.display_size(), (1920, 1080));
    }

    #[test]
    fn media_lookups_never_find_a_multiview_tab() {
        let (tabs, shared) = collection_with_multiview(&[5, 6]);
        assert!(tabs.get_by_media_id(5).is_none());
        assert!(tabs.get_all_by_media_id(6).is_empty());
        let instance_id = shared.lock().id();
        assert!(tabs.get_by_instance_id(instance_id).is_some());
        assert!(tabs.get_by_instance_id(instance_id + 1).is_none());
    }

    #[test]
    fn several_multiview_tabs_get_numbered_titles() {
        let (mut tabs, _shared) = collection_with_multiview(&[5, 6]);
        let second =
            TabTitleFormatter::allocate_tab_title(tabs.all(), MULTIVIEW_TAB_MEDIA_ID, "Multiview");
        assert_eq!(second, "Multiview (1)");
        tabs.push(MediaTabSession::for_multiview(2, second, instance(&[7, 8])));
        let third =
            TabTitleFormatter::allocate_tab_title(tabs.all(), MULTIVIEW_TAB_MEDIA_ID, "Multiview");
        assert_eq!(third, "Multiview (2)");
    }

    #[test]
    fn renames_reach_the_members_but_not_the_title() {
        let (mut tabs, shared) = collection_with_multiview(&[5, 6]);
        tabs.update_filename(6, "renamed.png");
        let guard = shared.lock();
        assert_eq!(guard.view().tiles.len(), 2);
        drop(guard);
        let tab = tabs.get(1).expect("the tab exists");
        assert_eq!(tab.title, "Multiview");
    }

    #[test]
    fn a_deleted_member_turns_its_tile_into_an_error() {
        let (mut tabs, shared) = collection_with_multiview(&[5, 6]);
        tabs.mark_deleted(6);
        let view = shared.lock().view();
        assert!(view.tiles[0].error.is_none());
        assert!(view.tiles[1].error.is_some());
        assert_eq!(
            shared.lock().member_ids(),
            vec![5, 6],
            "the tile keeps its slot"
        );

        tabs.mark_deleted(99);
        assert!(shared.lock().view().tiles[0].error.is_none());
    }
}
