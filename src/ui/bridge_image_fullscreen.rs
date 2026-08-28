use super::ImageFullscreenWindow;
use super::MainWindow;
use crate::settings::{LogObscurer, SettingsManager};
use crate::viewer::{
    find_window_by_title, get_slint_window_xid_with_title_fallback, MediaTabSession, TabId,
    TabPayload, ViewerTabManager,
};
use parking_lot::Mutex;
use slint::{CloseRequestResponse, ComponentHandle, Image, Timer};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

thread_local! {
    static FULLSCREEN_WINDOW: RefCell<Option<ImageFullscreenWindow>> = const { RefCell::new(None) };
}

pub type FullscreenCloseCallback = Arc<dyn Fn(i32 /* origin_mode */, Option<TabId>) + Send + Sync>;

/// Specialized controller managing the frameless Fullscreen Media Viewer window lifecycle with seamless subwindow reparenting.
#[derive(Clone, Default)]
pub struct ImageFullscreenDialogController {
    window_weak: Arc<Mutex<Option<slint::Weak<ImageFullscreenWindow>>>>,
    main_window_weak: Arc<Mutex<Option<slint::Weak<MainWindow>>>>,
    tab_manager_weak: Arc<Mutex<Option<Arc<ViewerTabManager>>>>,
    last_click_instant: Arc<Mutex<Option<Instant>>>,
    is_active: Arc<AtomicBool>,
    is_fullscreen_video: Arc<AtomicBool>,
    active_fullscreen_tab_id: Arc<Mutex<Option<TabId>>>,
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
            last_click_instant: Arc::new(Mutex::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
            is_fullscreen_video: Arc::new(AtomicBool::new(false)),
            active_fullscreen_tab_id: Arc::new(Mutex::new(None)),
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

    /// Evaluates if a click qualifies as a double-click (< 400ms interval).
    pub fn check_double_click(&self) -> bool {
        let now = Instant::now();
        let mut guard = self.last_click_instant.lock();
        if let Some(prev) = *guard
            && now.duration_since(prev) < Duration::from_millis(400)
        {
            *guard = None;
            return true;
        }
        *guard = Some(now);
        false
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

    /// Displays the frameless fullscreen media window for any specific tab without changing active tab state (Requirement 3: auto-plays).
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

        let origin_mode = main_window_weak
            .upgrade()
            .map(|mw| mw.get_active_view_mode())
            .unwrap_or(1);
        *self.origin_view_mode.lock() = origin_mode;

        let is_video = matches!(target_tab.payload, TabPayload::Video(_));
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

        // Lock the entire Main UI to prevent user interaction while in fullscreen
        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(true);
        }

        let ctrl_clone = self.clone();
        let tm_clone = tab_manager.clone();

        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();

            // Always discard any previous window instance so X11 maps a 100% fresh, real fullscreen window every single time
            if let Some(old_win) = cell.take() {
                let _ = old_win.hide();
            }

            let win = ImageFullscreenWindow::new()
                .expect("Failed to create ImageFullscreenWindow Slint component");

            let ctrl_close = ctrl_clone.clone();
            win.on_request_close(move || {
                ctrl_close.close();
            });

            let ctrl_double = ctrl_clone.clone();
            win.on_canvas_double_clicked(move || {
                if ctrl_double.check_double_click() {
                    ctrl_double.close();
                }
            });

            let mw_next = main_window_weak.clone();
            win.on_next_image(move || {
                if let Some(mw) = mw_next.upgrade() {
                    mw.invoke_viewer_next();
                }
            });

            let mw_prev = main_window_weak;
            win.on_previous_image(move || {
                if let Some(mw) = mw_prev.upgrade() {
                    mw.invoke_viewer_previous();
                }
            });

            let ctrl_sys_close = ctrl_clone.clone();
            win.window().on_close_requested(move || {
                ctrl_sys_close.close();
                CloseRequestResponse::HideWindow
            });

            ctrl_clone.sync_tab_content(&win, &target_tab, settings);
            win.set_is_video(is_video);
            win.window().set_fullscreen(true);
            let _ = win.show();

            *ctrl_clone.window_weak.lock() = Some(win.as_weak());
            *cell = Some(win);
            ctrl_clone.is_active.store(true, Ordering::SeqCst);

            if is_video
                && let Some(ref fs_win) = *cell
            {
                ctrl_clone.reparent_to_fullscreen(&tm_clone, fs_win, tab_id);
            }

            tracing::info!(
                tab_id = target_tab.id,
                media_id = target_tab.media_id,
                is_video,
                origin_mode,
                filename = %LogObscurer::filename(&target_tab.filename),
                elapsed_us = t0.elapsed().as_micros(),
                "[FullscreenViewer: ShowForTab] Fullscreen media viewer activated with true X11 fullscreen mapping"
            );
        });
    }

    /// Reparents the running X11 video subwindow into the fullscreen window without interrupting playback and starts playback (Requirement 3).
    fn reparent_to_fullscreen(
        &self,
        tab_manager: &Arc<ViewerTabManager>,
        fs_win: &ImageFullscreenWindow,
        tab_id: TabId,
    ) {
        let main_xid = *self.cached_main_xid.lock();

        let fs_xid = match get_slint_window_xid_with_title_fallback(fs_win.window(), "RustView - Fullscreen") {
            Some(xid) if Some(xid) != main_xid => xid,
            _ => find_window_by_title("RustView - Fullscreen").unwrap_or_default(),
        };

        if fs_xid == 0 {
            tracing::debug!(
                tab_id,
                "[FullscreenViewer: Reparent] Fullscreen XID not ready yet. Scheduling fast retry..."
            );
            let ctrl = self.clone();
            let tm = tab_manager.clone();
            let win_weak = fs_win.as_weak();
            Timer::single_shot(std::time::Duration::from_millis(20), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.reparent_to_fullscreen(&tm, &w, tab_id);
                }
            });
            return;
        }

        *self.cached_fs_xid.lock() = Some(fs_xid);

        let fs_size = fs_win.window().size();
        let screen_w = fs_size.width.max(640);
        let screen_h = fs_size.height.max(480);

        if let Some(session) = tab_manager.get_tab(tab_id)
            && let TabPayload::Video(ref vid) = session.payload
        {
            // Wire player callbacks immediately to guarantee double-click, 'f', and ESC exit fullscreen from MPV
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

            // Requirement 3: Always enable controls, set active, and start playback in fullscreen
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
        Timer::single_shot(std::time::Duration::from_millis(35), move || {
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

        let win_opt = self.window_weak.lock().as_ref().and_then(|w| w.upgrade());
        let Some(win) = win_opt else {
            return;
        };

        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let target_tab = fs_tab_id.and_then(|id| tab_manager.get_tab(id)).or_else(|| tab_manager.get_active_tab());

        if let Some(tab) = target_tab {
            let is_video = matches!(tab.payload, TabPayload::Video(_));
            self.is_fullscreen_video.store(is_video, Ordering::SeqCst);
            win.set_is_video(is_video);
            self.sync_tab_content(&win, &tab, settings);

            if is_video {
                self.reparent_to_fullscreen(tab_manager, &win, tab.id);
            } else if let Some(sub_slot) = tab_manager.get_subwindow_for_tab(tab.id)
                && let Some(ref mut sub) = *sub_slot.lock()
            {
                sub.hide();
            }
        } else {
            self.close();
        }
    }

    /// Hides and dismisses the fullscreen media window, restores video subwindow to originating layout, and safely unlocks the Main UI.
    /// Thread-safe: can be invoked from any thread (including mpv-event-observer).
    pub fn close(&self) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        let was_video = self.is_fullscreen_video.load(Ordering::SeqCst);
        let fs_tab_id = *self.active_fullscreen_tab_id.lock();
        let origin_mode = *self.origin_view_mode.lock();
        let tm_weak = self.tab_manager_weak.lock().clone();
        let main_xid_opt = *self.cached_main_xid.lock();
        let viewport_rect = *self.cached_viewport_rect.lock();
        let fs_win_weak_opt = self.window_weak.lock().clone();
        let mw_weak_opt = self.main_window_weak.lock().clone();
        let handler_opt = self.close_handler.lock().clone();

        *self.window_weak.lock() = None;
        *self.cached_fs_xid.lock() = None;
        *self.active_fullscreen_tab_id.lock() = None;

        // Reparent video subwindow back to the Main UI window if it was a video
        if was_video
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

        // Clean up thread-local instance on the main thread
        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();
            if let Some(win) = cell.take() {
                let _ = win.hide();
            }
        });

        // Safely unlock the Main UI and invoke the restore handler on the Main UI thread
        if let Some(mw_weak) = mw_weak_opt {
            let _ = mw_weak.upgrade_in_event_loop(move |mw| {
                mw.set_is_modal_locked(false);
                FULLSCREEN_WINDOW.with(|win_cell| {
                    if let Some(win) = win_cell.borrow_mut().take() {
                        let _ = win.hide();
                    }
                });
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
