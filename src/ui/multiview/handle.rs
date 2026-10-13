//! Thread-safe face of the Multiview. The session itself (Slint window, timers) lives on the UI thread; this handle is what
//! the rest of the application holds: open/close requests, the F2 rename target and the "is a Multiview open" flag that the
//! drag-and-drop lock, dialogs and the tab embedding already ask the fullscreen controller about.

use super::eligibility;
use super::session;
use crate::library::models::MediaId;
use crate::settings::SettingsManager;
use crate::ui::MainWindow;
use crate::ui::bridge_image_fullscreen::FullscreenCloseCallback;
use crate::ui::bridge_state::UiState;
use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
use parking_lot::{Mutex, RwLock};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

static OPEN: AtomicBool = AtomicBool::new(false);

pub(super) fn set_open(open: bool) {
    OPEN.store(open, Ordering::SeqCst);
}

struct Shared {
    main_window: Mutex<Option<slint::Weak<MainWindow>>>,
    close_handler: Arc<Mutex<Option<FullscreenCloseCallback>>>,
    /// The instance shown by the open fullscreen Multiview (it owns the rename target).
    current: Mutex<WeakInstance>,
}

#[derive(Clone)]
pub struct MultiviewHandle {
    shared: Arc<Shared>,
}

impl Default for MultiviewHandle {
    fn default() -> Self {
        Self::new(Arc::new(Mutex::new(None)))
    }
}

impl MultiviewHandle {
    /// `close_handler` is shared with the single fullscreen controller: the same handler releases the modal lock and
    /// re-syncs the Browse view when either fullscreen mode closes.
    pub fn new(close_handler: Arc<Mutex<Option<FullscreenCloseCallback>>>) -> Self {
        Self {
            shared: Arc::new(Shared {
                main_window: Mutex::new(None),
                close_handler,
                current: Mutex::new(WeakInstance::new()),
            }),
        }
    }

    pub fn is_open(&self) -> bool {
        OPEN.load(Ordering::SeqCst)
    }

    /// The media an F2 rename acts on (the tile the user pressed F2 over) while the fullscreen Multiview is open.
    pub fn rename_target(&self) -> Option<MediaId> {
        if !self.is_open() {
            return None;
        }
        let instance = self.shared.current.lock().upgrade()?;
        instance.lock().rename_target()
    }

    pub(super) fn bind_instance(&self, instance: &SharedInstance) {
        *self.shared.current.lock() = Arc::downgrade(instance);
    }

    /// Opens the Multiview with the playable items of the current selection. UI thread only. Returns `false` if the
    /// selection does not contain at least two playable items.
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
                "[Multiview: Open] Ignored: fewer than two playable files in the selection"
            );
            return false;
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

    /// Closes the Multiview and runs the close handler (re-syncs the Browse view). Callable from any thread.
    pub fn request_close(&self) {
        if self.is_open() {
            let _ = slint::invoke_from_event_loop(|| session::close(true));
        }
    }

    /// Closes the Multiview without the close handler because another fullscreen replaces it. UI thread only.
    pub fn dismiss(&self) {
        if self.is_open() {
            session::close(false);
        }
    }

    /// F2 on a tile: asks the main window for the regular rename dialog, which acts on `rename_target()`.
    pub fn request_rename(&self) {
        if let Some(main_window) = self.shared.main_window.lock().clone() {
            let _ = main_window.upgrade_in_event_loop(|w| w.invoke_request_rename());
        }
    }

    pub(super) fn notify_closed(&self) {
        let handler = self.shared.close_handler.lock().clone();
        let main_window = self.shared.main_window.lock().clone();
        if let (Some(handler), Some(main_window)) = (handler, main_window) {
            let _ = main_window.upgrade_in_event_loop(move |w| {
                w.set_is_modal_locked(false);
                handler(0, None, None);
            });
        }
    }
}

/// Hook for the selection hub (`update_metrics`): costs one atomic load while no Multiview is open.
pub fn on_selection_changed() {
    if OPEN.load(Ordering::Relaxed) {
        session::schedule_reconcile();
    }
}
