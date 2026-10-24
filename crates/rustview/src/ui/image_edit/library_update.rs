//! The library after the editor wrote a file: the record, hash and thumbnail of an overwritten medium are refreshed and every
//! view of it shows the new picture (the quiet refresh of `BridgeMedia`); a new file ("Save As") joins the library at once,
//! inside the root or outside it (the tree does not show the latter, but the record keeps its rating and tags until a wider
//! root shows it), and the Browse view shows it when it lists that folder.

use super::EditContext;
use crate::ui::MainWindow;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::{BridgeMedia, RefreshOrigin};
use crate::ui::selection::BridgeFileSelection;
use rustview_core::library::file_operations::is_same_file_path;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::{MediaId, MediaRecord};
use std::path::Path;

/// `media_id` was overwritten with its edited picture, or its edits were dropped for the file as it is on disk now.
pub fn refresh_medium(context: &EditContext, window: &MainWindow, media_id: MediaId) {
    let app = &context.app;
    BridgeMedia::refresh_media_ids(
        window,
        &app.library,
        &app.thumbnails,
        &app.tab_manager,
        &app.settings,
        &app.state,
        &[media_id],
        RefreshOrigin::Saved,
    );
}

/// The root of the folder tree, if one is configured.
fn scope_root(context: &EditContext) -> Option<std::path::PathBuf> {
    context
        .app
        .settings
        .get()
        .general
        .root_folder
        .as_deref()
        .map(normalize_path)
}

/// The edited picture of `source_id` was written to the new file `written`; with `keep_assignments` it takes over the rating
/// and the tags. Returns the record of the new medium (`None` when it could not be indexed; a refresh of its folder finds it).
pub fn after_export(
    context: &EditContext,
    window: &MainWindow,
    source_id: MediaId,
    written: &Path,
    keep_assignments: bool,
) -> Option<MediaRecord> {
    let app = &context.app;
    let folder = written.parent()?;
    let name = written.file_name()?.to_string_lossy().into_owned();
    let root = scope_root(context);
    let in_scope = root.as_deref().is_some_and(|r| normalize_path(folder).starts_with(r));
    let (folder_record, added) = match app.library.folder_creator().register_existing(folder, root.as_deref()) {
        Ok(registered) => registered,
        Err(e) => {
            tracing::warn!(error = %e, "[ImageEdit: Library] The folder of the new file could not be registered");
            return None;
        }
    };
    let record = match app.library.file_operations().index_edited_copy(
        source_id,
        folder_record.id,
        &name,
        keep_assignments,
    ) {
        Ok(record) => record,
        Err(e) => {
            tracing::warn!(error = %e, "[ImageEdit: Library] The new file could not be indexed; a refresh of its folder finds it");
            return None;
        }
    };
    let shown = is_shown_folder(context, folder);
    if in_scope {
        // The folder counts of the tree come from the folder snapshot: it is read again with the new count (and new folders).
        app.state.write().invalidate_folder_hierarchy();
        BridgeFolders::reload_tree(window, &app.library, &app.settings, &app.state);
    }
    if shown {
        // The folder is listed: a reload (from the library, synchronous) shows the new medium; its thumbnail is made by the
        // folder pipeline.
        BridgeMedia::reload(
            window,
            &app.library,
            &app.thumbnails,
            &app.settings,
            &app.state,
            true,
            true,
        );
    }
    tracing::info!(
        media_id = record.id,
        listed = shown,
        in_scope,
        folders_added = added,
        keep_assignments,
        "[ImageEdit: Library] The new file was indexed"
    );
    Some(record)
}

fn is_shown_folder(context: &EditContext, folder: &Path) -> bool {
    context
        .app
        .state
        .read()
        .current_folder_path
        .as_deref()
        .is_some_and(|current| is_same_file_path(current, folder))
}

/// A Multiview that follows the Browse selection now shows `new` (saved at `path`) where it showed `old`: the Browse view
/// selects it in the background, in place of `old`, without telling the Multiview (it shows the picture already, so nothing
/// is loaded twice). A file in another folder of the library is selected after the Browse view went there, but only when the
/// Multiview shows that one picture (its other pictures would no longer be selected otherwise); a file outside the library's
/// root cannot be selected at all.
pub fn select_in_browse(
    context: &EditContext,
    window: &MainWindow,
    old: MediaId,
    new: &MediaRecord,
    path: &Path,
    lone: bool,
) {
    let app = &context.app;
    let Some(folder) = path.parent() else { return };
    let in_scope = scope_root(context).is_some_and(|r| normalize_path(folder).starts_with(r));
    let selected = if !in_scope {
        false
    } else if is_shown_folder(context, folder) {
        BridgeFileSelection::swap_selected_media(old, new.id, window, &app.library, &app.state)
    } else if lone {
        BridgeFolders::select_folder(
            new.folder_id,
            folder,
            window,
            &app.library,
            &app.thumbnails,
            &app.settings,
            &app.state,
        );
        BridgeFileSelection::swap_selected_media(old, new.id, window, &app.library, &app.state)
    } else {
        false
    };
    tracing::info!(
        old_media_id = old,
        media_id = new.id,
        in_scope,
        selected,
        "[ImageEdit: Library] The Browse view follows the saved copy"
    );
}
