//! Opening tabs: a medium, a background tab, an adopted Multiview, paging a tab.

use super::ViewerTabManager;
use crate::viewer::error::ViewerError;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
use crate::viewer::multiview::model::{TileKind, aspect_from_size};
use crate::viewer::multiview::route::OriginKind;
use crate::viewer::multiview::tile::TileSeed;
use crate::viewer::tab_item::{TabCollection, TabTitleFormatter};
use crate::viewer::types::{MULTIVIEW_TAB_MEDIA_ID, MediaTabSession, TabId};
use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::media::MediaType;
use rustview_core::settings::LogObscurer;
use rustview_core::settings::schema::{MULTIVIEW_FALLBACK_ASPECT, MULTIVIEW_TAB_TITLE};
use std::sync::atomic::Ordering;
use std::time::Instant;

impl ViewerTabManager {
    /// The Multiview of one tile that a new tab of `record` shows: isolated from the Browse view (origin `Tab`), started by the first
    /// host that shows it.
    pub(super) fn solo_instance(
        &self,
        record: &MediaRecord,
        file_path: &std::path::Path,
    ) -> Result<SharedInstance, ViewerError> {
        let sink = self
            .event_sink
            .lock()
            .clone()
            .ok_or_else(|| ViewerError::DecodeFailed("the Multiview is not ready yet".to_string()))?;
        let instance = MultiviewInstance::create(sink, vec![Self::solo_seed(record, file_path)]);
        instance.lock().set_origin(OriginKind::Tab);
        Ok(instance)
    }

    pub(super) fn solo_seed(record: &MediaRecord, file_path: &std::path::Path) -> TileSeed {
        TileSeed::new(
            record.id,
            if matches!(record.media_type, MediaType::Video) {
                TileKind::Video
            } else {
                TileKind::Image
            },
            aspect_from_size(record.width, record.height, MULTIVIEW_FALLBACK_ASPECT),
            file_path.to_path_buf(),
            record.filename.clone(),
        )
    }

    /// A new tab that shows `record` alone. Its title is the file name (numbered when a tab of the same medium has it already)
    /// and never changes afterwards.
    pub(super) fn create_solo_session(
        &self,
        tabs: &TabCollection,
        id: TabId,
        record: MediaRecord,
        file_path: std::path::PathBuf,
    ) -> Result<MediaTabSession, ViewerError> {
        let instance = self.solo_instance(&record, &file_path)?;
        let title = TabTitleFormatter::allocate_tab_title(tabs.all(), record.id, &record.filename);
        Ok(MediaTabSession::for_solo(id, title, record, file_path, instance))
    }

    /// Opens a media item in a viewer tab and activates it in the foreground. Reuses existing tab if `allow_multiple` is false.
    pub fn open_or_focus_media(&self, media_id: MediaId, allow_multiple: bool) -> Result<(TabId, bool), ViewerError> {
        let mut tabs = self.tabs.write();

        if !allow_multiple && let Some(existing) = tabs.get_by_media_id_mut(media_id) {
            let id = existing.id;
            existing.last_accessed = Instant::now();
            drop(tabs);

            self.set_active_tab_internal(id, true);
            tracing::info!(
                tab_id = id,
                media_id,
                "[ViewerTabManager: Focus] Focused existing tab session"
            );
            return Ok((id, false));
        }

        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;

        let file_path = self.library.resolve_media_path(media_id)?;
        let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);

        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.get_mut(current_id)
        {
            Self::pause_session_playback(current_tab);
        }

        let session = self.create_solo_session(&tabs, new_id, record, file_path.clone())?;

        tabs.push(session);
        *self.active_tab_id.write() = Some(new_id);

        tracing::info!(
            tab_id = new_id,
            media_id,
            total_open_tabs = tabs.len(),
            file_path = %LogObscurer::path(&file_path),
            "[ViewerTabManager: Open] Created new viewer tab session"
        );

        drop(tabs);
        self.tab_created();
        Ok((new_id, true))
    }

    /// Opens a media item in a background viewer tab without activating it or interrupting current foreground playback.
    pub fn open_media_in_background(
        &self,
        media_id: MediaId,
        allow_multiple: bool,
    ) -> Result<(TabId, bool), ViewerError> {
        let mut tabs = self.tabs.write();

        if !allow_multiple && let Some(existing) = tabs.get_by_media_id_mut(media_id) {
            let id = existing.id;
            tracing::info!(
                tab_id = id,
                media_id,
                "[ViewerTabManager: OpenBackground] Tab already exists for media; leaving in background"
            );
            return Ok((id, false));
        }

        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;

        let file_path = self.library.resolve_media_path(media_id)?;
        let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);

        let session = self.create_solo_session(&tabs, new_id, record, file_path.clone())?;

        tabs.push(session);

        tracing::info!(
            tab_id = new_id,
            media_id,
            total_open_tabs = tabs.len(),
            file_path = %LogObscurer::path(&file_path),
            "[ViewerTabManager: OpenBackground] Created new background viewer tab session"
        );

        drop(tabs);
        self.tab_created();
        Ok((new_id, true))
    }

    /// Adopts a running Multiview into a new background tab without restarting or moving anything. Nothing is activated;
    /// the caller shows the tab when it wants to. Adopting the same instance twice returns its tab.
    pub fn adopt_multiview_in_background(&self, instance: SharedInstance) -> TabId {
        let instance_id = instance.lock().id();
        self.stop_observing_instance(instance_id);
        let mut tabs = self.tabs.write();
        if let Some(existing) = tabs.get_by_instance_id(instance_id) {
            return existing.id;
        }

        let new_id = self.tab_counter.fetch_add(1, Ordering::Relaxed);
        // A Multiview of one medium is named after its file, one of several media "Multiview".
        let single = instance.lock().solo_tile().map(|(id, _, _)| id);
        let single_filename = instance.lock().solo_filename();
        let base_title = single_filename.as_deref().unwrap_or(MULTIVIEW_TAB_TITLE);
        let title =
            TabTitleFormatter::allocate_tab_title(tabs.all(), single.unwrap_or(MULTIVIEW_TAB_MEDIA_ID), base_title);
        let mut session = MediaTabSession::for_multiview(new_id, title, instance);
        if let Some(filename) = single_filename {
            session.filename = filename;
        }
        tabs.push(session);

        tracing::info!(
            tab_id = new_id,
            instance = instance_id,
            total_open_tabs = tabs.len(),
            "[ViewerTabManager: AdoptMultiview] Adopted the running Multiview into a background tab"
        );
        drop(tabs);
        self.sync_tab_identities();
        self.tab_created();
        new_id
    }

    /// Makes every tab's description (medium, path, file name, record) agree with what its Multiview shows now: the one medium of
    /// a tab with exactly one tile, placeholders for a tab of several media. Tiles come and go inside a tab (duplicate, remove,
    /// previous/next), so this runs whenever the tab list is rendered. The title never changes.
    pub fn sync_tab_identities(&self) {
        let snapshot: Vec<(TabId, MediaId, Option<MediaId>)> = self
            .tabs
            .read()
            .all()
            .iter()
            .map(|t| {
                let solo = t.multiview.instance.lock().solo_tile().map(|(id, _, _)| id);
                (t.id, t.media_id, solo)
            })
            .collect();
        for (tab_id, current, solo) in snapshot {
            match solo {
                Some(media_id) if media_id == current => {}
                Some(media_id) => {
                    let record = self.library.media().get(media_id).ok().flatten();
                    let path = self.library.resolve_media_path(media_id).ok();
                    if let (Some(record), Some(path)) = (record, path)
                        && let Some(tab) = self.tabs.write().get_mut(tab_id)
                    {
                        tab.set_solo_identity(record, path);
                    }
                }
                None if current != MULTIVIEW_TAB_MEDIA_ID => {
                    if let Some(tab) = self.tabs.write().get_mut(tab_id) {
                        tab.clear_solo_identity();
                    }
                }
                None => {}
            }
        }
    }
}
