use super::error::ViewerError;
use super::multiview::events::MultiviewEventSink;
use super::multiview::instance::{SharedInstance, WeakInstance};
use super::tab_item::TabCollection;
use super::tab_ui_gate::TabUiGate;
use super::types::{MediaTabSession, TabId};

use parking_lot::{Mutex, RwLock};
use rustview_core::library::Library;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64};

mod close;
mod merge;
mod open;
mod ordering;
mod playback;
mod query;

/// High-level workspace coordinator managing open tabs, player lifecycles, and window embedding (SRP/OCP).
pub struct ViewerTabManager {
    tabs: RwLock<TabCollection>,
    active_tab_id: RwLock<Option<TabId>>,
    tab_counter: AtomicU64,
    library: Arc<Library>,
    cached_viewport_rect: RwLock<(i32, i32, u32, u32)>,
    /// A context menu of the main window is open (no bars meanwhile).
    menu_open: AtomicBool,
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
            menu_open: AtomicBool::new(false),
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
    // Queries & SPoT Title Calculation
    // =========================================================================

    pub fn list_tabs(&self) -> Vec<MediaTabSession> {
        self.tabs.read().to_vec()
    }

    /// The Multiview of every tab (cheap: no tab is copied).
    pub fn instances(&self) -> Vec<SharedInstance> {
        self.tabs
            .read()
            .all()
            .iter()
            .map(|tab| tab.multiview.instance.clone())
            .collect()
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

    // =========================================================================
    // Tab Activation, Browsing & Closing
    // =========================================================================

    pub fn set_active_tab(&self, tab_id: TabId) -> bool {
        self.set_active_tab_internal(tab_id, false)
    }
}
