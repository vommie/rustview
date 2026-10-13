use super::bridge_file_selection::{BridgeFileSelection, DoubleClickKind, LastClickRecord};
use super::bridge_state::UiState;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::multiview::MultiviewHandle;
use super::{ImageFullscreenWindow, MainWindow};
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::MonitorInfo;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::{
    force_x11_fullscreen_on_monitor, get_raw_window_xid, HighResImageLoader, MediaTabSession,
    MpvPlayer, TabId, TabPayload, ViewerTabManager, X11VideoSubwindow,
};
use parking_lot::{Mutex, RwLock};
use slint::{CloseRequestResponse, ComponentHandle, Image, Timer};
use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

thread_local! {
    static FULLSCREEN_WINDOW: RefCell<Option<ImageFullscreenWindow>> = const { RefCell::new(None) };
}

pub type FullscreenCloseCallback =
    Arc<dyn Fn(i32 /* origin_mode */, Option<TabId>, Option<MediaId>) + Send + Sync>;

/// Specialized controller managing the single, persistent Fullscreen Media Viewer window.
/// Enforces strict exclusivity: Tab-Fullscreen shows ONLY the tab's own file and cannot navigate.
/// Quick-Fullscreen operates dynamically within the Browse View context.
#[derive(Clone, Default)]
pub struct ImageFullscreenDialogController {
    window_weak: Arc<Mutex<Option<slint::Weak<ImageFullscreenWindow>>>>,
    main_window_weak: Arc<Mutex<Option<slint::Weak<MainWindow>>>>,
    tab_manager_weak: Arc<Mutex<Option<Arc<ViewerTabManager>>>>,
    library_weak: Arc<Mutex<Option<Arc<Library>>>>,
    thumbnails_weak: Arc<Mutex<Option<Arc<ThumbnailManager>>>>,
    state_weak: Arc<Mutex<Option<Arc<RwLock<UiState>>>>>,
    settings_weak: Arc<Mutex<Option<Arc<SettingsManager>>>>,
    last_click_record: Arc<Mutex<Option<LastClickRecord>>>,
    is_active: Arc<AtomicBool>,
    is_fullscreen_video: Arc<AtomicBool>,
    active_fullscreen_tab_id: Arc<Mutex<Option<TabId>>>,
    active_standalone_media_id: Arc<Mutex<Option<MediaId>>>,
    standalone_video_player: Arc<Mutex<Option<Arc<MpvPlayer>>>>,
    standalone_subwindow: Arc<Mutex<Option<X11VideoSubwindow>>>,
    origin_view_mode: Arc<Mutex<i32>>,
    cached_main_xid: Arc<Mutex<Option<u64>>>,
    cached_fs_xid: Arc<Mutex<Option<u64>>>,
    cached_viewport_rect: Arc<Mutex<(i32, i32, u32, u32)>>,
    close_handler: Arc<Mutex<Option<FullscreenCloseCallback>>>,
    multiview: MultiviewHandle,
}

impl ImageFullscreenDialogController {
    pub fn new() -> Self {
        let close_handler = Arc::new(Mutex::new(None));
        Self {
            multiview: MultiviewHandle::new(close_handler.clone()),
            close_handler,
            window_weak: Arc::new(Mutex::new(None)),
            main_window_weak: Arc::new(Mutex::new(None)),
            tab_manager_weak: Arc::new(Mutex::new(None)),
            library_weak: Arc::new(Mutex::new(None)),
            thumbnails_weak: Arc::new(Mutex::new(None)),
            state_weak: Arc::new(Mutex::new(None)),
            settings_weak: Arc::new(Mutex::new(None)),
            last_click_record: Arc::new(Mutex::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
            is_fullscreen_video: Arc::new(AtomicBool::new(false)),
            active_fullscreen_tab_id: Arc::new(Mutex::new(None)),
            active_standalone_media_id: Arc::new(Mutex::new(None)),
            standalone_video_player: Arc::new(Mutex::new(None)),
            standalone_subwindow: Arc::new(Mutex::new(None)),
            origin_view_mode: Arc::new(Mutex::new(1)),
            cached_main_xid: Arc::new(Mutex::new(None)),
            cached_fs_xid: Arc::new(Mutex::new(None)),
            cached_viewport_rect: Arc::new(Mutex::new((0, 0, 800, 600))),
        }
    }

    /// The Multiview (several media in one fullscreen window) shares this controller's close handler and open state.
    pub fn multiview(&self) -> &MultiviewHandle {
        &self.multiview
    }

    /// Stores the ThumbnailManager instance to facilitate smooth slice re-anchoring when browsing from fullscreen.
    pub fn set_thumbnails(&self, thumbnails: Arc<ThumbnailManager>) {
        *self.thumbnails_weak.lock() = Some(thumbnails);
    }

    /// Registers a closure executed when the fullscreen window closes, passing the originating view mode,
    /// optional tab ID, and optional standalone media ID.
    pub fn set_close_handler<F>(&self, handler: F)
    where
        F: Fn(i32, Option<TabId>, Option<MediaId>) + Send + Sync + 'static,
    {
        *self.close_handler.lock() = Some(Arc::new(handler));
    }

    /// Stores the main window XID and latest tab viewport rectangle for restoring on fullscreen exit.
    pub fn set_main_viewport_info(&self, main_xid: u64, x: i32, y: i32, w: u32, h: u32) {
        *self.cached_main_xid.lock() = Some(main_xid);
        *self.cached_viewport_rect.lock() = (x, y, w, h);
    }

    /// Returns the active media ID being viewed in fullscreen if available (in the Multiview: the tile F2 acts on).
    pub fn get_active_media_id(&self) -> Option<MediaId> {
        if self.multiview.is_open() {
            return self.multiview.rename_target();
        }
        if let Some(tab_id) = *self.active_fullscreen_tab_id.lock()
            && let Some(ref tm) = *self.tab_manager_weak.lock()
            && let Some(tab) = tm.get_tab(tab_id)
        {
            Some(tab.media_id)
        } else {
            *self.active_standalone_media_id.lock()
        }
    }

    /// Evaluates if successive clicks qualify as a double-click (< 400ms interval).
    pub fn check_double_click(&self, is_ctrl: bool) -> DoubleClickKind {
        let now = Instant::now();
        let mut guard = self.last_click_record.lock();
        if let Some(last) = *guard
            && now.duration_since(last.instant) < Duration::from_millis(400)
        {
            *guard = None;
            if is_ctrl && last.is_ctrl {
                return DoubleClickKind::Ctrl;
            } else if !is_ctrl && !last.is_ctrl {
                return DoubleClickKind::Normal;
            }
        }
        *guard = Some(LastClickRecord {
            media_id: 0,
            instant: now,
            is_ctrl,
        });
        DoubleClickKind::None
    }

    /// Resolves the configured monitor or evaluates the fallback rule with Main UI window detection.
    fn resolve_current_target_monitor(&self, settings: &SettingsManager) -> MonitorInfo {
        let main_geom = self.main_window_weak.lock().as_ref().and_then(|mw| {
            mw.upgrade().map(|w| {
                let pos = w.window().position();
                let size = w.window().size();
                (pos.x, pos.y, size.width, size.height)
            })
        });

        crate::system::SystemDisplayService::resolve_target_monitor(&settings.get().fullscreen, main_geom)
    }

    /// Applies position and dimensions to the Slint fullscreen window matching the target monitor bounds.
    fn position_fullscreen_window(&self, win: &ImageFullscreenWindow, target_mon: &MonitorInfo) {
        tracing::info!(
            connector = %target_mon.connector,
            index = target_mon.index,
            x = target_mon.x,
            y = target_mon.y,
            width = target_mon.width,
            height = target_mon.height,
            "[FullscreenViewer: Position] Positioning fullscreen window onto target display"
        );
        win.window()
            .set_position(slint::PhysicalPosition::new(target_mon.x, target_mon.y));
        win.window()
            .set_size(slint::PhysicalSize::new(target_mon.width, target_mon.height));
    }

    /// Applies X11 EWMH monitor fullscreen and geometry to the mapped X11 window.
    fn apply_x11_monitor_fullscreen(&self, fs_xid: u64, target_mon: &MonitorInfo) {
        if fs_xid == 0 {
            return;
        }
        force_x11_fullscreen_on_monitor(
            fs_xid,
            target_mon.index,
            target_mon.x,
            target_mon.y,
            target_mon.width,
            target_mon.height,
        );
    }

    /// Resolves the raw native X11 Window ID belonging exclusively to the fs_win instance.
    /// Under NO CIRCUMSTANCES will this ever search the process or return another window.
    fn resolve_fs_xid(&self, fs_win: &ImageFullscreenWindow) -> u64 {
        if let Some(cached) = *self.cached_fs_xid.lock()
            && cached > 0
        {
            return cached;
        }

        if let Some(xid) = get_raw_window_xid(fs_win.window())
            && xid > 0
        {
            *self.cached_fs_xid.lock() = Some(xid);
            return xid;
        }

        0
    }

    /// Lazily initializes or retrieves the single thread-local ImageFullscreenWindow instance without cloning.
    pub fn with_fullscreen_window<R>(&self, f: impl FnOnce(&ImageFullscreenWindow) -> R) -> R {
        let ctrl = self.clone();
        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();
            if cell.is_none() {
                let win = ImageFullscreenWindow::new()
                    .expect("Failed to create ImageFullscreenWindow Slint component");

                let ctrl_close = ctrl.clone();
                win.on_request_close(move || {
                    ctrl_close.close();
                });

                let ctrl_ren = ctrl.clone();
                win.on_request_rename(move || {
                    if let Some(ref mw_weak) = *ctrl_ren.main_window_weak.lock()
                        && let Some(mw) = mw_weak.upgrade()
                    {
                        mw.invoke_request_rename();
                    }
                });

                let ctrl_click = ctrl.clone();
                win.on_canvas_clicked(move |is_ctrl| match ctrl_click.check_double_click(is_ctrl) {
                    DoubleClickKind::Normal => {
                        ctrl_click.close();
                    }
                    DoubleClickKind::Ctrl => {
                        ctrl_click.handle_ctrl_double_click();
                    }
                    DoubleClickKind::None => {}
                });

                let ctrl_next = ctrl.clone();
                win.on_next_image(move || {
                    if ctrl_next.active_fullscreen_tab_id.lock().is_some() {
                        return;
                    }
                    ctrl_next.navigate_standalone_adjacent(true);
                });

                let ctrl_prev = ctrl.clone();
                win.on_previous_image(move || {
                    if ctrl_prev.active_fullscreen_tab_id.lock().is_some() {
                        return;
                    }
                    ctrl_prev.navigate_standalone_adjacent(false);
                });

                let ctrl_sys_close = ctrl.clone();
                win.window().on_close_requested(move || {
                    ctrl_sys_close.close();
                    CloseRequestResponse::HideWindow
                });

                *ctrl.window_weak.lock() = Some(win.as_weak());
                *cell = Some(win);
            }
            let win = cell.as_ref().unwrap();
            f(win)
        })
    }

    /// Displays the frameless fullscreen media window for the active tab.
    pub fn show(
        &self,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let Some(active_tab) = tab_manager.get_active_tab() else {
            tracing::warn!(
                "[FullscreenViewer: Show] Cannot show fullscreen: no active viewer tab session"
            );
            return;
        };

        self.show_for_tab(active_tab.id, tab_manager, settings, main_window_weak);
    }

    /// Displays the frameless fullscreen media window for any specific tab without changing active tab state (auto-plays).
    pub fn show_for_tab(
        &self,
        tab_id: TabId,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let Some(target_tab) = tab_manager.get_tab(tab_id) else {
            tracing::warn!(tab_id, "[FullscreenViewer: ShowForTab] Tab session not found");
            return;
        };

        self.multiview.dismiss();

        let t0 = Instant::now();
        *self.main_window_weak.lock() = Some(main_window_weak.clone());
        *self.tab_manager_weak.lock() = Some(tab_manager.clone());
        *self.settings_weak.lock() = Some(settings.clone());

        // Cleanly detach and pause any standalone video player when entering Tab-Fullscreen
        if let Some(player) = self.standalone_video_player.lock().as_ref() {
            player.set_view_active(false);
            player.pause().ok();
        }
        if let Some(sub) = self.standalone_subwindow.lock().as_mut() {
            sub.hide();
        }

        *self.active_fullscreen_tab_id.lock() = Some(tab_id);
        *self.active_standalone_media_id.lock() = None;

        let origin_mode = main_window_weak
            .upgrade()
            .map(|mw| mw.get_active_view_mode())
            .unwrap_or(1);
        *self.origin_view_mode.lock() = origin_mode;

        let is_video = matches!(target_tab.payload, TabPayload::Video(_));
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

        let file_exists = target_tab.file_path.is_file();

        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(true);
        }

        let ctrl = self.clone();
        let tm_clone = tab_manager.clone();
        let was_already_open = self.is_active.load(Ordering::SeqCst);
        let settings_clone = settings.clone();

        self.with_fullscreen_window(|win| {
            ctrl.sync_tab_content(win, &target_tab, &settings_clone);
            win.set_is_video(is_video && file_exists);
            win.set_allow_navigation(false);

            if !file_exists {
                win.set_source_image(Image::default());
                win.set_is_loading(false);
                let missing_msg = format!(
                    "File not found on physical disk:\n{}",
                    target_tab.file_path.display()
                );
                win.set_error_message(missing_msg.into());
            }

            let target_mon = ctrl.resolve_current_target_monitor(&settings_clone);

            if !was_already_open {
                // Initialize canvas opacity to 0 to prevent any visible flash on the originating display
                win.set_is_ready(false);
                ctrl.position_fullscreen_window(win, &target_mon);
                let _ = win.show();

                let fs_xid = ctrl.resolve_fs_xid(win);
                if fs_xid > 0 {
                    ctrl.apply_x11_monitor_fullscreen(fs_xid, &target_mon);
                } else {
                    let ctrl_retry = ctrl.clone();
                    let win_weak_retry = win.as_weak();
                    let mon_retry = target_mon.clone();
                    Timer::single_shot(std::time::Duration::from_millis(15), move || {
                        if let Some(w) = win_weak_retry.upgrade() {
                            let xid = ctrl_retry.resolve_fs_xid(&w);
                            if xid > 0 {
                                ctrl_retry.apply_x11_monitor_fullscreen(xid, &mon_retry);
                            }
                        }
                    });
                }
                ctrl.is_active.store(true, Ordering::SeqCst);
            }

            if is_video && file_exists {
                ctrl.reparent_to_fullscreen(&tm_clone, win, tab_id, &target_mon);
            }

            // Window geometry is firmly settled on the target display; render canvas seamlessly
            win.set_is_ready(true);
        });

        tracing::info!(
            tab_id = target_tab.id,
            media_id = target_tab.media_id,
            is_video,
            file_exists,
            origin_mode,
            in_place_swap = was_already_open,
            filename = %LogObscurer::filename(&target_tab.filename),
            elapsed_us = t0.elapsed().as_micros(),
            "[FullscreenViewer: ShowForTab] Tab-Fullscreen activated (exclusive to tab, navigation disabled)"
        );
    }

    /// Displays the frameless fullscreen viewer directly for a single media item without creating or modifying tabs (Quick-Fullscreen Mode).
    pub fn show_for_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &Arc<ViewerTabManager>,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        if self.is_active.load(Ordering::SeqCst) && self.active_fullscreen_tab_id.lock().is_some() {
            tracing::info!(
                media_id,
                "[FullscreenViewer: ShowForMedia] Ignored Quick-Fullscreen request because Tab-Fullscreen is currently exclusive"
            );
            return;
        }

        let t0 = Instant::now();
        let record_res = library.media().get(media_id);
        let Some(record) = record_res.ok().flatten() else {
            tracing::warn!(
                media_id,
                "[FullscreenViewer: ShowForMedia] Media record not found in database"
            );
            return;
        };

        if library.resolve_media_path(media_id).is_err() {
            tracing::warn!(
                media_id,
                "[FullscreenViewer: ShowForMedia] Failed to resolve media file path"
            );
            return;
        }

        self.multiview.dismiss();

        *self.main_window_weak.lock() = Some(main_window_weak.clone());
        *self.library_weak.lock() = Some(library.clone());
        *self.settings_weak.lock() = Some(settings.clone());
        *self.state_weak.lock() = Some(state.clone());
        *self.tab_manager_weak.lock() = Some(tab_manager.clone());

        *self.active_fullscreen_tab_id.lock() = None;
        *self.active_standalone_media_id.lock() = Some(media_id);

        let origin_mode = main_window_weak
            .upgrade()
            .map(|mw| mw.get_active_view_mode())
            .unwrap_or(0);
        *self.origin_view_mode.lock() = origin_mode;

        let was_already_open = self.is_active.load(Ordering::SeqCst);
        let settings_clone = settings.clone();

        let target_mon = self.resolve_current_target_monitor(&settings_clone);

        if !was_already_open {
            let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
                "deep black" | "black" => 1,
                "transparent checkerboard" | "checkerboard" => 2,
                _ => 0,
            };

            let ctrl = self.clone();
            let mon_clone = target_mon.clone();
            self.with_fullscreen_window(|win| {
                win.set_is_ready(false);
                win.set_background_mode(bg_mode_idx);

                ctrl.position_fullscreen_window(win, &mon_clone);
                let _ = win.show();

                let fs_xid = ctrl.resolve_fs_xid(win);
                if fs_xid > 0 {
                    ctrl.apply_x11_monitor_fullscreen(fs_xid, &mon_clone);
                } else {
                    let ctrl_retry = ctrl.clone();
                    let win_weak_retry = win.as_weak();
                    let mon_retry = mon_clone.clone();
                    Timer::single_shot(std::time::Duration::from_millis(15), move || {
                        if let Some(w) = win_weak_retry.upgrade() {
                            let xid = ctrl_retry.resolve_fs_xid(&w);
                            if xid > 0 {
                                ctrl_retry.apply_x11_monitor_fullscreen(xid, &mon_retry);
                            }
                        }
                    });
                }
                ctrl.is_active.store(true, Ordering::SeqCst);
            });
        }

        self.with_fullscreen_window(|win| {
            win.set_allow_navigation(true);
        });

        self.sync_standalone_media(media_id, library, settings, &target_mon);

        self.with_fullscreen_window(|win| {
            win.set_is_ready(true);
        });

        tracing::info!(
            media_id,
            origin_mode,
            in_place_swap = was_already_open,
            filename = %LogObscurer::filename(&record.filename),
            elapsed_us = t0.elapsed().as_micros(),
            "[FullscreenViewer: ShowForMedia] Quick-Fullscreen active (navigation enabled within Browse context)"
        );
    }

    #[inline]
    pub fn is_quick_fullscreen_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst) && self.active_standalone_media_id.lock().is_some()
    }

    #[inline]
    pub fn dismiss_if_quick_fullscreen(&self) {}

    pub fn update_quick_fullscreen_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
    ) {
        if self.is_quick_fullscreen_active() {
            tracing::info!(
                media_id,
                "[FullscreenViewer: UpdateQuick] Quick-Fullscreen active; dynamically replacing fullscreen content in-place"
            );
            let target_mon = self.resolve_current_target_monitor(settings);
            self.sync_standalone_media(media_id, library, settings, &target_mon);
        }
    }

    pub fn handle_ctrl_double_click(&self) {
        let origin_mode = *self.origin_view_mode.lock();
        let standalone_id_opt = *self.active_standalone_media_id.lock();

        if origin_mode != 0 || standalone_id_opt.is_none() {
            tracing::debug!(
                origin_mode,
                standalone = standalone_id_opt.is_some(),
                "[FullscreenViewer: CtrlDblClick] Ignored Ctrl+DoubleClick: Fullscreen was not opened standalone from Browse view"
            );
            return;
        }

        let media_id = standalone_id_opt.unwrap();
        let is_video = self.is_fullscreen_video.load(Ordering::SeqCst);

        let tab_mgr_opt = self.tab_manager_weak.lock().clone();
        let lib_opt = self.library_weak.lock().clone();
        let set_opt = self.settings_weak.lock().clone();
        let st_opt = self.state_weak.lock().clone();
        let mw_opt = self.main_window_weak.lock().clone();

        let (Some(tab_mgr), Some(library), Some(settings), Some(state), Some(mw_weak)) =
            (tab_mgr_opt, lib_opt, set_opt, st_opt, mw_opt)
        else {
            tracing::warn!(
                "[FullscreenViewer: CtrlDblClick] Missing subsystem reference for background tab conversion"
            );
            self.close();
            return;
        };

        tracing::info!(
            media_id,
            is_video,
            "[FullscreenViewer: CtrlDblClick] Converting standalone fullscreen media to background tab and closing fullscreen..."
        );

        if is_video {
            let player = self.standalone_video_player.lock().take();
            let subwindow = self.standalone_subwindow.lock().take();
            let allow_multiple = settings.get().viewer.videos.allow_multiple_tabs;

            if let Some(player_arc) = player
                && let Ok(new_tab_id) = tab_mgr.adopt_video_session_in_background(
                    media_id,
                    player_arc,
                    subwindow,
                    allow_multiple,
                )
            {
                tracing::info!(
                    media_id,
                    tab_id = new_tab_id,
                    "[FullscreenViewer: CtrlDblClick] Standalone video successfully adopted into background tab session"
                );
            }
        } else if let Some(mw) = mw_weak.upgrade() {
            BridgeViewerTabs::open_media_tab_background(
                media_id,
                &mw,
                &tab_mgr,
                &library,
                &settings,
                &state,
            );
        }

        self.close();
    }

    fn init_or_play_standalone_video(
        &self,
        fs_win: &ImageFullscreenWindow,
        media_id: MediaId,
        file_path: &Path,
        target_mon: &MonitorInfo,
    ) {
        if !file_path.is_file() {
            tracing::warn!(
                media_id,
                file_path = %LogObscurer::path(file_path),
                "[FullscreenViewer: StandaloneVideo] Target video file does not exist on disk, skipping subwindow display"
            );
            return;
        }

        let fs_xid = self.resolve_fs_xid(fs_win);

        if fs_xid == 0 {
            tracing::debug!(
                media_id,
                "[FullscreenViewer: StandaloneVideo] Fullscreen XID not ready yet. Scheduling fast retry..."
            );
            let ctrl = self.clone();
            let win_weak = fs_win.as_weak();
            let fpath = file_path.to_path_buf();
            let mon_clone = target_mon.clone();
            Timer::single_shot(std::time::Duration::from_millis(15), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.init_or_play_standalone_video(&w, media_id, &fpath, &mon_clone);
                }
            });
            return;
        }

        self.apply_x11_monitor_fullscreen(fs_xid, target_mon);

        let screen_w = target_mon.width.max(640);
        let screen_h = target_mon.height.max(480);

        let mut player_opt = self.standalone_video_player.lock();
        let mut sub_opt = self.standalone_subwindow.lock();

        let player = match (&*player_opt, &mut *sub_opt) {
            (Some(p), Some(sub)) => {
                if sub.parent_xid() != fs_xid {
                    let _ = sub.reparent(fs_xid, 0, 0, screen_w, screen_h);
                } else {
                    sub.move_resize(0, 0, screen_w, screen_h);
                }
                sub.show();
                p.clone()
            }
            _ => {
                let mut sub = match X11VideoSubwindow::new(fs_xid, 0, 0, screen_w, screen_h) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "[FullscreenViewer: Subwindow] Failed to create X11 subwindow"
                        );
                        return;
                    }
                };
                let wid = sub.xid();
                sub.show();
                *sub_opt = Some(sub);

                let p = match MpvPlayer::new() {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            "[FullscreenViewer: MpvInit] Failed to create MpvPlayer"
                        );
                        return;
                    }
                };

                let fs_close = self.clone();
                p.on_toggle_fullscreen(move || {
                    fs_close.close();
                });
                let close_fs = self.clone();
                p.on_close_tab(move || {
                    close_fs.close();
                });

                let ctrl_fs_ren = self.clone();
                p.on_rename_requested(move || {
                    if let Some(ref mw_weak) = *ctrl_fs_ren.main_window_weak.lock() {
                        let _ = mw_weak.upgrade_in_event_loop(move |w| {
                            w.invoke_request_rename();
                        });
                    }
                });

                let fs_convert = self.clone();
                p.on_convert_to_background_tab(move || {
                    if fs_convert.check_double_click(true) == DoubleClickKind::Ctrl {
                        tracing::info!(
                            "[FullscreenViewer: MpvCtrlDblClick] Ctrl+DoubleClick intercepted from MPV video player in fullscreen"
                        );
                        fs_convert.handle_ctrl_double_click();
                    }
                });

                let _ = p.attach_to_window(wid);
                *player_opt = Some(p.clone());
                p
            }
        };

        drop(player_opt);
        drop(sub_opt);

        player.set_tab_grid_mode(false);
        player.set_view_active(true);
        player.set_user_paused(false);

        let _ = player.load_file(file_path);
        let _ = player.play();
    }

    pub fn sync_browse_selection(&self, media_id: MediaId) {
        if *self.origin_view_mode.lock() != 0 {
            return;
        }

        let Some(state_arc) = self.state_weak.lock().clone() else {
            return;
        };
        let Some(library_arc) = self.library_weak.lock().clone() else {
            return;
        };
        let Some(settings_arc) = self.settings_weak.lock().clone() else {
            return;
        };
        let Some(mw_weak) = self.main_window_weak.lock().clone() else {
            return;
        };

        let thumb_opt = self.thumbnails_weak.lock().clone();

        let _ = mw_weak.upgrade_in_event_loop(move |mw| {
            BridgeFileSelection::select_and_reveal_media(
                media_id,
                &mw,
                &library_arc,
                thumb_opt.as_ref(),
                &settings_arc,
                &state_arc,
            );
        });
    }

    pub fn navigate_standalone_adjacent(&self, is_next: bool) {
        if self.active_fullscreen_tab_id.lock().is_some() {
            tracing::debug!(
                "[FullscreenViewer: StandaloneNavigate] Bypassed navigation because Tab-Fullscreen is active"
            );
            return;
        }

        let Some(current_id) = *self.active_standalone_media_id.lock() else {
            return;
        };

        let Some(state_arc) = self.state_weak.lock().clone() else {
            return;
        };

        let Some(library_arc) = self.library_weak.lock().clone() else {
            return;
        };

        let Some(settings_arc) = self.settings_weak.lock().clone() else {
            return;
        };

        let target_media_id_opt = {
            let st = state_arc.read();

            let collection_len = if !st.active_indices.is_empty() {
                st.active_indices.len()
            } else {
                st.all_folder_media.len()
            };

            if collection_len == 0 {
                tracing::info!(
                    current_id,
                    "[FullscreenViewer: Navigate] Active collection is empty. Standalone navigation safely bypassed."
                );
                None
            } else {
                let current_pos = if !st.active_indices.is_empty() {
                    st.active_indices.iter().position(|&idx| {
                        st.all_folder_media
                            .get(idx)
                            .map(|m| m.record.id == current_id)
                            .unwrap_or(false)
                    })
                } else {
                    st.all_folder_media
                        .iter()
                        .position(|m| m.record.id == current_id)
                };

                match current_pos {
                    Some(pos) => {
                        let target_idx = if is_next {
                            (pos + 1) % collection_len
                        } else {
                            (pos + collection_len - 1) % collection_len
                        };

                        if !st.active_indices.is_empty() {
                            st.active_indices
                                .get(target_idx)
                                .and_then(|&idx| st.all_folder_media.get(idx))
                                .map(|m| m.record.id)
                        } else {
                            st.all_folder_media.get(target_idx).map(|m| m.record.id)
                        }
                    }
                    None => {
                        let recovery_idx = if is_next { 0 } else { collection_len - 1 };
                        let recovered_id = if !st.active_indices.is_empty() {
                            st.active_indices
                                .get(recovery_idx)
                                .and_then(|&idx| st.all_folder_media.get(idx))
                                .map(|m| m.record.id)
                        } else {
                            st.all_folder_media.get(recovery_idx).map(|m| m.record.id)
                        };

                        tracing::info!(
                            current_id,
                            is_next,
                            recovery_idx,
                            total_in_results = collection_len,
                            target_id = ?recovered_id,
                            "[FullscreenViewer: OrphanedNavigation] Current file not found in active query results. Anchoring to nearest collection boundary."
                        );

                        recovered_id
                    }
                }
            }
        };

        if let Some(target_id) = target_media_id_opt {
            tracing::info!(
                current_id,
                target_id,
                is_next,
                "[FullscreenViewer: StandaloneNavigate] Navigating in-place to target media in Quick-Fullscreen"
            );
            let target_mon = self.resolve_current_target_monitor(&settings_arc);
            self.sync_standalone_media(target_id, &library_arc, &settings_arc, &target_mon);
        }
    }

    pub fn sync_standalone_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        _settings: &Arc<SettingsManager>,
        target_mon: &MonitorInfo,
    ) {
        *self.active_standalone_media_id.lock() = Some(media_id);

        let Ok(Some(record)) = library.media().get(media_id) else {
            return;
        };

        let Ok(file_path) = library.resolve_media_path(media_id) else {
            return;
        };

        let is_video = matches!(record.media_type, MediaType::Video);
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);
        let file_exists = file_path.is_file();

        let ctrl = self.clone();
        let mon_clone = target_mon.clone();

        self.with_fullscreen_window(|win| {
            win.set_is_video(is_video && file_exists);

            if !file_exists {
                tracing::warn!(
                    media_id,
                    file_path = %LogObscurer::path(&file_path),
                    "[FullscreenViewer: StandaloneSync] Target file does not exist on disk, displaying missing-file fallback view"
                );
                if let Some(sub) = ctrl.standalone_subwindow.lock().as_mut() {
                    sub.hide();
                }
                if let Some(player) = ctrl.standalone_video_player.lock().as_ref() {
                    player.pause().ok();
                }
                win.set_source_image(Image::default());
                win.set_is_loading(false);
                let missing_msg =
                    format!("File not found on physical disk:\n{}", file_path.display());
                win.set_error_message(missing_msg.into());
            } else if is_video {
                win.set_is_loading(false);
                win.set_error_message("".into());
                win.set_source_image(Image::default());

                ctrl.init_or_play_standalone_video(win, media_id, &file_path, &mon_clone);
            } else {
                if let Some(sub) = ctrl.standalone_subwindow.lock().as_mut() {
                    sub.hide();
                }
                if let Some(player) = ctrl.standalone_video_player.lock().as_ref() {
                    player.set_view_active(false);
                    player.pause().ok();
                }

                let initial_placeholder = {
                    if let Some(state_arc) = self.state_weak.lock().as_ref() {
                        let st = state_arc.read();
                        st.folder_thumbnails
                            .get(&media_id)
                            .map(|buf| Image::from_rgb8(buf.clone()))
                    } else {
                        None
                    }
                };

                if let Some(img) = initial_placeholder {
                    win.set_source_image(img);
                    win.set_is_loading(false);
                } else {
                    win.set_source_image(Image::default());
                    win.set_is_loading(true);
                }
                win.set_error_message("".into());

                let win_weak_async = win.as_weak();
                let fpath_clone = file_path;
                let ctrl_async = ctrl.clone();

                rayon::spawn(move || {
                    let load_res = HighResImageLoader::load_full_resolution(&fpath_clone);
                    let _ = win_weak_async.upgrade_in_event_loop(move |w| {
                        if *ctrl_async.active_standalone_media_id.lock() != Some(media_id) {
                            return;
                        }
                        match load_res {
                            Ok((_w, _h, pixel_buf)) => {
                                w.set_source_image(Image::from_rgb8(pixel_buf));
                                w.set_is_loading(false);
                            }
                            Err(err) => {
                                w.set_error_message(err.to_string().into());
                                w.set_is_loading(false);
                            }
                        }
                    });
                });
            }
        });

        self.sync_browse_selection(media_id);
    }

    fn reparent_to_fullscreen(
        &self,
        tab_manager: &Arc<ViewerTabManager>,
        fs_win: &ImageFullscreenWindow,
        tab_id: TabId,
        target_mon: &MonitorInfo,
    ) {
        let fs_xid = self.resolve_fs_xid(fs_win);

        if fs_xid == 0 {
            tracing::debug!(
                tab_id,
                "[FullscreenViewer: Reparent] Fullscreen XID not ready yet. Scheduling fast retry..."
            );
            let ctrl = self.clone();
            let tm = tab_manager.clone();
            let win_weak = fs_win.as_weak();
            let mon_clone = target_mon.clone();
            Timer::single_shot(std::time::Duration::from_millis(15), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.reparent_to_fullscreen(&tm, &w, tab_id, &mon_clone);
                }
            });
            return;
        }

        self.apply_x11_monitor_fullscreen(fs_xid, target_mon);

        let screen_w = target_mon.width.max(640);
        let screen_h = target_mon.height.max(480);

        if let Some(session) = tab_manager.get_tab(tab_id)
            && let TabPayload::Video(ref vid) = session.payload
        {
            if !session.file_path.is_file() || !vid.error_message.is_empty() {
                tracing::warn!(
                    tab_id,
                    "[FullscreenViewer: Reparent] Video file is missing or in error state; skipping subwindow reparenting"
                );
                return;
            }

            let fs_toggle = self.clone();
            vid.player.on_toggle_fullscreen(move || {
                let fs = fs_toggle.clone();
                tracing::info!(
                    "[BridgeViewerTabs: MpvDblClick] Fullscreen exit requested via MPV event in fullscreen"
                );
                fs.close();
            });

            if let Some(mw_weak) = self.main_window_weak.lock().as_ref() {
                let win_close = mw_weak.clone();
                vid.player.on_close_tab(move || {
                    let _ = win_close.upgrade_in_event_loop(move |w| {
                        tracing::info!(
                            "[BridgeViewerTabs: MpvCloseTab] Ctrl+W intercepted from MPV video player in fullscreen. Invoking viewer_close_tab..."
                        );
                        w.invoke_viewer_close_tab();
                    });
                });

                let win_ren = mw_weak.clone();
                vid.player.on_rename_requested(move || {
                    let _ = win_ren.upgrade_in_event_loop(move |w| {
                        w.invoke_request_rename();
                    });
                });
            }

            vid.player.set_tab_grid_mode(false);
            vid.player.set_view_active(true);
            vid.player.set_user_paused(false);
            let _ = vid.player.play();

            let mut sub_guard = vid.subwindow.lock();
            if let Some(ref mut sub) = *sub_guard {
                let _ = sub.reparent(fs_xid, 0, 0, screen_w, screen_h);
                sub.show();
            }
        }
    }

    pub fn sync_active_tab(&self, tab_manager: &Arc<ViewerTabManager>, settings: &SettingsManager) {
        if !self.is_active.load(Ordering::SeqCst) {
            return;
        }

        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let Some(tab_id) = fs_tab_id else {
            return;
        };

        let Some(tab) = tab_manager.get_tab(tab_id) else {
            self.close();
            return;
        };

        let ctrl = self.clone();
        let tm_clone = tab_manager.clone();
        let target_mon = self.resolve_current_target_monitor(settings);

        let file_exists = tab.file_path.is_file();
        let is_video = matches!(tab.payload, TabPayload::Video(_));
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

        self.with_fullscreen_window(|win| {
            win.set_is_video(is_video && file_exists);
            win.set_allow_navigation(false);
            ctrl.sync_tab_content(win, &tab, settings);

            if is_video && file_exists {
                ctrl.reparent_to_fullscreen(&tm_clone, win, tab.id, &target_mon);
            } else if let Some(sub_slot) = tm_clone.get_subwindow_for_tab(tab.id)
                && let Some(ref mut sub) = *sub_slot.lock()
            {
                sub.hide();
            }
        });
    }

    /// Closes whichever fullscreen is open (single viewer or Multiview).
    pub fn close(&self) {
        self.multiview.request_close();
        self.close_single(true);
    }

    /// Closes the single fullscreen without the close handler (the Browse view and its selection stay untouched)
    /// because another fullscreen takes over.
    pub fn dismiss_for_replacement(&self) {
        self.close_single(false);
    }

    fn close_single(&self, notify: bool) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        let was_video = self.is_fullscreen_video.load(Ordering::SeqCst);
        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let is_standalone = fs_tab_id.is_none();
        let standalone_media_id = *self.active_standalone_media_id.lock();
        let origin_mode = *self.origin_view_mode.lock();
        let tm_weak = self.tab_manager_weak.lock().clone();
        let main_xid_opt = *self.cached_main_xid.lock();
        let viewport_rect = *self.cached_viewport_rect.lock();
        let fs_win_weak_opt = self.window_weak.lock().clone();
        let mw_weak_opt = self.main_window_weak.lock().clone();
        let handler_opt = self.close_handler.lock().clone();

        *self.active_fullscreen_tab_id.lock() = None;
        *self.active_standalone_media_id.lock() = None;
        *self.cached_fs_xid.lock() = None;

        if let Some(player) = self.standalone_video_player.lock().take() {
            player.stop();
        }
        if let Some(sub) = self.standalone_subwindow.lock().take() {
            drop(sub);
        }

        if was_video
            && !is_standalone
            && let Some(ref tm) = tm_weak
            && let Some(main_xid) = main_xid_opt
            && let Some(target_id) = fs_tab_id
            && let Some(session) = tm.get_tab(target_id)
            && let TabPayload::Video(ref vid) = session.payload
            && let Some(ref mut sub) = *vid.subwindow.lock()
        {
            let (x, y, w, h) = viewport_rect;
            let _ = sub.reparent(main_xid, x, y, w, h);
        }

        if let Some(fs_weak) = fs_win_weak_opt {
            let _ = fs_weak.upgrade_in_event_loop(move |fs_win| {
                fs_win.set_is_ready(false);
                let _ = fs_win.hide();
                tracing::info!(
                    origin_mode,
                    "[FullscreenViewer: Close] Fullscreen media viewer window dismissed"
                );
            });
        }

        if notify && let Some(mw_weak) = mw_weak_opt {
            let _ = mw_weak.upgrade_in_event_loop(move |mw| {
                mw.set_is_modal_locked(false);
                tracing::info!("[FullscreenViewer: Close] Main UI modal lock released");
                if let Some(handler) = handler_opt {
                    handler(origin_mode, fs_tab_id, standalone_media_id);
                }
            });
        }
    }

    /// True while the single fullscreen or the Multiview is open.
    #[inline]
    pub fn is_open(&self) -> bool {
        self.is_active.load(Ordering::SeqCst) || self.multiview.is_open()
    }

    fn sync_tab_content(
        &self,
        win: &ImageFullscreenWindow,
        session: &MediaTabSession,
        settings: &SettingsManager,
    ) {
        let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        };
        win.set_background_mode(bg_mode_idx);

        let file_exists = session.file_path.is_file();
        if !file_exists {
            win.set_is_loading(false);
            win.set_source_image(Image::default());
            let missing_msg = format!(
                "File not found on physical disk:\n{}",
                session.file_path.display()
            );
            win.set_error_message(missing_msg.into());
            return;
        }

        match session.payload {
            TabPayload::Image(ref img) => {
                win.set_is_loading(img.is_loading);
                win.set_error_message(img.error_message.clone().into());

                if let Some(ref buf) = img.high_res_pixels {
                    win.set_source_image(Image::from_rgb8(buf.clone()));
                } else {
                    win.set_source_image(Image::default());
                }
            }
            TabPayload::Video(ref vid) => {
                win.set_is_loading(vid.is_loading);
                win.set_error_message(vid.error_message.clone().into());
                win.set_source_image(Image::default());
            }
        }
    }
}
