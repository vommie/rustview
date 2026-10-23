//! The combined "Delete Files & Folders" dialog: deleting a selection of the Browse view that holds folders.
//!
//! What gets deleted, and how, is `library::selection_delete` (the same operations and checks as the single dialogs: Trash or
//! permanent delete of what is on disk, database cleanup of what is gone, a folder that is back on disk is restored and kept). This
//! module only shows the plan, asks, and brings the tree, the list, the viewers and the panels in line with the result.

use crate::ui::file_dialogs::FileDialogContext;
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_dialogs::reborn_warn::RebornFolderWarnDialogController;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::status_bar::{StatusBarBridge, StatusMessageLevel};
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::BridgeTags;
use crate::ui::{DeleteSelectionDialog, MainWindow};
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::{FolderId, MediaId};
use rustview_core::library::selection_delete::{
    DeleteMode, FolderState, SelectionDeleteExecutor, SelectionDeletePlan, SelectionDeletePlanner,
    SelectionDeleteReport,
};
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// For a folder that is deleted: which part of its subtree is gone from the drive already (`None` when nothing is).
pub fn missing_in_subtree_hint(missing_subfolders: usize, missing_media: usize) -> Option<String> {
    let mut parts = Vec::new();
    if missing_subfolders > 0 {
        parts.push(count(missing_subfolders, "subfolder", "subfolders"));
    }
    if missing_media > 0 {
        parts.push(count(missing_media, "media file", "media files"));
    }
    let verb = if missing_subfolders + missing_media == 1 {
        "is"
    } else {
        "are"
    };
    (!parts.is_empty()).then(|| format!("Of these, {} {verb} already missing on disk.", parts.join(" and ")))
}

/// For files that are deleted: how many of them are gone from the drive already (`None` when none or when all are, which has its own dialog).
pub fn missing_files_hint(missing: usize, total: usize) -> Option<String> {
    (missing > 0 && missing < total).then(|| {
        format!(
            "{missing} of the {total} files {} already missing on disk and {} only removed from the database.",
            if missing == 1 { "is" } else { "are" },
            if missing == 1 { "is" } else { "are" }
        )
    })
}

/// Every text of the dialog, derived from the plan alone (unit tested).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogTexts {
    pub window_title: String,
    pub heading: String,
    pub question: String,
    pub details: Vec<String>,
    pub database_details: Vec<String>,
    pub note: String,
    pub database_only: bool,
}

impl DialogTexts {
    /// `reborn`: how many selected folders turned out to exist on disk again and were restored (they are not part of the plan).
    pub fn of(plan: &SelectionDeletePlan, reborn: usize) -> Self {
        let present_files: Vec<_> = plan.present_files().collect();
        let missing_files: Vec<_> = plan.missing_files().collect();
        let present_folders: Vec<_> = plan.folders_in(FolderState::Present).collect();
        let missing_folders: Vec<_> = plan.folders_in(FolderState::Missing).collect();
        let database_only = plan.is_database_only();

        // The title names everything the selection holds, as the user sees it.
        let (files, folders) = (plan.files.len(), plan.folders.len());
        let kinds = match (files, folders) {
            (0, f) => count_word(f, "Folder", "Folders"),
            (n, 0) => count_word(n, "File", "Files"),
            (n, f) => format!(
                "{} & {}",
                count_word(n, "File", "Files"),
                count_word(f, "Folder", "Folders")
            ),
        };
        let heading = if database_only {
            format!("Remove {kinds} from Database")
        } else {
            format!("Delete {kinds}")
        };

        let mut texts = Self {
            window_title: format!("RustView - {heading}"),
            heading,
            database_only,
            ..Self::default()
        };

        // Question and the list of what is deleted on disk.
        let file_phrase = match present_files.as_slice() {
            [] => None,
            [one] => Some(format!("the file '{}'", one.name)),
            many => Some(format!("the {} selected files", many.len())),
        };
        let folder_phrase = match present_folders.as_slice() {
            [] => None,
            [one] => Some(format!("the folder '{}'", one.name)),
            many => Some(format!("the {} selected folders", many.len())),
        };
        texts.question = match (file_phrase, folder_phrase) {
            (Some(f), Some(d)) => format!("Are you sure you want to delete {f} and {d}?"),
            (Some(f), None) | (None, Some(f)) => format!("Are you sure you want to delete {f}?"),
            (None, None) => {
                "None of the selected items exists on disk any more. Remove them from the library database?".to_string()
            }
        };

        if !present_files.is_empty() {
            texts.details.push(format!(
                "{}, with their records, thumbnails and preview caches",
                count(present_files.len(), "file", "files")
            ));
        }
        if !present_folders.is_empty() {
            let subfolders: usize = present_folders.iter().map(|f| f.subfolders).sum();
            let media: usize = present_folders.iter().map(|f| f.media_files).sum();
            texts.details.push(format!(
                "{} with {} and {}",
                count(present_folders.len(), "folder", "folders"),
                count(subfolders, "subfolder", "subfolders"),
                count(media, "media file", "media files")
            ));
            let missing_subfolders: usize = present_folders.iter().map(|f| f.missing_subfolders).sum();
            let missing_media: usize = present_folders.iter().map(|f| f.missing_media).sum();
            if let Some(hint) = missing_in_subtree_hint(missing_subfolders, missing_media) {
                texts.details.push(hint);
            }
        }

        // What is only cleaned from the database.
        if !missing_files.is_empty() {
            texts
                .database_details
                .push(count(missing_files.len(), "missing file", "missing files"));
        }
        if !missing_folders.is_empty() {
            let subfolders: usize = missing_folders.iter().map(|f| f.subfolders).sum();
            let media: usize = missing_folders.iter().map(|f| f.media_files).sum();
            texts.database_details.push(format!(
                "{} with {} and {} cached",
                count(missing_folders.len(), "missing folder", "missing folders"),
                count(subfolders, "subfolder", "subfolders"),
                count(media, "media file", "media files")
            ));
        }

        let mut notes = Vec::new();
        if !present_folders.is_empty() {
            notes.push("Folders and their media are deleted on disk and flagged in the library database.".to_string());
        }
        let covered = plan.nested_folders + plan.files_in_folders;
        if covered > 0 {
            notes.push(format!(
                "{} lie inside selected folders and are deleted with them.",
                count(covered, "selected item", "selected items")
            ));
        }
        if reborn > 0 {
            notes.push(format!(
                "{} found on disk again and restored; not deleted.",
                count(reborn, "folder was", "folders were")
            ));
        }
        if plan.protected_folders > 0 {
            notes.push("The library root cannot be deleted.".to_string());
        }
        texts.note = notes.join("\n");
        texts
    }
}

fn count_word(n: usize, one: &str, many: &str) -> String {
    (if n == 1 { one } else { many }).to_string()
}

/// The status bar line for a finished deletion and whether it is a warning.
pub fn summary(report: &SelectionDeleteReport, mode: DeleteMode) -> (String, StatusMessageLevel) {
    let mut parts = Vec::new();
    let on_disk = |files: usize, folders: usize| match (files, folders) {
        (0, 0) => None,
        (f, 0) => Some(count(f, "file", "files")),
        (0, d) => Some(count(d, "folder", "folders")),
        (f, d) => Some(format!(
            "{} and {}",
            count(f, "file", "files"),
            count(d, "folder", "folders")
        )),
    };
    if let Some(done) = on_disk(report.files_deleted.len(), report.top_level_folders_deleted) {
        let verb = if mode == DeleteMode::Trash {
            "Moved to Trash"
        } else {
            "Permanently deleted"
        };
        parts.push(format!("{verb} {done}."));
    }
    if let Some(done) = on_disk(report.files_purged.len(), report.top_level_folders_purged) {
        parts.push(format!("Removed {done} from database (missing on disk)."));
    }
    if !report.folders_reborn.is_empty() {
        parts.push(format!(
            "{} found on disk again and restored.",
            count(report.folders_reborn.len(), "folder was", "folders were")
        ));
    }
    let failures = report.failures();
    if failures > 0 {
        let first = report.file_failures.iter().chain(&report.folder_failures).next();
        let detail = first
            .map(|f| format!(" ({}: {})", f.name, f.reason))
            .unwrap_or_default();
        parts.push(format!("{} failed{detail}.", count(failures, "item", "items")));
    }
    if parts.is_empty() {
        parts.push("Nothing was deleted.".to_string());
    }
    let level = if failures > 0 || !report.folders_reborn.is_empty() {
        StatusMessageLevel::Warning
    } else {
        StatusMessageLevel::Success
    };
    (parts.join(" "), level)
}

/// The dialog and what it does when confirmed.
#[derive(Default, Clone)]
pub struct DeleteSelectionDialogController {
    dialog: Rc<RefCell<Option<DeleteSelectionDialog>>>,
}

impl DeleteSelectionDialogController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Plans the deletion of `files` and `folders` and asks the user. Nothing happens on disk before a button is pressed.
    pub fn show(
        &self,
        window: &MainWindow,
        files: &[MediaId],
        folders: &[FolderId],
        ctx: &FolderDialogContext,
        reborn_warn: &RebornFolderWarnDialogController,
    ) {
        window.set_is_context_menu_open(false);

        let protected = Self::protected_folders(ctx);
        let mut plan = match SelectionDeletePlanner::build(&ctx.library, files, folders, &protected) {
            Ok(plan) => plan,
            Err(e) => {
                tracing::error!(error = %e, "[DeleteSelectionDialog: Plan] The selection could not be planned");
                StatusBarBridge::error(window, format!("Cannot delete the selection: {e}"));
                return;
            }
        };

        // Folders flagged missing that are on disk again are restored right away, like the single dialogs do.
        let reborn = plan.take_reborn();
        let mut reborn_restored = 0;
        for folder in &reborn {
            if ctx
                .library
                .folder_operations()
                .check_and_restore_if_exists(folder.id)
                .is_ok()
            {
                reborn_restored += 1;
            }
        }
        if reborn_restored > 0 {
            BridgeFolders::reload_tree_after_restore(window, &ctx.library, &ctx.settings, &ctx.state);
        }

        if plan.is_empty() {
            match reborn.first() {
                Some(folder) => {
                    StatusBarBridge::info(window, "Folder re-detected on disk and restored.");
                    reborn_warn.show(window, &folder.name, &folder.path.to_string_lossy());
                }
                None => StatusBarBridge::info(
                    window,
                    "Nothing to delete: the selection is not part of the library any more.",
                ),
            }
            return;
        }

        self.ensure_dialog_instantiated();
        if let Some(ref win) = *self.dialog.borrow() {
            self.populate(win, &DialogTexts::of(&plan, reborn_restored));
            self.bind_dialog_events(win, window, &Rc::new(plan), ctx);
            window.set_is_modal_locked(true);
            let _ = win.show();
        }
    }

    /// Hides the dialog and releases the modal lock of the main window.
    pub fn close(&self, window: &MainWindow) {
        if let Some(ref win) = *self.dialog.borrow() {
            let _ = win.hide();
        }
        window.set_is_modal_locked(false);
    }

    fn protected_folders(ctx: &FolderDialogContext) -> HashSet<FolderId> {
        let mut protected = HashSet::new();
        if let Some(root) = ctx.settings.get().general.root_folder
            && let Ok(Some(record)) = ctx.library.folders().get_folder_by_path(&normalize_path(&root))
        {
            protected.insert(record.id);
        }
        protected
    }

    fn populate(&self, win: &DeleteSelectionDialog, texts: &DialogTexts) {
        let list = |lines: &[String]| {
            ModelRc::new(VecModel::from(
                lines.iter().map(|l| SharedString::from(l.as_str())).collect::<Vec<_>>(),
            ))
        };
        win.set_window_title(texts.window_title.as_str().into());
        win.set_heading(texts.heading.as_str().into());
        win.set_question(texts.question.as_str().into());
        win.set_details(list(&texts.details));
        win.set_database_details(list(&texts.database_details));
        win.set_note(texts.note.as_str().into());
        win.set_database_only(texts.database_only);
        win.set_error_message("".into());
    }

    /// Carries the deletion out and brings the application in line with what happened.
    fn execute(&self, plan: &SelectionDeletePlan, mode: DeleteMode, ctx: &FolderDialogContext, window: &MainWindow) {
        // Where the view goes if the open folder is part of the deletion (it cannot be for a selection of the Browse view).
        let affected: Vec<FolderId> = plan.folders.iter().flat_map(|f| f.affected.iter().copied()).collect();
        let successor = plan.folders.first().and_then(|first| {
            BridgeFolders::find_visible_successor_folder(first.id, &affected, &ctx.library, &ctx.settings, &ctx.state)
        });

        let report = SelectionDeleteExecutor::execute(&ctx.library, plan, mode);

        // Nothing worked: the dialog stays, with the reason.
        if !report.changed_anything() && report.failures() > 0 {
            let reasons: Vec<String> = report
                .file_failures
                .iter()
                .chain(&report.folder_failures)
                .map(|f| format!("{}: {}", f.name, f.reason))
                .collect();
            if let Some(ref win) = *self.dialog.borrow() {
                win.set_error_message(format!("Nothing could be deleted. {}", reasons.join("; ")).into());
            }
            return;
        }

        self.close(window);
        Self::sync_application(window, ctx, &report, successor);

        let (message, level) = summary(&report, mode);
        StatusBarBridge::set_message(window, message, level);
        StatusBarBridge::update_metrics(window, &ctx.state);
    }

    fn sync_application(
        window: &MainWindow,
        ctx: &FolderDialogContext,
        report: &SelectionDeleteReport,
        successor: Option<(FolderId, std::path::PathBuf)>,
    ) {
        let gone_files: HashSet<MediaId> = report
            .files_deleted
            .iter()
            .chain(&report.files_purged)
            .chain(&report.folder_media_purged)
            .copied()
            .collect();
        let flagged: HashSet<MediaId> = report.folder_media_flagged.iter().copied().collect();

        for &id in gone_files.iter().chain(&flagged) {
            ctx.tab_manager.notify_media_deleted(id);
        }

        {
            let mut st = ctx.state.write();
            st.remove_media(&gone_files);
            for item in st.all_folder_media.iter_mut() {
                if flagged.contains(&item.record.id) {
                    item.mark_deleted();
                }
            }
            st.file_selection.clear();
            st.folder_rows.clear_selection();
        }

        let folder_ids = report.all_folder_ids();
        BridgeFolders::prune_expanded_folders(&folder_ids, &ctx.settings, &ctx.state);
        BridgeFolders::navigate_to_successor_if_affected(
            successor,
            &folder_ids,
            window,
            &ctx.library,
            &ctx.thumbnails,
            &ctx.settings,
            &ctx.state,
        );

        let file_ctx = FileDialogContext {
            library: ctx.library.clone(),
            thumbnails: ctx.thumbnails.clone(),
            tab_manager: ctx.tab_manager.clone(),
            settings: ctx.settings.clone(),
            state: ctx.state.clone(),
        };
        BridgeViewerTabs::sync_ui(
            window,
            &file_ctx.tab_manager,
            &file_ctx.library,
            &file_ctx.settings,
            &file_ctx.state,
        );
        BridgeMedia::reload(
            window,
            &file_ctx.library,
            &file_ctx.thumbnails,
            &file_ctx.settings,
            &file_ctx.state,
            false,
            false,
        );
        BridgeInspector::update(window, &ctx.state);
        BridgeTags::sync_tag_selection(window, &ctx.library, &ctx.state);
        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(window, &ctx.tab_manager, &ctx.library, &ctx.settings, &ctx.state);
        }
    }

    fn ensure_dialog_instantiated(&self) {
        let mut d_ref = self.dialog.borrow_mut();
        if d_ref.is_none() {
            *d_ref = Some(DeleteSelectionDialog::new().expect("Failed to instantiate DeleteSelectionDialog"));
        }
    }

    fn bind_dialog_events(
        &self,
        win: &DeleteSelectionDialog,
        window: &MainWindow,
        plan: &Rc<SelectionDeletePlan>,
        ctx: &FolderDialogContext,
    ) {
        let confirm = |mode: DeleteMode| {
            let (ctrl, plan, ctx, window) = (self.clone(), plan.clone(), ctx.clone(), window.clone_strong());
            move || ctrl.execute(&plan, mode, &ctx, &window)
        };
        win.on_move_to_trash_confirmed(confirm(DeleteMode::Trash));
        win.on_permanent_delete_confirmed(confirm(DeleteMode::Permanent));
        // Only reached when nothing is left on the drive, so the mode does not matter.
        win.on_remove_from_database_confirmed(confirm(DeleteMode::Permanent));

        let (ctrl, window_cancel) = (self.clone(), window.clone_strong());
        win.on_cancelled(move || ctrl.close(&window_cancel));

        let (ctrl, window_close) = (self.clone(), window.clone_strong());
        win.window().on_close_requested(move || {
            ctrl.close(&window_close);
            CloseRequestResponse::HideWindow
        });
    }
}
