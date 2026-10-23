use super::error::ViewerError;
use super::multiview::events::MultiviewEventSink;
use super::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use super::multiview::model::{TileKind, aspect_from_size};
use super::multiview::route::OriginKind;
use super::multiview::teardown;
use super::multiview::tile::TileSeed;
use super::tab_item::{TabCollection, TabTitleFormatter};
use super::tab_ui_gate::TabUiGate;
use super::types::{MULTIVIEW_TAB_MEDIA_ID, MediaTabSession, TabId};
use super::x11::{X11VideoSubwindow, XRectangle};
use parking_lot::{Mutex, RwLock};
use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::media::MediaType;
use rustview_core::settings::LogObscurer;
use rustview_core::settings::schema::{MULTIVIEW_FALLBACK_ASPECT, MULTIVIEW_TAB_TITLE, SHUTDOWN_TEARDOWN_WAIT_MS};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// High-level workspace coordinator managing open tabs, player lifecycles, and window embedding (SRP/OCP).
pub struct ViewerTabManager {
    tabs: RwLock<TabCollection>,
    active_tab_id: RwLock<Option<TabId>>,
    tab_counter: AtomicU64,
    library: Arc<Library>,
    cached_viewport_rect: RwLock<(i32, i32, u32, u32)>,
    active_menu_cutouts: RwLock<Vec<XRectangle>>,
    active_tooltip_cutouts: RwLock<Vec<XRectangle>>,
    /// Multiviews that no tab owns (the fullscreen one that follows the Browse selection) but whose media may be renamed,
    /// deleted or refreshed while it is open.
    observed_instances: Mutex<Vec<WeakInstance>>,
    /// Whether the UI shows the tabs that exist, and who to tell when a tab is created (see `TabUiGate`).
    tab_ui_gate: TabUiGate,
    tabs_changed_hook: Mutex<Option<TabsChangedHook>>,
    /// Where the Multiviews of the tabs this manager creates report their events (decoded images, player events); set by the
    /// application when the Multiview UI exists.
    event_sink: Mutex<Option<Arc<dyn MultiviewEventSink>>>,
}

/// Called (from any thread, with no manager lock held) after a tab was created.
type TabsChangedHook = Arc<dyn Fn() + Send + Sync>;

impl ViewerTabManager {
    pub fn new(library: Arc<Library>) -> Self {
        Self {
            tabs: RwLock::new(TabCollection::new()),
            active_tab_id: RwLock::new(None),
            tab_counter: AtomicU64::new(1),
            library,
            cached_viewport_rect: RwLock::new((0, 64, 800, 600)),
            active_menu_cutouts: RwLock::new(Vec::new()),
            active_tooltip_cutouts: RwLock::new(Vec::new()),
            observed_instances: Mutex::new(Vec::new()),
            tab_ui_gate: TabUiGate::default(),
            tabs_changed_hook: Mutex::new(None),
            event_sink: Mutex::new(None),
        }
    }

    // =========================================================================
    // Tab UI Invariant: a created tab is always rendered
    // =========================================================================

    /// Sets what happens after a tab was created: the application renders the tab list if it has not done so already.
    pub fn set_tabs_changed_hook(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self.tabs_changed_hook.lock() = Some(Arc::new(hook));
    }

    /// Sets where the Multiviews of the tabs created from now on report their events. Tabs cannot be created before this.
    pub fn set_event_sink(&self, sink: Arc<dyn MultiviewEventSink>) {
        *self.event_sink.lock() = Some(sink);
    }

    /// The revision of the tab list; the UI passes it to `mark_tab_ui_synced` once it has rendered the tabs.
    pub fn tab_ui_revision(&self) -> u64 {
        self.tab_ui_gate.revision()
    }

    pub fn mark_tab_ui_synced(&self, revision: u64) {
        self.tab_ui_gate.mark_synced(revision);
    }

    /// True while a tab exists that the UI has not rendered yet.
    pub fn is_tab_ui_stale(&self) -> bool {
        self.tab_ui_gate.is_stale()
    }

    /// Every creation path ends here, after its tab collection lock is released.
    fn tab_created(&self) {
        let revision = self.tab_ui_gate.changed();
        tracing::debug!(
            revision,
            "[ViewerTabManager: TabCreated] The tab UI has to show the new tab"
        );
        let hook = self.tabs_changed_hook.lock().clone();
        if let Some(hook) = hook {
            hook();
        }
    }

    // =========================================================================
    // Viewport & Window Handle Configuration
    // =========================================================================

    /// Stores the latest geometry of the viewer area (where a tab that moves back from the fullscreen parks its windows).
    pub fn set_main_viewport_info(&self, x: i32, y: i32, w: u32, h: u32) {
        *self.cached_viewport_rect.write() = (x, y, w, h);
    }

    pub fn get_viewport_rect(&self) -> (i32, i32, u32, u32) {
        *self.cached_viewport_rect.read()
    }

    // =========================================================================
    // Active Player & Subwindow Queries & Centralized Overlay Clipping
    // =========================================================================

    /// Hides native X11 video subwindows across all video tabs and auto-pauses playback without modifying user pause state.
    pub fn hide_all_video_subwindows(&self) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            tab.deactivate_native();
        }
    }

    /// Hides all video subwindows except for the specified `except_tab_id`.
    pub fn hide_video_subwindows_except(&self, except_tab_id: Option<TabId>) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            if Some(tab.id) != except_tab_id {
                tab.deactivate_native();
            }
        }
    }

    /// Sets view-active and interactivity state across all video tabs (used when entering/leaving `TabGrid` mode).
    pub fn set_all_videos_view_active(&self, active: bool) {
        let tabs = self.tabs.read();
        for tab in tabs.all() {
            tab.set_grid_view_active(active);
        }
    }

    // =========================================================================
    // Dynamic XShape Overlay Clipping (Menu & Tooltip Layer Hierarchy)
    // =========================================================================

    /// Updates active context menu cutouts with deduplication to eliminate redundant `XShape` calls.
    pub fn set_menu_cutouts(&self, cutouts: Vec<XRectangle>) {
        {
            let current = self.active_menu_cutouts.read();
            if *current == cutouts {
                return;
            }
        }
        *self.active_menu_cutouts.write() = cutouts;
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Clears active context menu cutouts with deduplication.
    pub fn clear_menu_cutouts(&self) {
        {
            let mut writer = self.active_menu_cutouts.write();
            if writer.is_empty() {
                return;
            }
            writer.clear();
        }
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Updates active tooltip cutouts (Z: 200) with deduplication.
    pub fn set_tooltip_cutouts(&self, cutouts: Vec<XRectangle>) {
        {
            let current = self.active_tooltip_cutouts.read();
            if *current == cutouts {
                return;
            }
        }
        *self.active_tooltip_cutouts.write() = cutouts;
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Clears active tooltip cutouts with deduplication.
    pub fn clear_tooltip_cutouts(&self) {
        {
            let mut writer = self.active_tooltip_cutouts.write();
            if writer.is_empty() {
                return;
            }
            writer.clear();
        }
        self.apply_combined_cutouts_to_all_subwindows();
    }

    /// Returns all combined cutouts (both menu and tooltip).
    pub fn get_combined_cutouts(&self) -> Vec<XRectangle> {
        let mut combined = self.active_menu_cutouts.read().clone();
        let tooltips = self.active_tooltip_cutouts.read();
        combined.extend_from_slice(&tooltips);
        combined
    }

    /// Re-applies all active cutouts to a specific subwindow (e.g. during scroll or resize).
    pub fn reapply_active_cutouts_to_subwindow(&self, sub: &mut X11VideoSubwindow) {
        let combined = self.get_combined_cutouts();
        if combined.is_empty() {
            sub.reset_clip_shape();
        } else {
            sub.clip_cutout_rectangles(&combined);
        }
    }

    /// Re-applies all combined cutouts (menus + tooltips) across EVERY visible video subwindow on screen.
    pub fn apply_combined_cutouts_to_all_subwindows(&self) {
        let combined = self.get_combined_cutouts();
        let tabs = self.tabs.read();

        for tab in tabs.all() {
            tab.apply_overlay_cutouts(&combined);
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

    /// The one medium of the active tab; `None` for the Browse view and for a Multiview tab.
    pub fn get_active_media_id(&self) -> Option<MediaId> {
        self.get_active_tab().and_then(|t| t.single_media_id())
    }

    /// Every medium the active tab shows (the tiles of a Multiview tab); empty for the Browse view.
    pub fn get_active_media_ids(&self) -> Vec<MediaId> {
        self.get_active_tab().map(|t| t.member_ids()).unwrap_or_default()
    }

    /// The tab that owns the Multiview instance `instance_id`.
    pub fn tab_id_of_instance(&self, instance_id: u64) -> Option<TabId> {
        self.tabs.read().get_by_instance_id(instance_id).map(|t| t.id)
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

    #[inline]
    pub fn get_by_media_id(&self, media_id: MediaId) -> Option<MediaTabSession> {
        self.get_tab_by_media_id(media_id)
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

    #[inline]
    pub fn move_tab_to_start(&self, tab_id: TabId) -> bool {
        self.move_tab(tab_id, 0)
    }

    #[inline]
    pub fn move_to_start(&self, tab_id: TabId) -> bool {
        self.move_tab_to_start(tab_id)
    }

    pub fn move_tab_left(&self, tab_id: TabId) -> bool {
        let pos = match self.get_tab_index(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos > 0 { self.move_tab(tab_id, pos - 1) } else { false }
    }

    #[inline]
    pub fn move_left(&self, tab_id: TabId) -> bool {
        self.move_tab_left(tab_id)
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

    #[inline]
    pub fn move_right(&self, tab_id: TabId) -> bool {
        self.move_tab_right(tab_id)
    }

    pub fn move_tab_to_end(&self, tab_id: TabId) -> bool {
        let count = self.sessions_count();
        if count > 0 {
            self.move_tab(tab_id, count - 1)
        } else {
            false
        }
    }

    #[inline]
    pub fn move_to_end(&self, tab_id: TabId) -> bool {
        self.move_tab_to_end(tab_id)
    }

    #[inline]
    fn sessions_count(&self) -> usize {
        self.tabs.read().len()
    }

    // =========================================================================
    // Tab Duplication, Opening, Background Opening & In-Place Navigation
    // =========================================================================

    /// Duplicates an existing tab of one medium, placing the clone immediately adjacent to the source tab. A tab that shows
    /// several media cannot be duplicated.
    pub fn duplicate_tab(&self, source_tab_id: TabId) -> Result<TabId, ViewerError> {
        let source_session = self
            .get_tab(source_tab_id)
            .ok_or(ViewerError::MediaNotFound(source_tab_id))?;
        let media_id = source_session.single_media_id().ok_or(ViewerError::MultiviewTab)?;

        let (new_tab_id, _) = self.open_or_focus_media(media_id, true)?;

        if let Some(source_index) = self.get_tab_index(source_tab_id) {
            self.move_tab(new_tab_id, source_index + 1);
        }
        self.set_active_tab_internal(new_tab_id, true);

        tracing::info!(
            source_tab_id,
            new_tab_id,
            media_id,
            "[ViewerTabManager: Duplicate] Duplicated tab session placed adjacent to source"
        );

        Ok(new_tab_id)
    }

    /// Cycles the active tab forward (`is_next = true`) or backward (`is_next = false`) with cyclic wrapping.
    pub fn cycle_tab(&self, is_next: bool) -> Option<TabId> {
        let tabs = self.tabs.read();
        let total = tabs.len();
        if total <= 1 {
            return None;
        }

        let current_active = *self.active_tab_id.read();
        let current_idx = current_active.and_then(|id| tabs.index_of(id));

        let target_idx = match current_idx {
            Some(idx) => {
                if is_next {
                    (idx + 1) % total
                } else {
                    (idx + total - 1) % total
                }
            }
            None => {
                if is_next {
                    0
                } else {
                    total - 1
                }
            }
        };

        let target_tab = tabs.get_at(target_idx)?;
        let target_id = target_tab.id;
        drop(tabs);

        if self.set_active_tab_internal(target_id, false) {
            Some(target_id)
        } else {
            None
        }
    }

    /// The Multiview of one tile that a new tab of `record` shows: isolated from the Browse view (origin `Tab`), started by the first
    /// host that shows it.
    fn solo_instance(&self, record: &MediaRecord, file_path: &std::path::Path) -> Result<SharedInstance, ViewerError> {
        let sink = self
            .event_sink
            .lock()
            .clone()
            .ok_or_else(|| ViewerError::DecodeFailed("the Multiview is not ready yet".to_string()))?;
        let instance = MultiviewInstance::create(sink, vec![Self::solo_seed(record, file_path)]);
        instance.lock().set_origin(OriginKind::Tab);
        Ok(instance)
    }

    fn solo_seed(record: &MediaRecord, file_path: &std::path::Path) -> TileSeed {
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
    fn create_solo_session(
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

    /// Shows `media_id` in the one tile of the active tab (previous/next of the tab toolbar). The tab keeps its title. Fails for a
    /// tab of several media.
    pub fn navigate_active_tab_to_media(&self, media_id: MediaId) -> Result<TabId, ViewerError> {
        let active_id = if let Some(id) = *self.active_tab_id.read() {
            id
        } else {
            let (id, _) = self.open_or_focus_media(media_id, false)?;
            return Ok(id);
        };
        let instance: SharedInstance = self
            .tabs
            .read()
            .get(active_id)
            .map(|t| t.multiview.instance.clone())
            .ok_or(ViewerError::MediaNotFound(active_id))?;

        let record = self
            .library
            .media()
            .get(media_id)?
            .ok_or(ViewerError::MediaNotFound(media_id))?;
        let file_path = self.library.resolve_media_path(media_id)?;

        let replaced = instance.lock().replace_solo(Self::solo_seed(&record, &file_path));
        if !replaced {
            return Err(ViewerError::MultiviewTab);
        }
        if let Some(tab) = self.tabs.write().get_mut(active_id) {
            tab.set_solo_identity(record, file_path);
            tab.last_accessed = Instant::now();
        }

        tracing::info!(
            tab_id = active_id,
            media_id,
            "[ViewerTabManager: NavigateInPlace] Navigated active tab in-place"
        );

        Ok(active_id)
    }

    // =========================================================================
    // Tab Activation, Browsing & Closing
    // =========================================================================

    pub fn set_active_tab(&self, tab_id: TabId) -> bool {
        self.set_active_tab_internal(tab_id, false)
    }

    pub fn set_active_tab_internal(&self, tab_id: TabId, force_resume: bool) -> bool {
        let current_active = *self.active_tab_id.read();

        let mut tabs = self.tabs.write();

        if current_active == Some(tab_id) && !force_resume {
            return true;
        }

        if let Some(prev_id) = current_active
            && prev_id != tab_id
            && let Some(prev_tab) = tabs.get_mut(prev_id)
        {
            Self::pause_session_playback(prev_tab);
        }

        if let Some(new_tab) = tabs.get_mut(tab_id) {
            new_tab.last_accessed = Instant::now();
            *self.active_tab_id.write() = Some(tab_id);
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
        let removed_id = removed.id;
        Self::cleanup_session_resources(&removed);

        tracing::info!(
            tab_id = removed_id,
            remaining = tabs.len(),
            "[ViewerTabManager: Close] Closed tab session"
        );

        if *active == Some(tab_id) {
            *active = Self::resolve_next_active_tab(&mut tabs, pos);
        }

        *active
    }

    pub fn close_tabs_to_left(&self, tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let current_active = *active;
        let evicted = tabs.remove_to_left(tab_id);
        let closed_count = evicted.len();

        if closed_count == 0 {
            return (0, current_active);
        }

        let evicted_ids: HashSet<TabId> = evicted.iter().map(|t| t.id).collect();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        tracing::info!(
            target_tab_id = tab_id,
            closed_count,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: CloseToLeft] Closed all tabs to the left of target"
        );

        if let Some(active_id) = current_active
            && evicted_ids.contains(&active_id)
        {
            *active = Some(tab_id);
            if let Some(target_tab) = tabs.get_mut(tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        (closed_count, *active)
    }

    pub fn close_tabs_to_right(&self, tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let current_active = *active;
        let evicted = tabs.remove_to_right(tab_id);
        let closed_count = evicted.len();

        if closed_count == 0 {
            return (0, current_active);
        }

        let evicted_ids: HashSet<TabId> = evicted.iter().map(|t| t.id).collect();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        tracing::info!(
            target_tab_id = tab_id,
            closed_count,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: CloseToRight] Closed all tabs to the right of target"
        );

        if let Some(active_id) = current_active
            && evicted_ids.contains(&active_id)
        {
            *active = Some(tab_id);
            if let Some(target_tab) = tabs.get_mut(tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        (closed_count, *active)
    }

    pub fn close_other_tabs(&self, keep_tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let evicted = tabs.retain_only(keep_tab_id);
        let closed_count = evicted.len();

        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        if tabs.is_empty() {
            *active = None;
        } else {
            *active = Some(keep_tab_id);
            if let Some(target_tab) = tabs.get_mut(keep_tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        tracing::info!(
            keep_tab_id,
            closed_count,
            "[ViewerTabManager: CloseOthers] Closed all other tabs, keeping single active tab"
        );

        (closed_count, *active)
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

    /// The application exits: every tab is closed and its native resources are destroyed on the calling thread, in the safe
    /// order (a player is shut down completely before the window it rendered into is destroyed). Blocks until everything is
    /// gone, including the teardowns that closed Multiviews left to worker threads.
    pub fn shutdown(&self) {
        let sessions = {
            let mut tabs = self.tabs.write();
            *self.active_tab_id.write() = None;
            tabs.clear()
        };
        self.observed_instances.lock().clear();
        let count = sessions.len();
        for session in sessions {
            Self::shutdown_session(&session);
        }
        let settled = teardown::wait_for_teardowns(Duration::from_millis(SHUTDOWN_TEARDOWN_WAIT_MS));
        tracing::info!(
            tabs = count,
            settled,
            "[ViewerTabManager: Shutdown] All tabs closed and their players shut down"
        );
    }

    fn shutdown_session(session: &MediaTabSession) {
        session.multiview.instance.lock().shutdown_now();
    }

    // =========================================================================
    // Reactive Event Synchronization
    // =========================================================================

    /// Lets library events (rename, delete, refresh) reach a Multiview that no tab owns. Dead references are dropped.
    pub fn observe_instance(&self, instance: &SharedInstance) {
        let mut observed = self.observed_instances.lock();
        observed.retain(|weak| weak.strong_count() > 0);
        let id = instance.lock().id();
        if !observed
            .iter()
            .filter_map(WeakInstance::upgrade)
            .any(|other| other.lock().id() == id)
        {
            observed.push(Arc::downgrade(instance));
        }
    }

    /// The Multiview with `instance_id` is owned by a tab now (or gone), so the tabs deliver its events.
    pub fn stop_observing_instance(&self, instance_id: u64) {
        self.observed_instances
            .lock()
            .retain(|weak| weak.upgrade().is_some_and(|other| other.lock().id() != instance_id));
    }

    fn for_each_observed(&self, mut apply: impl FnMut(&mut MultiviewInstance)) {
        let instances: Vec<SharedInstance> = self
            .observed_instances
            .lock()
            .iter()
            .filter_map(WeakInstance::upgrade)
            .collect();
        for instance in instances {
            apply(&mut instance.lock());
        }
    }

    pub fn notify_media_renamed(&self, media_id: MediaId, new_filename: &str) {
        self.tabs.write().update_filename(media_id, new_filename);
        self.for_each_observed(|mv| mv.member_renamed(media_id, new_filename));
    }

    pub fn notify_media_deleted(&self, media_id: MediaId) {
        self.tabs.write().mark_deleted(media_id);
        self.for_each_observed(|mv| mv.member_deleted(media_id));
    }

    pub fn notify_media_refreshed(&self, media_id: MediaId, updated_record: &MediaRecord) {
        self.tabs.write().update_record(media_id, updated_record);
        self.for_each_observed(|mv| mv.member_refreshed(media_id, updated_record));
    }

    /// The file of `media_id` was refreshed (it may have changed on disk): every tab and observed Multiview that shows it decodes the
    /// picture or loads the video again.
    pub fn reload_media(&self, media_id: MediaId) {
        let instances: Vec<SharedInstance> = self
            .tabs
            .read()
            .all()
            .iter()
            .map(|t| t.multiview.instance.clone())
            .collect();
        for instance in instances {
            instance.lock().reload_member(media_id);
        }
        self.for_each_observed(|mv| mv.reload_member(media_id));
    }

    // =========================================================================
    // Internal Helper Methods
    // =========================================================================

    fn pause_session_playback(tab: &mut MediaTabSession) {
        tab.deactivate_native();
    }

    /// The tab is closed: playback stops and the windows disappear. The Multiview shuts its players down before their windows are
    /// destroyed (see `teardown`; dropping the tab would destroy a window under a running player and end the application).
    fn cleanup_session_resources(session: &MediaTabSession) {
        session.stop_native();
    }

    /// The tab that is shown after the tab at `closed_index` was closed: the one that took its place, else the one before.
    fn resolve_next_active_tab(tabs: &mut TabCollection, closed_index: usize) -> Option<TabId> {
        if tabs.is_empty() {
            return None;
        }
        let next_idx = if closed_index < tabs.len() {
            closed_index
        } else {
            closed_index.saturating_sub(1)
        };
        tabs.get_at(next_idx).map(|tab| tab.id)
    }
}
