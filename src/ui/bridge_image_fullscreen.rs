use super::ImageFullscreenWindow;
use super::MainWindow;
use crate::settings::{LogObscurer, SettingsManager};
use crate::viewer::{
    find_window_by_title, get_slint_window_xid, MediaTabSession, TabPayload, ViewerTabManager,
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

/// Specialized controller managing the frameless Fullscreen Media Viewer window lifecycle with seamless subwindow reparenting.
#[derive(Clone, Default)]
pub struct ImageFullscreenDialogController {
    window_weak: Arc<Mutex<Option<slint::Weak<ImageFullscreenWindow>>>>,
    main_window_weak: Arc<Mutex<Option<slint::Weak<MainWindow>>>>,
    tab_manager_weak: Arc<Mutex<Option<Arc<ViewerTabManager>>>>,
    last_click_instant: Arc<Mutex<Option<Instant>>>,
    is_active: Arc<AtomicBool>,
    is_fullscreen_video: Arc<AtomicBool>,
    cached_main_xid: Arc<Mutex<Option<u64>>>,
    cached_fs_xid: Arc<Mutex<Option<u64>>>,
    cached_viewport_rect: Arc<Mutex<(i32, i32, u32, u32)>>,
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
            cached_main_xid: Arc::new(Mutex::new(None)),
            cached_fs_xid: Arc::new(Mutex::new(None)),
            cached_viewport_rect: Arc::new(Mutex::new((0, 0, 800, 600))),
        }
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

    /// Displays the frameless fullscreen media window with seamless subwindow reparenting for videos and Main UI input locking.
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

        let t0 = Instant::now();
        *self.main_window_weak.lock() = Some(main_window_weak.clone());
        *self.tab_manager_weak.lock() = Some(tab_manager.clone());

        let is_video = matches!(active_tab.payload, TabPayload::Video(_));
        self.is_fullscreen_video.store(is_video, Ordering::SeqCst);

        // Lock the entire Main UI to prevent user interaction while in fullscreen
        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(true);
        }

        let ctrl_clone = self.clone();
        let tm_clone = tab_manager.clone();

        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();

            // Strict Singleton: If a fullscreen window is already open, sync and raise it
            if let Some(ref win) = *cell {
                ctrl_clone.sync_tab_content(win, &active_tab, settings);
                win.set_is_video(is_video);
                win.window().set_fullscreen(true);
                let _ = win.show();
                ctrl_clone.is_active.store(true, Ordering::SeqCst);

                if is_video {
                    ctrl_clone.reparent_to_fullscreen(&tm_clone, win);
                }
                return;
            }

            // Instantiate single clean fullscreen window
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

            ctrl_clone.sync_tab_content(&win, &active_tab, settings);
            win.set_is_video(is_video);
            win.window().set_fullscreen(true);
            let _ = win.show();

            *ctrl_clone.window_weak.lock() = Some(win.as_weak());
            *cell = Some(win);
            ctrl_clone.is_active.store(true, Ordering::SeqCst);

            if is_video
                && let Some(ref fs_win) = *cell
            {
                ctrl_clone.reparent_to_fullscreen(&tm_clone, fs_win);
            }

            tracing::info!(
                media_id = active_tab.media_id,
                is_video,
                filename = %LogObscurer::filename(&active_tab.filename),
                elapsed_us = t0.elapsed().as_micros(),
                "[FullscreenViewer: Show] Fullscreen media viewer activated with Main UI locking"
            );
        });
    }

    /// Reparents the running X11 video subwindow into the fullscreen window without interrupting playback.
    fn reparent_to_fullscreen(&self, tab_manager: &Arc<ViewerTabManager>, fs_win: &ImageFullscreenWindow) {
        let main_xid = *self.cached_main_xid.lock();

        let fs_xid = match get_slint_window_xid(fs_win.window()) {
            Some(xid) if Some(xid) != main_xid => xid,
            _ => find_window_by_title("RustView - Fullscreen").unwrap_or_default(),
        };

        if fs_xid == 0 {
            tracing::warn!("[FullscreenViewer: Reparent] Fullscreen XID not ready yet. Scheduling fast retry...");
            let ctrl = self.clone();
            let tm = tab_manager.clone();
            let win_weak = fs_win.as_weak();
            Timer::single_shot(std::time::Duration::from_millis(20), move || {
                if let Some(w) = win_weak.upgrade() {
                    ctrl.reparent_to_fullscreen(&tm, &w);
                }
            });
            return;
        }

        *self.cached_fs_xid.lock() = Some(fs_xid);

        let fs_size = fs_win.window().size();
        let screen_w = fs_size.width.max(640);
        let screen_h = fs_size.height.max(480);

        if let Some(sub_slot) = tab_manager.get_active_subwindow()
            && let Some(ref mut sub) = *sub_slot.lock()
        {
            let _ = sub.reparent(fs_xid, 0, 0, screen_w, screen_h);
            sub.show();
        }

        // Schedule synchronization tick once the window manager finishes full-display mapping
        let tm_sync = tab_manager.clone();
        let win_weak_sync = fs_win.as_weak();
        let fs_xid_sync = fs_xid;
        Timer::single_shot(std::time::Duration::from_millis(35), move || {
            if let Some(w) = win_weak_sync.upgrade() {
                let size = w.window().size();
                if let Some(sub_slot) = tm_sync.get_active_subwindow()
                    && let Some(ref mut sub) = *sub_slot.lock()
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

        if let Some(active_tab) = tab_manager.get_active_tab() {
            let is_video = matches!(active_tab.payload, TabPayload::Video(_));
            self.is_fullscreen_video.store(is_video, Ordering::SeqCst);
            win.set_is_video(is_video);
            self.sync_tab_content(&win, &active_tab, settings);

            if is_video {
                self.reparent_to_fullscreen(tab_manager, &win);
            } else if let Some(sub_slot) = tab_manager.get_active_subwindow()
                && let Some(ref mut sub) = *sub_slot.lock()
            {
                sub.hide();
            }
        } else {
            self.close();
        }
    }

    /// Hides and dismisses the fullscreen media window, restores video subwindow to tab viewport, and safely unlocks the Main UI.
    pub fn close(&self) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        let was_video = self.is_fullscreen_video.load(Ordering::SeqCst);

        // Reparent video subwindow back to the Main UI viewport
        if was_video
            && let Some(tm) = self.tab_manager_weak.lock().as_ref()
            && let Some(main_xid) = *self.cached_main_xid.lock()
        {
            let (x, y, w, h) = *self.cached_viewport_rect.lock();
            if let Some(sub_slot) = tm.get_active_subwindow()
                && let Some(ref mut sub) = *sub_slot.lock()
            {
                let _ = sub.reparent(main_xid, x, y, w, h);
                sub.show();
            }
        }

        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();
            if let Some(win) = cell.take() {
                let _ = win.hide();
                tracing::info!("[FullscreenViewer: Close] Fullscreen media viewer window dismissed");
            }
        });

        *self.window_weak.lock() = None;
        *self.cached_fs_xid.lock() = None;

        // Safely unlock the Main UI
        if let Some(mw_weak) = self.main_window_weak.lock().as_ref()
            && let Some(mw) = mw_weak.upgrade()
        {
            mw.set_is_modal_locked(false);
            tracing::info!("[FullscreenViewer: Close] Main UI modal lock released");
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
