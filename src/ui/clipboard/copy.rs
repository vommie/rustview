//! Cut and copy: puts the selected media on the system clipboard (nothing moves yet; a cut moves when it is pasted).

use crate::library::models::MediaId;
use crate::library::Library;
use crate::system::file_clipboard::{ClipboardMode, FileClipboard};
use crate::ui::{MainWindow, StatusBarBridge, UiState};
use parking_lot::RwLock;
use std::path::PathBuf;

pub struct CopySelection;

/// The files of a selection that can go on the clipboard, and what had to stay behind. Memory only.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Collected {
    pub paths: Vec<PathBuf>,
    /// Flagged missing or not complete yet: there is nothing to copy.
    pub unavailable: usize,
    pub folders: usize,
}

impl CopySelection {
    /// Ctrl+C / Ctrl+X: the selection of the Browse view.
    pub fn from_selection(window: &MainWindow, state: &RwLock<UiState>, mode: ClipboardMode) {
        let collected = {
            let st = state.read();
            let (folders, _) = st.browse_selection_counts();
            let mut collected = Collected { folders, ..Collected::default() };
            for item in st.get_selected_media_ordered() {
                if item.is_deleted || item.is_pending {
                    collected.unavailable += 1;
                } else {
                    collected.paths.push(item.full_path());
                }
            }
            collected
        };
        Self::put(window, collected, mode);
    }

    /// The Cut / Copy entries of a media menu: the media it was opened for.
    pub fn from_ids(window: &MainWindow, library: &Library, state: &RwLock<UiState>, ids: &[MediaId], mode: ClipboardMode) {
        let mut collected = Collected::default();
        {
            let st = state.read();
            for &id in ids {
                if let Some(item) = st.all_folder_media.find(id) {
                    if item.is_deleted || item.is_pending {
                        collected.unavailable += 1;
                    } else {
                        collected.paths.push(item.full_path());
                    }
                    continue;
                }
                match library.media().get(id) {
                    Ok(Some(record)) if !record.is_deleted => match library.resolve_media_path(id) {
                        Ok(path) => collected.paths.push(path),
                        Err(_) => collected.unavailable += 1,
                    },
                    _ => collected.unavailable += 1,
                }
            }
        }
        Self::put(window, collected, mode);
    }

    fn put(window: &MainWindow, collected: Collected, mode: ClipboardMode) {
        let verb = match mode {
            ClipboardMode::Copy => "copy",
            ClipboardMode::Cut => "cut",
        };
        if !FileClipboard::is_supported() {
            StatusBarBridge::warn(window, format!("Cannot {verb} files: the file clipboard needs an X11 session."));
            return;
        }
        if collected.paths.is_empty() {
            let reason = if collected.unavailable > 0 {
                "the selected files are missing on disk or not complete yet"
            } else if collected.folders > 0 {
                "folders cannot be copied or cut yet"
            } else {
                "no file is selected"
            };
            StatusBarBridge::warn(window, format!("Nothing to {verb}: {reason}."));
            return;
        }
        let count = collected.paths.len();
        match FileClipboard::set(&collected.paths, mode) {
            Ok(()) => {
                tracing::info!(count, mode = ?mode, unavailable = collected.unavailable, folders = collected.folders, "[Clipboard: Put] Files are on the clipboard");
                let mut message = format!(
                    "{} {} on the clipboard",
                    if mode == ClipboardMode::Cut { "Cut" } else { "Copied" },
                    if count == 1 { "1 file".to_string() } else { format!("{count} files") }
                );
                let mut left_out = Vec::new();
                if collected.unavailable > 0 {
                    left_out.push(format!("{} missing or incomplete", collected.unavailable));
                }
                if collected.folders > 0 {
                    left_out.push(format!("{} folder(s) not supported", collected.folders));
                }
                if left_out.is_empty() {
                    StatusBarBridge::success(window, message);
                } else {
                    message.push_str(&format!(" ({} left out)", left_out.join(", ")));
                    StatusBarBridge::warn(window, message);
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "[Clipboard: Put] The clipboard could not be filled");
                StatusBarBridge::error(window, format!("Cannot {verb} files: {e}."));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_collected_by_default() {
        assert_eq!(Collected::default(), Collected { paths: vec![], unavailable: 0, folders: 0 });
    }
}
