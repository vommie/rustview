//! Thread-safe face of the Multiview. The session itself (Slint window, timers) lives on the UI thread; this handle is what
//! the rest of the application holds: open/close requests, the F2 rename target and the "is a Multiview open" flag that the
//! drag-and-drop lock, dialogs and the tab embedding ask about.

use super::bar_overlay::{self, BarKind};
use super::eligibility;
use super::navigation;
use super::tab_host;
use super::tab_single;
use super::session::{self, Session, TabReturn};
use crate::library::Library;
use crate::library::models::MediaId;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use crate::ui::MainWindow;
use crate::ui::bridge_file_selection::BridgeFileSelection;
use crate::ui::bridge_state::UiState;
use crate::ui::bridge_tab_grid::BridgeTabGrid;
use crate::ui::bridge_viewer_tabs::BridgeViewerTabs;
use crate::ui::media_open_with::{MediaOpenWith, OpenWithChoice};
use crate::settings::schema::MULTIVIEW_DOUBLE_CLICK_MS;
use crate::viewer::multiview::click::{ClickDetector, ClickKind};
use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
use crate::viewer::multiview::media_actions::MediaActionId;
use crate::viewer::multiview::view::Navigation;
use crate::viewer::{TabId, ViewerTabManager};
use parking_lot::{Mutex, RwLock};
use slint::ComponentHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static OPEN: AtomicBool = AtomicBool::new(false);
/// The open Multiview belongs to a tab: it is isolated from the Browse view, which stays locked while it is shown.
static ISOLATED: AtomicBool = AtomicBool::new(false);
/// The rename request that is being handled comes from the fullscreen window (F2 there), not from the main window.
static RENAME_FROM_FULLSCREEN: AtomicBool = AtomicBool::new(false);
/// The medium the delete request that is being handled was made for in the fullscreen window (DEL there, or the delete
/// button of a tile); `None` for a delete request of the main window.
static DELETE_FROM_FULLSCREEN: Mutex<Option<MediaId>> = Mutex::new(None);

pub(super) fn set_open(open: bool) {
    OPEN.store(open, Ordering::SeqCst);
    if !open {
        ISOLATED.store(false, Ordering::SeqCst);
    }
}

pub(super) fn set_isolated(isolated: bool) {
    ISOLATED.store(isolated, Ordering::SeqCst);
}

/// True while the F2 rename request that is being handled was made in the fullscreen window.
pub fn rename_requested_from_fullscreen() -> bool {
    RENAME_FROM_FULLSCREEN.load(Ordering::SeqCst)
}

/// The medium the delete request that is being handled was made for in the fullscreen window, if it was made there.
pub fn delete_requested_from_fullscreen() -> Option<MediaId> {
    *DELETE_FROM_FULLSCREEN.lock()
}

/// What the Multiview needs from the application to act on the Browse view and the tabs.
#[derive(Clone)]
pub struct AppContext {
    pub tab_manager: Arc<ViewerTabManager>,
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

/// What the application does when the fullscreen closes: the view it returns to ([`KEEP_VIEW`], 1 = tab view, 2 = tab grid)
/// and the tab that was shown.
pub type CloseHandler = Arc<dyn Fn(i32, Option<TabId>) + Send + Sync>;

struct Shared {
    main_window: Mutex<Option<slint::Weak<MainWindow>>>,
    close_handler: Mutex<Option<CloseHandler>>,
    /// Double clicks on the image canvas of the tab view (they open the tab in the fullscreen).
    canvas_clicks: Mutex<ClickDetector>,
    app: Mutex<Option<AppContext>>,
    /// The instance shown by the open fullscreen Multiview (it owns the rename target).
    current: Mutex<WeakInstance>,
}

#[derive(Clone)]
pub struct MultiviewHandle {
    shared: Arc<Shared>,
}

impl Default for MultiviewHandle {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                main_window: Mutex::new(None),
                close_handler: Mutex::new(None),
                canvas_clicks: Mutex::new(ClickDetector::default()),
                app: Mutex::new(None),
                current: Mutex::new(WeakInstance::new()),
            }),
        }
    }
}

impl MultiviewHandle {
    /// Sets what the application does when the fullscreen closes (restoring the view, the video embedding, the status).
    pub fn set_close_handler(&self, handler: impl Fn(i32, Option<TabId>) + Send + Sync + 'static) {
        *self.shared.close_handler.lock() = Some(Arc::new(handler));
    }

    /// A click on the image canvas of the tab view: true for the second click of a double click.
    pub fn canvas_double_click(&self) -> bool {
        self.shared.canvas_clicks.lock().register(
            Instant::now(),
            false,
            Duration::from_millis(MULTIVIEW_DOUBLE_CLICK_MS),
        ) == ClickKind::Double
    }

    /// Shows the active tab in the fullscreen. UI thread only.
    pub fn open_active_tab(
        &self,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        main_window: slint::Weak<MainWindow>,
    ) -> bool {
        match tab_manager.get_active_tab() {
            Some(tab) => self.open_tab(tab.id, tab_manager, settings, main_window),
            None => {
                tracing::warn!("[Multiview: OpenTab] No active tab to show in fullscreen");
                false
            }
        }
    }

    pub fn is_open(&self) -> bool {
        OPEN.load(Ordering::SeqCst)
    }

    /// True while a Multiview that belongs to a tab is shown: it locks the Browse view and nothing follows the selection.
    pub fn is_isolated_open(&self) -> bool {
        ISOLATED.load(Ordering::SeqCst)
    }

    /// The media an F2 rename acts on (the tile the user pressed F2 over) while the fullscreen Multiview is open.
    pub fn rename_target(&self) -> Option<MediaId> {
        if !self.is_open() {
            return None;
        }
        let instance = self.shared.current.lock().upgrade()?;
        instance.lock().rename_target()
    }

    /// Connects the Multiview to the application once at startup: it needs the main window (rename dialog, close handler,
    /// actions of the tab view), the tab manager (converting the fullscreen into a tab) and the Browse state (previous/next,
    /// deselecting a removed medium, opening a medium as a tab).
    pub fn bind(&self, app: AppContext, main_window: slint::Weak<MainWindow>) {
        *self.shared.app.lock() = Some(app);
        *self.shared.main_window.lock() = Some(main_window);
    }

    fn app(&self) -> Option<AppContext> {
        self.shared.app.lock().clone()
    }

    pub(super) fn tab_manager(&self) -> Option<Arc<ViewerTabManager>> {
        self.app().map(|app| app.tab_manager)
    }

    /// The application and its main window, for the actions that reach into the Browse view. UI thread only.
    fn app_and_window(&self) -> Option<(AppContext, MainWindow)> {
        let window = self.main_window()?.upgrade()?;
        Some((self.app()?, window))
    }

    /// Action button "open in a background tab". Returns whether a tab exists afterwards.
    pub(super) fn open_tile_as_tab(&self, media_id: MediaId) -> bool {
        let Some((app, window)) = self.app_and_window() else {
            return false;
        };
        BridgeViewerTabs::open_media_tab_background(
            media_id,
            &window,
            &app.tab_manager,
            &app.library,
            &app.settings,
            &app.state,
        )
        .is_some()
    }

    /// A medium left the Multiview of a tab through its remove button: the tab bar, the inspector, the status bar and the
    /// targets of tag and rating assignment (the tab's remaining media) catch up. UI thread only.
    pub(super) fn tab_members_changed(&self) {
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        BridgeViewerTabs::sync_ui(&window, &app.tab_manager, &app.library, &app.settings, &app.state);
        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(&window, &app.tab_manager, &app.library, &app.settings, &app.state);
        }
        tracing::debug!("[Multiview: TabMembers] The application caught up with the media left in the tab");
    }

    /// A medium left the Multiview through its remove button: it is deselected in the Browse view too.
    pub(super) fn deselect_in_browse(&self, media_id: MediaId) {
        if let Some((app, window)) = self.app_and_window() {
            BridgeFileSelection::deselect_media(media_id, &window, &app.library, &app.state);
        }
    }

    /// Previous/next for a single image: the neighbor in the Browse view is selected (and revealed there), and the
    /// Multiview follows that selection at once. A sticky image refuses and flashes its sticky button.
    pub(super) fn navigate(&self, forward: bool) {
        let Some(instance) = self.shared.current.lock().upgrade() else {
            return;
        };
        let (current, navigation) = {
            let guard = instance.lock();
            (guard.solo_tile().map(|(id, _, _)| id), guard.navigation())
        };
        drop(instance);
        match (navigation, current) {
            (Navigation::Blocked, _) => {
                if let Some(instance) = self.shared.current.lock().upgrade() {
                    instance.lock().flash_blocked_navigation();
                }
                session::with_session(Session::refresh);
            }
            (Navigation::Allowed, Some(current)) => {
                let Some((app, window)) = self.app_and_window() else {
                    return;
                };
                let target = navigation::neighbor(&app.state.read(), current, forward);
                let Some(target) = target else {
                    tracing::debug!(
                        current,
                        forward,
                        "[Multiview: Navigate] No other showable medium in the Browse view"
                    );
                    return;
                };
                tracing::info!(current, target, forward, "[Multiview: Navigate] Moving on in the Browse view");
                BridgeFileSelection::select_and_reveal_media(
                    target,
                    &window,
                    &app.library,
                    Some(&app.thumbnails),
                    &app.settings,
                    &app.state,
                );
                session::with_session(Session::follow_now);
            }
            _ => {}
        }
    }

    pub(super) fn main_window(&self) -> Option<slint::Weak<MainWindow>> {
        self.shared.main_window.lock().clone()
    }

    /// Makes `instance` the one the fullscreen window shows. A Multiview that follows the Browse selection is no tab's
    /// property, so it asks the tab manager to deliver library events (rename, delete, refresh) to it.
    pub(super) fn bind_instance(&self, instance: &SharedInstance, observe: bool) {
        *self.shared.current.lock() = Arc::downgrade(instance);
        if observe && let Some(tab_manager) = self.tab_manager() {
            tab_manager.observe_instance(instance);
        }
    }

    /// Opens the Multiview with the playable items of the current selection (one fills the display, up to four are shown
    /// side by side); if one that follows the selection is open already, it follows the selection now. UI thread only.
    /// Returns `false` if the selection contains nothing playable.
    pub fn open(
        &self,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        main_window: slint::Weak<MainWindow>,
    ) -> bool {
        let plan = eligibility::plan_open(&state.read());
        if !plan.summary.is_actionable() {
            tracing::info!(
                selected = plan.summary.selected,
                playable = plan.summary.playable,
                "[Multiview: Open] Ignored: no playable file in the selection"
            );
            return false;
        }
        if self.is_open() && !self.is_isolated_open() {
            tracing::debug!("[Multiview: Open] Already open: following the selection instead of reopening");
            session::schedule_follow(Pace::Immediate);
            return true;
        }
        *self.shared.main_window.lock() = Some(main_window.clone());
        session::open(
            self.clone(),
            plan,
            settings.clone(),
            state.clone(),
            main_window,
        )
    }

    /// Shows tab `tab_id` in the fullscreen window (from the tab view or the tab grid): the Multiview a tab owns as it is, a
    /// normal image or video tab as a Multiview of one tile that stays isolated from the Browse view. UI thread only.
    /// Returns `false` if the tab does not exist or cannot be shown.
    pub fn open_tab(
        &self,
        tab_id: TabId,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        main_window: slint::Weak<MainWindow>,
    ) -> bool {
        let Some(tab) = tab_manager.get_tab(tab_id) else {
            return false;
        };
        let instance = match tab.multiview() {
            Some(multiview) => multiview.instance.clone(),
            None => match tab_single::instance_for_tab(self, &tab) {
                Some(instance) => instance,
                None => return false,
            },
        };
        let origin_mode = main_window
            .upgrade()
            .map_or(1, |w| w.get_active_view_mode());
        *self.shared.main_window.lock() = Some(main_window.clone());
        session::open_tab(
            self.clone(),
            instance,
            TabReturn {
                tab_id,
                origin_mode,
            },
            settings.clone(),
            main_window,
        )
    }

    /// A tab changed (closed, refreshed, renamed): a fullscreen that shows a tab closes when that tab is gone.
    pub fn sync_tab(&self, tab_manager: &ViewerTabManager) {
        if self.is_isolated_open() {
            session::close_if_tab_gone(tab_manager);
        }
    }

    /// Closes the Multiview and runs the close handler (re-syncs the Browse view). Callable from any thread.
    pub fn request_close(&self) {
        if self.is_open() {
            let _ = slint::invoke_from_event_loop(|| {
                session::close_by_user();
            });
        }
    }

    /// The primary bar's cursor button: stores the new value in the application's settings (the one source of truth the
    /// settings dialog shares); `prefs` and the open hosts follow through the settings subscriber.
    pub(super) fn set_idle_cursor_hide(&self, on: bool) {
        let Some(app) = self.app() else {
            return;
        };
        let mut settings = app.settings.get();
        settings.fullscreen.primary_bar.hide_idle_cursor = on;
        app.settings.update(settings);
        tracing::info!(on, "[Multiview: Cursor] Hiding of the idle mouse cursor changed by the bar button");
    }

    /// The application exits: the fullscreen (pinned or not) and the windows of both action bars are closed. UI thread only.
    pub fn shutdown(&self) {
        session::shutdown();
        super::tab_host::shutdown();
        super::tab_plain::shutdown();
        super::bar_overlay::shutdown();
    }

    /// Closes the Multiview without the close handler because another fullscreen replaces it. UI thread only.
    pub fn dismiss(&self) {
        if self.is_open() {
            session::close(false);
        }
    }

    /// F2 on a tile: asks the main window for the regular rename dialog, which acts on `rename_target()`. The request is
    /// marked as coming from the fullscreen window while it is handled; an F2 in the main window always means the Browse
    /// selection, even with a Multiview open.
    pub fn request_rename(&self) {
        if let Some(main_window) = self.shared.main_window.lock().clone() {
            let _ = main_window.upgrade_in_event_loop(|w| {
                RENAME_FROM_FULLSCREEN.store(true, Ordering::SeqCst);
                w.invoke_request_rename();
                RENAME_FROM_FULLSCREEN.store(false, Ordering::SeqCst);
            });
        }
    }

    /// DEL on the single medium or the delete button of a tile: asks the main window for the regular delete dialog (the same
    /// flow as DEL in the Browse view) for `media_id`. The request is marked as coming from the fullscreen window while it is
    /// handled; DEL in the main window always means the Browse selection, even with a Multiview open.
    pub fn request_delete(&self, media_id: MediaId) {
        if let Some(main_window) = self.shared.main_window.lock().clone() {
            let _ = main_window.upgrade_in_event_loop(move |w| {
                *DELETE_FROM_FULLSCREEN.lock() = Some(media_id);
                w.invoke_request_delete();
                *DELETE_FROM_FULLSCREEN.lock() = None;
            });
        }
    }

    /// Media were deleted through the Multiview (the dialog opened by `request_delete` did its work): their tiles leave the
    /// Multiview that shows them, the last one closes it. UI thread only.
    pub fn drop_deleted(&self, ids: &[MediaId]) {
        if self.is_open() {
            session::run(|s| s.drop_deleted(ids));
        } else {
            tab_host::drop_deleted(ids);
            self.close_plain_tab_of(ids);
        }
    }

    /// A normal image or video tab whose medium was deleted through its action bar closes, like the last medium of a Multiview
    /// does. Only the active tab can have asked: the bar belongs to the tab that is shown.
    fn close_plain_tab_of(&self, ids: &[MediaId]) {
        let Some(tab) = self.tab_manager().and_then(|tm| tm.get_active_tab()) else {
            return;
        };
        if !tab.is_multiview() && ids.contains(&tab.media_id) {
            tracing::info!(media_id = tab.media_id, "[Multiview: PlainTab] The medium of the tab was deleted: closing the tab");
            self.close_active_tab();
        }
    }

    /// The "Open With" button of a tile: the drop-down with the applications of the medium opens below the bar, or closes if
    /// it is open. The applications are looked up now, on the click, never while the bar is merely shown. UI thread only.
    pub(super) fn toggle_open_with(&self, media_id: MediaId) {
        if bar_overlay::close_clicked_menu(BarKind::Media) {
            return;
        }
        let Some((app, _)) = self.app_and_window() else {
            return;
        };
        let Some(path) = self.media_path(&app, media_id) else {
            tracing::warn!(media_id, "[Multiview: OpenWith] No file path known for the medium");
            return;
        };
        let apps = MediaOpenWith::discover_apps(&app.state, &path);
        bar_overlay::open_menu(BarKind::Media, MediaActionId::OpenWith.code(), &MediaOpenWith::menu_items(&apps));
    }

    /// An item of the "Open With" drop-down was chosen: the medium is opened with that application, or the native chooser
    /// opens, exactly like the same items of the context menu of a medium.
    pub(super) fn open_with_item(&self, media_id: MediaId, item: i32) {
        let item = u64::try_from(item).unwrap_or_default();
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        let Some(path) = self.media_path(&app, media_id) else {
            return;
        };
        let paths = [path];
        match MediaOpenWith::choice_of(item) {
            Some(OpenWithChoice::App(index)) => MediaOpenWith::launch(&window, &app.state, &paths, index),
            Some(OpenWithChoice::Other) => MediaOpenWith::choose_application(&window, &paths),
            None => tracing::debug!(item, "[Multiview: OpenWith] Ignored an unknown menu item"),
        }
    }

    /// The file of a medium: the loaded folder or the library know it, else the tab that shows it.
    fn media_path(&self, app: &AppContext, media_id: MediaId) -> Option<std::path::PathBuf> {
        MediaOpenWith::paths_for(&app.state, &app.library, &[media_id])
            .into_iter()
            .next()
            .or_else(|| app.tab_manager.get_tab_by_media_id(media_id).map(|tab| tab.file_path.clone()))
    }

    /// Closes the tab that is shown in the main window (Ctrl+W on a video of a Multiview tab).
    pub fn close_active_tab(&self) {
        if let Some(main_window) = self.shared.main_window.lock().clone() {
            let _ = main_window.upgrade_in_event_loop(|w| w.invoke_viewer_close_tab());
        }
    }

    /// Runs the application's close handler. A Multiview that came from a tab names the view it returns to and releases the
    /// lock it put on the main window; one that followed the Browse selection leaves the main window as it is.
    pub(super) fn notify_closed(&self, tab: Option<TabReturn>) {
        let handler = self.shared.close_handler.lock().clone();
        let main_window = self.shared.main_window.lock().clone();
        match (handler, main_window) {
            (Some(handler), Some(main_window)) => {
                let _ = main_window.upgrade_in_event_loop(move |w| match tab {
                    Some(tab) => {
                        w.set_is_fullscreen_locked(false);
                        handler(tab.origin_mode, Some(tab.tab_id));
                        // The handler shows the tab again in an event of its own, queued just now: the placeholders of the
                        // fullscreen go away after it, not before (no gap with a stale or empty picture).
                        let _ = w.as_weak().upgrade_in_event_loop(|w| w.set_fullscreen_tab_id(-1));
                    }
                    None => handler(KEEP_VIEW, None),
                });
            }
            (None, Some(main_window)) => {
                let _ = main_window.upgrade_in_event_loop(|w| w.set_fullscreen_tab_id(-1));
            }
            _ => {}
        }
    }

    /// Opens the tab that owns the Multiview instance `instance_id` in the fullscreen window, through the same main window
    /// callback as a middle click on its grid item.
    pub(super) fn open_tab_fullscreen(&self, instance_id: u64) {
        self.on_tab_of_instance(instance_id, |w, tab_id| {
            w.invoke_tab_grid_item_middle_clicked(tab_id as i32)
        });
    }

    /// Makes the tab that owns the Multiview instance `instance_id` the shown tab (left click on its grid item).
    pub(super) fn activate_tab(&self, instance_id: u64) {
        self.on_tab_of_instance(instance_id, |w, tab_id| {
            w.invoke_tab_grid_item_left_clicked(tab_id as i32)
        });
    }

    fn on_tab_of_instance(
        &self,
        instance_id: u64,
        f: impl FnOnce(&MainWindow, TabId) + Send + 'static,
    ) {
        let (Some(tab_manager), Some(main_window)) = (self.tab_manager(), self.main_window())
        else {
            return;
        };
        let Some(tab_id) = tab_manager.tab_id_of_instance(instance_id) else {
            return;
        };
        let _ = main_window.upgrade_in_event_loop(move |w| f(&w, tab_id));
    }

    /// Scrolls the tab grid (wheel over a grid item that shows a Multiview).
    pub(super) fn scroll_grid(&self, up: bool) {
        if let Some(main_window) = self.main_window() {
            let _ = main_window.upgrade_in_event_loop(move |w| {
                w.invoke_scroll_media_by(if up { 40.0 } else { -40.0 })
            });
        }
    }
}

/// Origin mode for the close handler that means "the main window shows what it shows now": a Multiview that followed the
/// Browse selection must not pull the user back to the Browse view.
pub const KEEP_VIEW: i32 = -1;

/// How soon a changed Browse selection is applied to the open Multiview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// A discrete action (click, key, previous/next): applied at once.
    Immediate,
    /// A continuous gesture (rubber band): applied once the selection has been quiet for a moment.
    Settled,
    /// A burst of keys that each move the selection (type-ahead find): applied once the selection has been quiet for this long,
    /// so a video is not opened for every letter typed.
    After(Duration),
}

/// The user changed the selection in the Browse view (click, keys, rubber band, select all). Only these actions drive a
/// Multiview that follows the selection; a folder change, query or refresh that happens to change what is selected does
/// not. Costs one atomic load while no Multiview is open.
pub fn on_user_selection_changed(pace: Pace) {
    if OPEN.load(Ordering::Relaxed) {
        session::schedule_follow(pace);
    }
}
