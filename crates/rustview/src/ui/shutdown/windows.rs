//! Every Slint window of the application and how the shutdown deals with it.
//!
//! `tests/window_shutdown_policy.rs` fails when a `.slint` file declares a window that is not listed here, so a new window
//! cannot be forgotten: add it to [`WINDOWS`] and close it in [`super::close_windows`].

/// `(component, how the shutdown closes it)`.
pub const WINDOWS: &[(&str, &str)] = &[
    ("MainWindow", "its close request starts the shutdown"),
    (
        "MultiviewFullscreenWindow",
        "MultiviewHandle::shutdown closes the session (pinned or not) and hides the window; Slint destroys it when the event loop ends",
    ),
    ("MultiviewBarWindow", "bar_overlay::shutdown destroys both bar windows"),
    (
        "SettingsWindow",
        "SettingsDialogController::close; the changes that were not saved are discarded",
    ),
    ("ColorPickerWindow", "color_picker::shutdown"),
    ("FileRenameDialog", "hidden directly"),
    ("FavoriteRenameDialog", "FavoriteRenameDialogController::close"),
    ("MissingFileDialog", "MissingFileDialogController::close"),
    ("OpenManyFilesDialog", "OpenConfirmation::close"),
    ("DeleteFileDialog", "DeleteFileDialogController::close"),
    ("RemoveOrphanedFileDialog", "RemoveOrphanedFileDialogController::close"),
    ("FileOpsConflictDialog", "FileOpsConflictDialogController::close"),
    ("AddTagDialog", "TagDialogController::close_add"),
    ("RenameTagDialog", "TagDialogController::close_rename"),
    ("DeleteTagDialog", "TagDialogController::close_delete"),
    ("RenameFolderDialog", "FolderDialogController::close_rename"),
    ("NewFolderDialog", "FolderDialogController::close_new"),
    ("DeleteFolderDialog", "FolderDialogController::close_delete"),
    (
        "DeleteSelectionDialog",
        "FolderDialogController::close_delete_selection",
    ),
    (
        "MissingFolderDeleteDialog",
        "FolderDialogController::close_missing_delete",
    ),
    (
        "RemoveOrphanedFolderDialog",
        "FolderDialogController::close_remove_orphaned",
    ),
    (
        "RebornFolderDeleteWarnDialog",
        "FolderDialogController::close_reborn_warn",
    ),
    ("MoveFolderToDialog", "FolderDialogController::close_move"),
];
