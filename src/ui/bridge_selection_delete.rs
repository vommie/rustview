//! Deleting a selection that holds folders (Del, or "Delete..." in the menu of such a selection). The dialogs are the ones that
//! exist for files and for one folder; they run one after the other: first the files, then each folder. Cancelling one dialog ends
//! the sequence. A dialog that closes releases the modal lock of the main window; that is the signal to go on.

use super::MainWindow;
use super::bridge::{DeleteActionParams, UiBridge};
use super::bridge_file_dialog_delete::DeleteFileDialogController;
use super::bridge_file_dialog_remove_orphaned::RemoveOrphanedFileDialogController;
use super::bridge_folder_dialogs::FolderDialogController;
use super::bridge_state::UiState;
use super::favorites::FavoriteManager;
use super::multiview::MultiviewHandle;
use crate::library::Library;
use crate::library::models::{FolderId, MediaId};
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

enum Step {
    /// The selected media; `all_missing`: they are all gone from the drive already (the dialog only removes them from the database).
    Files { ids: Vec<MediaId>, all_missing: bool },
    Folder { id: FolderId, was_missing: bool },
}

struct Job {
    window: MainWindow,
    library: Arc<Library>,
    thumbnails: Arc<ThumbnailManager>,
    tab_manager: Arc<ViewerTabManager>,
    settings: Arc<SettingsManager>,
    state: Arc<RwLock<UiState>>,
    favorites: Arc<RwLock<FavoriteManager>>,
    delete_file_dialog: DeleteFileDialogController,
    remove_orphaned_file_dialog: RemoveOrphanedFileDialogController,
    folder_dialogs: FolderDialogController,
    fullscreen: MultiviewHandle,
    steps: VecDeque<Step>,
    awaiting: Option<Step>,
}

thread_local! {
    static JOB: RefCell<Option<Job>> = const { RefCell::new(None) };
}

pub struct SelectionDelete;

impl SelectionDelete {
    /// Starts the sequence for `files` and `folders` (the Browse selection).
    pub fn start(params: &DeleteActionParams<'_>, folders: Vec<FolderId>, files: Vec<MediaId>) {
        let mut steps = VecDeque::new();
        if !files.is_empty() {
            let all_missing = {
                let st = params.state.read();
                files.iter().all(|&id| st.all_folder_media.find(id).map(|m| m.record.is_deleted).unwrap_or(false))
            };
            steps.push_back(Step::Files { ids: files, all_missing });
        }
        for id in folders {
            let was_missing = params.library.folders().get_folder(id).ok().flatten().map(|r| r.is_deleted).unwrap_or(false);
            steps.push_back(Step::Folder { id, was_missing });
        }
        tracing::info!(steps = steps.len(), "[SelectionDelete: Start] Deleting a selection of folders and files, one dialog after the other");

        let job = Job {
            window: params.window.clone_strong(),
            library: params.library.clone(),
            thumbnails: params.thumbnails.clone(),
            tab_manager: params.tab_manager.clone(),
            settings: params.settings.clone(),
            state: params.state.clone(),
            favorites: params.favorites.clone(),
            delete_file_dialog: params.delete_file_dialog.clone(),
            remove_orphaned_file_dialog: params.remove_orphaned_file_dialog.clone(),
            folder_dialogs: params.folder_dialogs.clone(),
            fullscreen: params.fullscreen.clone(),
            steps,
            awaiting: None,
        };
        JOB.with(|slot| *slot.borrow_mut() = Some(job));
        Self::advance();
    }

    /// A dialog released the modal lock: the step it belonged to either did its work (go on) or was cancelled (stop).
    pub fn on_modal_lock_released() {
        let proceed = JOB.with(|slot| {
            let mut slot = slot.borrow_mut();
            let Some(job) = slot.as_mut() else { return false };
            let Some(step) = job.awaiting.take() else { return false };
            if job.is_done(&step) {
                true
            } else {
                tracing::info!("[SelectionDelete: Cancelled] A dialog was cancelled; the rest of the selection stays");
                *slot = None;
                false
            }
        });
        if proceed {
            Self::advance();
        }
    }

    /// Opens the dialog of the next step; a step whose dialog does not open is skipped.
    fn advance() {
        loop {
            let Some(mut job) = JOB.with(|slot| slot.borrow_mut().take()) else { return };
            let Some(step) = job.steps.pop_front() else {
                return;
            };
            job.open(&step);
            let opened = job.window.get_is_modal_locked();
            if opened {
                job.awaiting = Some(step);
            }
            JOB.with(|slot| *slot.borrow_mut() = Some(job));
            if opened {
                return;
            }
        }
    }
}

impl Job {
    fn params(&self) -> DeleteActionParams<'_> {
        DeleteActionParams {
            window: &self.window,
            library: &self.library,
            thumbnails: &self.thumbnails,
            tab_manager: &self.tab_manager,
            settings: &self.settings,
            state: &self.state,
            favorites: &self.favorites,
            delete_file_dialog: &self.delete_file_dialog,
            remove_orphaned_file_dialog: &self.remove_orphaned_file_dialog,
            folder_dialogs: &self.folder_dialogs,
            fullscreen: &self.fullscreen,
        }
    }

    fn open(&self, step: &Step) {
        match step {
            Step::Files { ids, .. } => UiBridge::open_media_delete_dialog(&self.params(), ids.clone(), None),
            Step::Folder { id, .. } => UiBridge::open_folder_delete_dialog(&self.params(), *id),
        }
    }

    /// Whether the dialog of `step` did its work: the folder or the files are gone from the library or from the drive.
    fn is_done(&self, step: &Step) -> bool {
        match step {
            Step::Files { ids, all_missing } => ids.iter().all(|&id| match self.library.resolve_media_path(id) {
                Err(_) => true,
                Ok(path) => !all_missing && !path.exists(),
            }),
            Step::Folder { id, was_missing } => match self.library.folders().get_folder(*id) {
                Ok(Some(record)) => !was_missing && record.is_deleted,
                _ => true,
            },
        }
    }
}
