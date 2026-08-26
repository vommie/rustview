use super::ImageFullscreenWindow;
use super::MainWindow;
use crate::settings::{LogObscurer, SettingsManager};
use crate::viewer::{MediaTabSession, TabPayload, ViewerTabManager};
use parking_lot::Mutex;
use slint::{CloseRequestResponse, ComponentHandle, Image};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

thread_local! {
    static FULLSCREEN_WINDOW: RefCell<Option<ImageFullscreenWindow>> = const { RefCell::new(None) };
}

/// Specialized controller managing the frameless Fullscreen Image Viewer window lifecycle with strict singleton enforcement.
#[derive(Clone, Default)]
pub struct ImageFullscreenDialogController {
    window_weak: Arc<Mutex<Option<slint::Weak<ImageFullscreenWindow>>>>,
    main_window_weak: Arc<Mutex<Option<slint::Weak<MainWindow>>>>,
    last_click_instant: Arc<Mutex<Option<Instant>>>,
    is_active: Arc<AtomicBool>,
}

impl ImageFullscreenDialogController {
    pub fn new() -> Self {
        Self {
            window_weak: Arc::new(Mutex::new(None)),
            main_window_weak: Arc::new(Mutex::new(None)),
            last_click_instant: Arc::new(Mutex::new(None)),
            is_active: Arc::new(AtomicBool::new(false)),
        }
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

    /// Displays the frameless fullscreen image window with strict singleton enforcement and full Main UI input locking.
    pub fn show(
        &self,
        tab_manager: &ViewerTabManager,
        settings: &SettingsManager,
        main_window_weak: slint::Weak<MainWindow>,
    ) {
        let Some(active_tab) = tab_manager.get_active_tab() else {
            tracing::warn!("[FullscreenViewer: Show] Cannot show fullscreen: no active viewer tab session");
            return;
        };

        let t0 = Instant::now();
        *self.main_window_weak.lock() = Some(main_window_weak.clone());

        // Lock the entire Main UI to prevent user interaction while in fullscreen
        if let Some(mw) = main_window_weak.upgrade() {
            mw.set_is_modal_locked(true);
        }

        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();

            // Strict Singleton: If a fullscreen window is already open, sync and raise it
            if let Some(ref win) = *cell {
                self.sync_tab_content(win, &active_tab, settings);
                win.window().set_fullscreen(true);
                let _ = win.show();
                self.is_active.store(true, Ordering::SeqCst);
                return;
            }

            // Instantiate single clean fullscreen window
            let win = ImageFullscreenWindow::new()
                .expect("Failed to create ImageFullscreenWindow Slint component");

            let ctrl_close = self.clone();
            win.on_request_close(move || {
                ctrl_close.close();
            });

            let ctrl_double = self.clone();
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

            let ctrl_sys_close = self.clone();
            win.window().on_close_requested(move || {
                ctrl_sys_close.close();
                CloseRequestResponse::HideWindow
            });

            self.sync_tab_content(&win, &active_tab, settings);
            win.window().set_fullscreen(true);
            let _ = win.show();

            *self.window_weak.lock() = Some(win.as_weak());
            *cell = Some(win);
            self.is_active.store(true, Ordering::SeqCst);

            tracing::info!(
                media_id = active_tab.media_id,
                filename = %LogObscurer::filename(&active_tab.filename),
                elapsed_us = t0.elapsed().as_micros(),
                "[FullscreenViewer: Show] Fullscreen image viewer activated with Main UI locking"
            );
        });
    }

    /// Synchronizes active tab payload into the fullscreen window if currently visible.
    pub fn sync_active_tab(&self, tab_manager: &ViewerTabManager, settings: &SettingsManager) {
        if !self.is_open() {
            return;
        }

        let win_opt = self.window_weak.lock().as_ref().and_then(|w| w.upgrade());
        let Some(win) = win_opt else {
            return;
        };

        if let Some(active_tab) = tab_manager.get_active_tab() {
            self.sync_tab_content(&win, &active_tab, settings);
        } else {
            self.close();
        }
    }

    /// Hides and dismisses the fullscreen image window and safely unlocks the Main UI.
    pub fn close(&self) {
        if !self.is_active.swap(false, Ordering::SeqCst) {
            return;
        }

        FULLSCREEN_WINDOW.with(|win_cell| {
            let mut cell = win_cell.borrow_mut();
            if let Some(win) = cell.take() {
                let _ = win.hide();
                tracing::info!("[FullscreenViewer: Close] Fullscreen image viewer window dismissed");
            }
        });

        *self.window_weak.lock() = None;

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

        if let TabPayload::Image(ref img) = session.payload {
            win.set_is_loading(img.is_loading);
            win.set_error_message(img.error_message.clone().into());

            if let Some(ref buf) = img.high_res_pixels {
                win.set_source_image(Image::from_rgb8(buf.clone()));
            } else {
                win.set_source_image(Image::default());
            }
        }
    }
}
