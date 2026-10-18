pub use super::bridge_folder_dialog_delete::DeleteFolderDialogController;
pub use super::bridge_folder_dialog_missing_delete::MissingFolderDeleteDialogController;
pub use super::bridge_folder_dialog_move::MoveFolderDialogController;
pub use super::bridge_folder_dialog_new::NewFolderDialogController;
pub use super::bridge_folder_dialog_reborn_warn::RebornFolderWarnDialogController;
pub use super::bridge_folder_dialog_remove_orphaned::RemoveOrphanedFolderDialogController;
pub use super::bridge_selection_delete::DeleteSelectionDialogController;
pub use super::bridge_folder_dialog_rename::RenameFolderDialogController;

use super::bridge_state::UiState;
use super::MainWindow;
use crate::library::Library;
use crate::library::models::FolderId;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use std::sync::Arc;

/// Thread-safe context bundle shared across folder dialog operations.
#[derive(Clone)]
pub struct FolderDialogContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub tab_manager: Arc<ViewerTabManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

/// Unified facade coordinating individual folder dialog controllers adhering to OCP and SRP.
#[derive(Default, Clone)]
pub struct FolderDialogController {
    pub rename: RenameFolderDialogController,
    pub new_folder: NewFolderDialogController,
    pub delete: DeleteFolderDialogController,
    pub delete_selection: DeleteSelectionDialogController,
    pub missing_delete: MissingFolderDeleteDialogController,
    pub move_folder: MoveFolderDialogController,
    pub remove_orphaned: RemoveOrphanedFolderDialogController,
    pub reborn_warn: RebornFolderWarnDialogController,
}

impl FolderDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn show_new(&self, window: &MainWindow, parent_folder_id: FolderId, ctx: FolderDialogContext) {
        self.new_folder.show(window, parent_folder_id, ctx);
    }

    #[inline]
    pub fn close_new(&self, window: &MainWindow) {
        self.new_folder.close(window);
    }

    #[inline]
    pub fn show_rename(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        current_name: &str,
        ctx: FolderDialogContext,
    ) {
        self.rename.show(window, folder_id, current_name, ctx);
    }

    #[inline]
    pub fn close_rename(&self, window: &MainWindow) {
        self.rename.close(window);
    }

    #[inline]
    pub fn show_delete(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        ctx: FolderDialogContext,
    ) {
        self.delete
            .show(window, folder_id, folder_name, ctx, &self.missing_delete);
    }

    #[inline]
    pub fn close_delete(&self, window: &MainWindow) {
        self.delete.close(window);
    }

    /// The combined dialog for a selection of folders and files.
    #[inline]
    pub fn show_delete_selection(
        &self,
        window: &MainWindow,
        files: Vec<crate::library::models::MediaId>,
        folders: Vec<FolderId>,
        ctx: FolderDialogContext,
    ) {
        self.delete_selection.show(window, files, folders, ctx, &self.reborn_warn);
    }

    #[inline]
    pub fn close_delete_selection(&self, window: &MainWindow) {
        self.delete_selection.close(window);
    }

    #[inline]
    pub fn show_missing_delete(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        folder_path: &str,
        ctx: FolderDialogContext,
    ) {
        self.missing_delete
            .show(window, folder_id, folder_name, folder_path, ctx);
    }

    #[inline]
    pub fn close_missing_delete(&self, window: &MainWindow) {
        self.missing_delete.close(window);
    }

    #[inline]
    pub fn show_remove_orphaned(
        &self,
        window: &MainWindow,
        folder_id: FolderId,
        folder_name: &str,
        ctx: FolderDialogContext,
    ) {
        self.remove_orphaned
            .show(window, folder_id, folder_name, ctx, &self.reborn_warn);
    }

    #[inline]
    pub fn close_remove_orphaned(&self, window: &MainWindow) {
        self.remove_orphaned.close(window);
    }

    #[inline]
    pub fn show_reborn_warn(&self, window: &MainWindow, folder_name: &str, folder_path: &str) {
        self.reborn_warn.show(window, folder_name, folder_path);
    }

    #[inline]
    pub fn close_reborn_warn(&self, window: &MainWindow) {
        self.reborn_warn.close(window);
    }

    #[inline]
    pub fn show_move(
        &self,
        window: &MainWindow,
        src_folder_id: FolderId,
        dst_folder_id: FolderId,
        ctx: FolderDialogContext,
    ) {
        self.move_folder.show(window, src_folder_id, dst_folder_id, ctx);
    }

    #[inline]
    pub fn close_move(&self, window: &MainWindow) {
        self.move_folder.close(window);
    }
}
