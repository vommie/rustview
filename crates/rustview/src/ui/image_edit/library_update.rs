//! The library after the editor wrote a file: the record, hash and thumbnail of an overwritten medium are refreshed and every
//! view of it shows the new picture (the quiet refresh of `BridgeMedia`); a new file in a library folder ("Save As") joins the
//! library at once, and the Browse view shows it when it lists that folder.

use super::EditContext;
use crate::ui::MainWindow;
use crate::ui::media::{BridgeMedia, RefreshOrigin};
use rustview_core::library::models::MediaId;
use rustview_core::settings::LogObscurer;
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

/// The edited picture of `source_id` was written to the new file `written`.
pub fn after_export(context: &EditContext, window: &MainWindow, source_id: MediaId, written: &Path) {
    let app = &context.app;
    let (Some(folder), Some(name)) = (
        written.parent(),
        written.file_name().map(|n| n.to_string_lossy().into_owned()),
    ) else {
        return;
    };
    let folder_record = match app.library.folders().get_folder_by_path(folder) {
        Ok(Some(record)) if !record.is_deleted => record,
        _ => {
            tracing::info!(
                path = %LogObscurer::path(written),
                "[ImageEdit: Library] The new file is outside the library folders; it is not indexed"
            );
            return;
        }
    };
    match app
        .library
        .file_operations()
        .index_edited_copy(source_id, folder_record.id, &name, true)
    {
        Ok(record) => {
            let shown = app
                .state
                .read()
                .current_folder_path
                .as_deref()
                .is_some_and(|current| rustview_core::library::file_operations::is_same_file_path(current, folder));
            // The folder counts of the tree come from the folder snapshot: it is read again with the new count.
            app.state.write().invalidate_folder_hierarchy();
            crate::ui::folder_tree::BridgeFolders::reload_tree(window, &app.library, &app.settings, &app.state);
            if shown {
                // The folder is listed: a reload shows the new medium and its thumbnail is made by the folder pipeline.
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
                "[ImageEdit: Library] The new file was indexed"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "[ImageEdit: Library] The new file could not be indexed; a refresh of its folder finds it");
        }
    }
}
