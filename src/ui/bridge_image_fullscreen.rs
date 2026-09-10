use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::ImageFullscreenWindow;
use super::MainWindow;
use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::ui::bridge_file_selection::{DoubleClickKind, LastClickRecord};
use crate::ui::bridge_state::UiState;
use crate::viewer::{
    force_x11_fullscreen, get_slint_window_xid_with_title_fallback, HighResImageLoader,
    MediaTabSession, MpvPlayer, TabId, TabPayload, ViewerTabManager, X11VideoSubwindow,
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

pub type FullscreenCloseCallback = Arc<dyn Fn(i32 /* origin_mode */, Option<TabId>) + Send + Sync>;

/// Specialized controller managing the frameless Fullscreen Media Viewer window lifecycle with seamless subwindow reparenting and quick fullscreen mode.
#[derive(Clone, Default)]
pub struct ImageFullscreenDialogController {
    window_weak: Arc<Mutex<Option<slint::Weak<ImageFullscreenWindow>>>>,
    main_window_weak: Arc<Mutex<Option<slint::Weak<MainWindow>>>>,
    tab_manager_weak: Arc<Mutex<Option<Arc<ViewerTabManager>>>>,
    library_weak: Arc<Mutex<Option<Arc<Library>>>>,
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
}

impl ImageFullscreenDialogController {
    pub fn new() -> Self {
        Self {
            window_weak: Arc::new(Mutex::new(None)),
            main_window_weak: Arc::new(Mutex::new(None)),
            tab_manager_weak: Arc::new(Mutex::new(None)),
            library_weak: Arc::new(Mutex::new(None)),
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
            close_handler: Arc::new(Mutex::new(None)),
        }
    }

    /// Registers a closure executed when the fullscreen window closes, passing the originating view mode.
    pub fn set_close_handler<F>(&self, handler: F)
    where
        F: Fn(i32, Option<TabId>) + Send + Sync + 'static,
    {
        *self.close_handler.lock() = Some(Arc::new(handler));
    }

    /// Stores the main window XID and latest tab viewport rectangle for restoring on fullscreen exit.
    pub fn set_main_viewport_info(&self, main_xid: u64, x: i32, y: i32, w: u32, h: u32) {
        *self.cached_main_xid.lock() = Some(main_xid);
        *self.cached_viewport_rect.lock() = (x, y, w, h);
    }

    /// Evaluates if successive clicks qualify as a double-click (< 400ms interval), distinguishing between normal and Ctrl-modified double clicks.
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

    /// Returns the cached fullscreen XID or resolves it once directly from the Slint window.
    fn resolve_fs_xid(&self, fs_win: &ImageFullscreenWindow) -> u64 {
        if let Some(cached) = *self.cached_fs_xid.lock()
            && cached > 0
        {
            return cached;
        }

        let main_xid = *self.cached_main_xid.lock();
        if let Some(xid) = get_slint_window_xid_with_title_fallback(fs_win.window(), "RustView - Fullscreen")
            && Some(xid) != main_xid
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

                let ctrl_click = ctrl.clone();
                win.on_canvas_clicked(move |is_ctrl| {
                    match ctrl_click.check_double_click(is_ctrl) {
                        DoubleClickKind::Normal => {
                            ctrl_click.close();
                        }
                        DoubleClickKind::Ctrl => {
                            ctrl_click.handle_ctrl_double_click();
                        }
                        DoubleClickKind::None => {}
                    }
                });

                let ctrl_next = ctrl.clone();
                win.on_next_image(move || {
                    if ctrl_next.active_fullscreen_tab_id.lock().is_some() {
                        if let Some(ref mw_weak) = *ctrl_next.main_window_weak.lock()
                            && let Some(mw) = mw_weak.upgrade()
                        {
                            mw.invoke_viewer_next();
                        }
                    } else {
                        ctrl_next.navigate_standalone_adjacent(true);
                    }
                });

                let ctrl_prev = ctrl.clone();
                win.on_previous_image(move || {
                    if ctrl_prev.active_fullscreen_tab_id.lock().is_some() {
                        if let Some(ref mw_weak) = *ctrl_prev.main_window_weak.lock()
                            && let Some(mw) = mw_weak.upgrade()
                        {
                            mw.invoke_viewer_previous();
                        }
                    } else {
                        ctrl_prev.navigate_standalone_adjacent(false);
                    }
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
        settings: &SettingsManager,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let Some(active_tab) = tab_manager.get_active_tab() else {
            tracing::warn!("[FullscreenViewer: Show] Cannot show fullscreen: no active viewer tab session");
            return;
        };

        self.show_for_tab(active_tab.id, tab_manager, settings, main_window_weak);
    }

    /// Displays the frameless fullscreen media window for any specific tab without changing active tab state (auto-plays).
    pub fn show_for_tab(
        &self,
        tab_id: TabId,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &SettingsManager,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let Some(target_tab) = tab_manager.get_tab(tab_id) else {
            tracing::warn!(tab_id, "[FullscreenViewer: ShowForTab] Tab session not found");
            return;
        };

        let t0 = Instant::now();
        *self.main_window_weak.lock() = Some(main_window_weak.clone());
        *self.tab_manager_weak.lock() = Some(tab_manager.clone());
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

        // Lock the entire Main UI to prevent user interaction while in fullscreen (Tab View mode)
        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(true);
        }

        let ctrl = self.clone();
        let tm_clone = tab_manager.clone();

        self.with_fullscreen_window(|win| {
            ctrl.sync_tab_content(win, &target_tab, settings);
            win.set_is_video(is_video && file_exists);

            if !file_exists {
                win.set_source_image(Image::default());
                win.set_is_loading(false);
                let missing_msg = format!("File not found on physical disk:\n{}", target_tab.file_path.display());
                win.set_error_message(missing_msg.into());
            }

            // Re-assert fullscreen state on Winit to guarantee proper mapping
            win.window().set_fullscreen(false);
            win.window().set_fullscreen(true);
            let _ = win.show();

            // Enforce true EWMH Fullscreen directly on X11 level (removes titlebar and covers taskbar)
            let fs_xid = ctrl.resolve_fs_xid(win);
            if fs_xid > 0 {
                force_x11_fullscreen(fs_xid);
            }

            ctrl.is_active.store(true, Ordering::SeqCst);

            if is_video && file_exists {
                ctrl.reparent_to_fullscreen(&tm_clone, win, tab_id);
            }
        });

        tracing::info!(
            tab_id = target_tab.id,
            media_id = target_tab.media_id,
            is_video,
            file_exists,
            origin_mode,
            filename = %LogObscurer::filename(&target_tab.filename),
            elapsed_us = t0.elapsed().as_micros(),
            "[FullscreenViewer: ShowForTab] Fullscreen media viewer activated with true X11 fullscreen mapping"
        );
    }

    /// Displays the frameless fullscreen viewer directly for a single media item without creating or modifying tabs (Quick-Fullscreen Mode).
    /// Leaves the Main Window unblocked (non-modal) to allow live switching upon clicking other media items.
    pub fn show_for_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
        tab_manager: &Arc<ViewerTabManager>,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let t0 = Instant::now();
        let record_res = library.media().get(media_id);
        let Some(record) = record_res.ok().flatten() else {
            tracing::warn!(media_id, "[FullscreenViewer: ShowForMedia] Media record not found in database");
            return;
        };

        let Ok(file_path) = library.resolve_media_path(media_id) else {
            tracing::warn!(media_id, "[FullscreenViewer: ShowForMedia] Failed to resolve media file path");
            return;
        };

        let file_exists = file_path.is_file();

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

        let is_video = matches!(record.media_type, MediaType::Video);
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

        // Quick-Fullscreen mode from Browse view does NOT modal-lock the Main Window!
        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(false);
        }

        let bg_mode_idx = match settings.get().viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        };

        let ctrl = self.clone();

        self.with_fullscreen_window(|win| {
            win.set_background_mode(bg_mode_idx);
            win.set_is_video(is_video && file_exists);

            // Clean up standalone player / subwindow if running
            if let Some(player) = ctrl.standalone_video_player.lock().take() {
                player.stop();
            }
            if let Some(sub) = ctrl.standalone_subwindow.lock().take() {
                drop(sub);
            }

            if !file_exists {
                tracing::warn!(
                    media_id,
                    file_path = %LogObscurer::path(&file_path),
                    "[FullscreenViewer: ShowForMedia] File does not exist on disk, displaying missing-file fallback view"
                );
                win.set_source_image(Image::default());
                win.set_is_loading(false);
                let missing_msg = format!("File not found on physical disk:\n{}", file_path.display());
                win.set_error_message(missing_msg.into());
            } else if !is_video {
                // Immediate placeholder image from cache so the screen is immediately populated in 0ms
                let initial_placeholder = {
                    let st = state.read();
                    st.folder_thumbnails
                        .get(&media_id)
                        .map(|buf| Image::from_rgb8(buf.clone()))
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
                let fpath_clone = file_path.clone();
                let ctrl_async = ctrl.clone();
                let cur_media_id = media_id;

                rayon::spawn(move || {
                    let load_res = HighResImageLoader::load_full_resolution(&fpath_clone);
                    let _ = win_weak_async.upgrade_in_event_loop(move |w| {
                        if *ctrl_async.active_standalone_media_id.lock() != Some(cur_media_id) {
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
            } else {
                win.set_is_loading(false);
                win.set_error_message("".into());
                win.set_source_image(Image::default());
            }

            win.window().set_fullscreen(false);
            win.window().set_fullscreen(true);
            let _ = win.show();

            let fs_xid = ctrl.resolve_fs_xid(win);
            if fs_xid > 0 {
                force_x11_fullscreen(fs_xid);
            }

            ctrl.is_active.store(true, Ordering::SeqCst);

            if is_video && file_exists {
                ctrl.init_or_play_standalone_video(win, media_id, &file_path);
            }
        });

        tracing::info!(
            media_id,
            is_video,
            file_exists,
            origin_mode,
            filename = %LogObscurer::filename(&record.filename),
            elapsed_us = t0.elapsed().as_micros(),
            "[FullscreenViewer: ShowForMedia] Fullscreen viewer activated (non-modal, file_exists verified)"
        );
    }

    /// Returns true if Quick-Fullscreen mode is currently active (opened from Browse view on a standalone media item).
    #[inline]
    pub fn is_quick_fullscreen_active(&self) -> bool {
        self.is_active.load(Ordering::SeqCst) && self.active_standalone_media_id.lock().is_some()
    }

    /// Dismisses / closes Quick-Fullscreen if currently active (triggered when interacting with non-media elements).
    pub fn dismiss_if_quick_fullscreen(&self) {
        if self.is_quick_fullscreen_active() {
            tracing::info!(
                "[FullscreenViewer: DismissQuick] Dismissing Quick-Fullscreen due to non-media interaction on Main UI"
            );
            self.close();
        }
    }

    /// Updates the Quick-Fullscreen viewer dynamically to display a newly selected media item.
    pub fn update_quick_fullscreen_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
    ) {
        if self.is_quick_fullscreen_active() {
            tracing::info!(
                media_id,
                "[FullscreenViewer: UpdateQuick] Quick-Fullscreen active; dynamically switching fullscreen view to newly clicked media item"
            );
            self.sync_standalone_media(media_id, library, settings);
        }
    }

    /// Handles Ctrl + Double-Click in Fullscreen: Converts the current media item into a background tab
    /// ONLY if it was opened standalone from the Browse View, preserving video playback and closing fullscreen.
    pub fn handle_ctrl_double_click(&self) {
        let origin_mode = *self.origin_view_mode.lock();
        let standalone_id_opt = *self.active_standalone_media_id.lock();

        // ONLY convert if opened directly from Browse view (origin_mode == 0) in standalone mode!
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
            (tab_mgr_opt, lib_opt, set_opt, st_opt, mw_opt) else {
                tracing::warn!("[FullscreenViewer: CtrlDblClick] Missing subsystem reference for background tab conversion");
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

    /// Initializes or reuses MPV and X11 subwindow directly on the fullscreen window before video load to guarantee 0ms flicker.
    fn init_or_play_standalone_video(
        &self,
        fs_win: &ImageFullscreenWindow,
        media_id: MediaId,
        file_path: &Path,
    ) {
        if !file_path.is_file() {
            tracing::warn!(
                media_id,
                file_path = %LogObscurer::path(file_path),
                "[FullscreenViewer: StandaloneVideo] Target video file does not exist on disk, skipping subwindow creation"
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
            Timer::single_shot(std::time::Duration::from_millis(15), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.init_or_play_standalone_video(&w, media_id, &fpath);
                }
            });
            return;
        }

        *self.cached_fs_xid.lock() = Some(fs_xid);
        force_x11_fullscreen(fs_xid);

        let fs_size = fs_win.window().size();
        let screen_w = fs_size.width.max(640);
        let screen_h = fs_size.height.max(480);

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
                        tracing::error!(error = %e, "[FullscreenViewer: Subwindow] Failed to create X11 subwindow");
                        return;
                    }
                };
                let wid = sub.xid();
                sub.show();
                *sub_opt = Some(sub);

                let p = match MpvPlayer::new() {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        tracing::error!(error = %e, "[FullscreenViewer: MpvInit] Failed to create MpvPlayer");
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

                let fs_convert = self.clone();
                p.on_convert_to_background_tab(move || {
                    if fs_convert.check_double_click(true) == DoubleClickKind::Ctrl {
                        tracing::info!("[FullscreenViewer: MpvCtrlDblClick] Ctrl+DoubleClick intercepted from MPV video player in fullscreen");
                        fs_convert.handle_ctrl_double_click();
                    }
                });

                // Attach WID BEFORE loading video to prevent mid-playback VO reload!
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

        let ctrl_sync = self.clone();
        let win_weak_sync = fs_win.as_weak();
        let fs_xid_sync = fs_xid;
        Timer::single_shot(std::time::Duration::from_millis(25), move || {
            if let Some(w) = win_weak_sync.upgrade() {
                let size = w.window().size();
                let mut sub_g = ctrl_sync.standalone_subwindow.lock();
                if let Some(ref mut sub) = *sub_g
                    && sub.parent_xid() == fs_xid_sync
                {
                    sub.move_resize(0, 0, size.width.max(640), size.height.max(480));
                    sub.show();
                }
            }
        });
    }

    /// Navigates adjacent items in standalone Quick-Fullscreen mode.
    pub fn navigate_standalone_adjacent(&self, is_next: bool) {
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
            let current_pos = if !st.active_indices.is_empty() {
                st.active_indices.iter().position(|&idx| {
                    st.all_folder_media.get(idx).map(|m| m.record.id == current_id).unwrap_or(false)
                })
            } else {
                st.all_folder_media.iter().position(|m| m.record.id == current_id)
            };

            if let Some(pos) = current_pos {
                let target_pos = if is_next {
                    pos.checked_add(1)
                } else {
                    pos.checked_sub(1)
                };

                target_pos.and_then(|t_idx| {
                    if !st.active_indices.is_empty() {
                        st.active_indices.get(t_idx).and_then(|&idx| st.all_folder_media.get(idx)).map(|m| m.record.id)
                    } else {
                        st.all_folder_media.get(t_idx).map(|m| m.record.id)
                    }
                })
            } else {
                None
            }
        };

        if let Some(target_id) = target_media_id_opt {
            tracing::info!(
                current_id,
                target_id,
                is_next,
                "[FullscreenViewer: StandaloneNavigate] Navigating to adjacent media in standalone fullscreen"
            );
            self.sync_standalone_media(target_id, &library_arc, &settings_arc);
        }
    }

    /// Synchronizes the standalone fullscreen viewer canvas with the specified media item.
    pub fn sync_standalone_media(
        &self,
        media_id: MediaId,
        library: &Arc<Library>,
        _settings: &Arc<SettingsManager>,
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

        self.with_fullscreen_window(|win| {
            win.set_is_video(is_video && file_exists);

            if let Some(player) = ctrl.standalone_video_player.lock().take() {
                player.stop();
            }
            if let Some(sub) = ctrl.standalone_subwindow.lock().take() {
                drop(sub);
            }

            if !file_exists {
                tracing::warn!(
                    media_id,
                    file_path = %LogObscurer::path(&file_path),
                    "[FullscreenViewer: StandaloneSync] Target file does not exist on disk, displaying missing-file fallback view"
                );
                win.set_source_image(Image::default());
                win.set_is_loading(false);
                let missing_msg = format!("File not found on physical disk:\n{}", file_path.display());
                win.set_error_message(missing_msg.into());
            } else if !is_video {
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
            } else {
                win.set_is_loading(false);
                win.set_error_message("".into());
                win.set_source_image(Image::default());

                ctrl.init_or_play_standalone_video(win, media_id, &file_path);
            }
        });
    }

    /// Reparents the running X11 video subwindow into the fullscreen window without interrupting playback and starts playback.
    fn reparent_to_fullscreen(
        &self,
        tab_manager: &Arc<ViewerTabManager>,
        fs_win: &ImageFullscreenWindow,
        tab_id: TabId,
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
            Timer::single_shot(std::time::Duration::from_millis(15), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.reparent_to_fullscreen(&tm, &w, tab_id);
                }
            });
            return;
        }

        *self.cached_fs_xid.lock() = Some(fs_xid);
        force_x11_fullscreen(fs_xid);

        let fs_size = fs_win.window().size();
        let screen_w = fs_size.width.max(640);
        let screen_h = fs_size.height.max(480);

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
                tracing::info!("[BridgeViewerTabs: MpvDblClick] Fullscreen exit requested via MPV event in fullscreen");
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

        let tm_sync = tab_manager.clone();
        let win_weak_sync = fs_win.as_weak();
        let fs_xid_sync = fs_xid;
        Timer::single_shot(std::time::Duration::from_millis(25), move || {
            if let Some(w) = win_weak_sync.upgrade() {
                let size = w.window().size();
                if let Some(session) = tm_sync.get_tab(tab_id)
                    && let TabPayload::Video(ref vid) = session.payload
                    && let Some(ref mut sub) = *vid.subwindow.lock()
                    && sub.parent_xid() == fs_xid_sync
                {
                    sub.move_resize(0, 0, size.width.max(640), size.height.max(480));
                    sub.show();
                }
            }
        });
    }

    /// Synchronizes active tab payload into the fullscreen window if currently visible.
    pub fn sync_active_tab(&self, tab_manager: &Arc<ViewerTabManager>, settings: &SettingsManager) {
        if !self.is_open() {
            return;
        }

        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let target_tab = fs_tab_id.and_then(|id| tab_manager.get_tab(id)).or_else(|| tab_manager.get_active_tab());

        let ctrl = self.clone();
        let tm_clone = tab_manager.clone();

        if let Some(tab) = target_tab {
            let file_exists = tab.file_path.is_file();
            let is_video = matches!(tab.payload, TabPayload::Video(_));
            self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

            self.with_fullscreen_window(|win| {
                win.set_is_video(is_video && file_exists);
                ctrl.sync_tab_content(win, &tab, settings);

                if is_video && file_exists {
                    ctrl.reparent_to_fullscreen(&tm_clone, win, tab.id);
                } else if let Some(sub_slot) = tm_clone.get_subwindow_for_tab(tab.id)
                    && let Some(ref mut sub) = *sub_slot.lock()
                {
                    sub.hide();
                }
            });
        } else if self.active_standalone_media_id.lock().is_none() {
            self.close();
        }
    }

    /// Hides and dismisses the fullscreen media window, restores video subwindow to originating layout, and safely unlocks the Main UI.
    pub fn close(&self) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        let was_video = self.is_fullscreen_video.load(Ordering::SeqCst);
        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let is_standalone = fs_tab_id.is_none();
        let origin_mode = *self.origin_view_mode.lock();
        let tm_weak = self.tab_manager_weak.lock().clone();
        let main_xid_opt = *self.cached_main_xid.lock();
        let viewport_rect = *self.cached_viewport_rect.lock();
        let fs_win_weak_opt = self.window_weak.lock().clone();
        let mw_weak_opt = self.main_window_weak.lock().clone();
        let handler_opt = self.close_handler.lock().clone();

        *self.active_fullscreen_tab_id.lock() = None;
        *self.active_standalone_media_id.lock() = None;

        // Stop and clean up standalone player if active
        if let Some(player) = self.standalone_video_player.lock().take() {
            player.stop();
        }
        if let Some(sub) = self.standalone_subwindow.lock().take() {
            drop(sub);
        }

        // Reparent video subwindow back to the Main UI window if it was a tab video
        if was_video && !is_standalone
            && let Some(ref tm) = tm_weak
            && let Some(main_xid) = main_xid_opt
            && let Some(target_id) = fs_tab_id
        {
            let (x, y, w, h) = viewport_rect;
            if let Some(session) = tm.get_tab(target_id)
                && let TabPayload::Video(ref vid) = session.payload
                && let Some(ref mut sub) = *vid.subwindow.lock()
            {
                let _ = sub.reparent(main_xid, x, y, w, h);
            }
        }

        // Hide Fullscreen Slint Window on the Main UI thread
        if let Some(fs_weak) = fs_win_weak_opt {
            let _ = fs_weak.upgrade_in_event_loop(move |fs_win| {
                let _ = fs_win.hide();
                tracing::info!(origin_mode, "[FullscreenViewer: Close] Fullscreen media viewer window dismissed");
            });
        }

        // Safely unlock the Main UI and invoke the restore handler on the Main UI thread
        if let Some(mw_weak) = mw_weak_opt {
            let _ = mw_weak.upgrade_in_event_loop(move |mw| {
                mw.set_is_modal_locked(false);
                tracing::info!("[FullscreenViewer: Close] Main UI modal lock released");
                if let Some(handler) = handler_opt {
                    handler(origin_mode, fs_tab_id);
                }
            });
        }
    }

    /// Returns whether a fullscreen window is currently active.
    #[inline]
    pub fn is_open(&self) -> bool {
        self.is_active.load(Ordering::SeqCst)
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
            let missing_msg = format!("File not found on physical disk:\n{}", session.file_path.display());
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
