//! The orderly shutdown of the application.
//!
//! It starts when the main window gets a close request (title bar button, Alt+F4, a window manager, or a termination signal
//! that the housekeeping timer in `main.rs` turns into the same request) and runs on the UI thread in this order:
//!
//! 0. background work (folder loads, the thumbnail pipeline) is asked to stop;
//! 1. all other windows are closed (`close_windows`): the dialogs, the settings window and the color picker, the fullscreen
//!    Multiview (pinned or not) and the windows of the action bars;
//! 2. the main window disappears;
//! 3. the native resources are released (`release_resources`): every tab's players are shut down before the windows they
//!    render into are destroyed, a running drag ghost helper is stopped, and the settings are written one last time;
//! 4. the event loop ends.
//!
//! A watchdog ends the process if this takes longer than `SHUTDOWN_WATCHDOG_MS`, so a stuck teardown never leaves an
//! invisible process behind.

mod windows;

pub use windows::WINDOWS;

use crate::system::SystemDndHandler;
use crate::system::termination;
use crate::ui::SettingsDialogController;
use crate::ui::open_confirmation::OpenConfirmation;
use crate::ui::tabs::merge_limit::MergeLimitDialog;
use crate::ui::{
    DeleteFileDialogController, FavoriteRenameDialogController, FileOpsConflictDialogController, FileRenameDialog,
    FolderDialogController, MainWindow, MissingFileDialogController, MultiviewHandle,
    RemoveOrphanedFileDialogController, TagDialogController, UiState, color_picker,
};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::settings::SettingsManager;
use rustview_core::settings::schema::SHUTDOWN_WATCHDOG_MS;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STARTED: AtomicBool = AtomicBool::new(false);

/// Claims the shutdown: true for the first caller only. Arms the watchdog.
pub fn begin() -> bool {
    if STARTED.swap(true, Ordering::SeqCst) {
        return false;
    }
    termination::arm_watchdog(Duration::from_millis(SHUTDOWN_WATCHDOG_MS));
    true
}

/// True once the shutdown has started.
pub fn is_started() -> bool {
    STARTED.load(Ordering::SeqCst)
}

/// What the shutdown has to reach. Built by `UiBridge`, which owns all of it.
pub struct ShutdownTargets<'a> {
    pub window: &'a MainWindow,
    pub settings: &'a SettingsManager,
    pub thumbnails: &'a ThumbnailManager,
    pub state: &'a RwLock<UiState>,
    pub settings_dialog: &'a Rc<RefCell<SettingsDialogController>>,
    pub fullscreen: &'a MultiviewHandle,
    pub tab_manager: &'a ViewerTabManager,
    pub file_rename_dialog: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub favorite_rename_dialog: &'a FavoriteRenameDialogController,
    pub missing_file_dialog: &'a MissingFileDialogController,
    pub delete_file_dialog: &'a DeleteFileDialogController,
    pub remove_orphaned_dialog: &'a RemoveOrphanedFileDialogController,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub tag_dialogs: &'a TagDialogController,
    pub folder_dialogs: &'a FolderDialogController,
}

/// Runs the whole shutdown (the caller has claimed it with [`begin`]). UI thread only.
pub fn run(targets: &ShutdownTargets<'_>) {
    let started = Instant::now();
    tracing::info!(
        windows = WINDOWS.len(),
        "[Shutdown: Begin] Closing all windows and releasing the native resources"
    );
    stop_background_work(targets);
    close_windows(targets);
    if let Err(e) = targets.window.hide() {
        tracing::warn!(error = %e, "[Shutdown: Windows] The main window could not be hidden");
    }
    release_resources(targets);
    tracing::info!(
        elapsed_ms = started.elapsed().as_millis().saturating_as::<u64>(),
        "[Shutdown: Done] Windows closed, native resources released, settings written"
    );
}

/// Asks the work that runs in the background (folder loads and scans, the thumbnail pipeline) to stop at its next checkpoint,
/// so that no new batch starts while the windows and players go away.
fn stop_background_work(t: &ShutdownTargets<'_>) {
    t.thumbnails.cancel_pipeline();
    crate::ui::list_loader::shutdown();
    if let Some(flag) = t.state.read().active_folder_cancel_flag.as_ref() {
        flag.store(true, Ordering::Release);
    }
    tracing::info!("[Shutdown: Background] Thumbnail pipeline and folder load asked to stop");
}

/// Closes every window except the main window.
fn close_windows(t: &ShutdownTargets<'_>) {
    let window = t.window;

    if let Ok(settings_dialog) = t.settings_dialog.try_borrow() {
        settings_dialog.close();
    }
    color_picker::shutdown();
    crate::ui::log_window::close();

    if let Ok(dialog) = t.file_rename_dialog.try_borrow()
        && let Some(dialog) = dialog.as_ref()
    {
        let _ = dialog.hide();
    }
    t.favorite_rename_dialog.close(window);
    t.missing_file_dialog.close(window);
    t.delete_file_dialog.close(window);
    OpenConfirmation::close(window);
    MergeLimitDialog::close(window);
    crate::ui::image_edit::close_windows(window);
    t.remove_orphaned_dialog.close(window);
    t.conflict_dialog.close(window);
    t.tag_dialogs.close_add(window);
    t.tag_dialogs.close_rename(window);
    t.tag_dialogs.close_delete(window);
    t.folder_dialogs.close_new(window);
    t.folder_dialogs.close_rename(window);
    t.folder_dialogs.close_delete(window);
    t.folder_dialogs.close_delete_selection(window);
    t.folder_dialogs.close_missing_delete(window);
    t.folder_dialogs.close_remove_orphaned(window);
    t.folder_dialogs.close_reborn_warn(window);
    t.folder_dialogs.close_move(window);

    t.fullscreen.shutdown();
    tracing::info!("[Shutdown: Windows] All windows except the main window are closed");
}

/// Releases what lives outside the windows.
fn release_resources(t: &ShutdownTargets<'_>) {
    // Unsaved edits are not asked about when the application exits (they live in memory only; the files are untouched).
    let unsaved: usize = t
        .tab_manager
        .all_instances()
        .iter()
        .map(|instance| instance.lock().unsaved_entries().len())
        .sum();
    if unsaved > 0 {
        tracing::warn!(unsaved, "[Shutdown: ImageEdit] Unsaved picture edits are discarded");
    }
    t.tab_manager.shutdown();
    // The dock saved every change already; its bridge goes with the window.
    crate::ui::dock::release();
    SystemDndHandler::cancel_ghost_overlay();
    t.settings.flush();
}
