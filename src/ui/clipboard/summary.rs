//! What the clipboard holds, as far as menus and buttons need to know (memory only).
//!
//! A watcher thread reads the clipboard whenever its owner changes (`FileClipboard::watch`) and publishes the result here; menus read it
//! when they open and never ask the clipboard (its owner can be slow or gone). The paste itself always reads the clipboard again.

use crate::media::MediaFormat;
use crate::system::file_clipboard::{ClipboardFiles, ClipboardMode};
use parking_lot::Mutex;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSummary {
    /// False until the clipboard was read once, or when the session cannot report changes: the menus then offer Paste and the paste
    /// itself decides.
    pub known: bool,
    /// Paths of files on the clipboard.
    pub total: usize,
    /// Of them, images and videos (judged by the extension; no drive access).
    pub media: usize,
    pub mode: ClipboardMode,
}

impl Default for ClipboardSummary {
    fn default() -> Self {
        Self { known: false, total: 0, media: 0, mode: ClipboardMode::Copy }
    }
}

static CURRENT: Mutex<ClipboardSummary> = Mutex::new(ClipboardSummary { known: false, total: 0, media: 0, mode: ClipboardMode::Copy });

pub fn is_media(path: &Path) -> bool {
    MediaFormat::from_path(path).is_supported()
}

impl ClipboardSummary {
    pub fn of(files: Option<&ClipboardFiles>) -> Self {
        match files {
            None => Self { known: true, ..Self::default() },
            Some(files) => Self {
                known: true,
                total: files.paths.len(),
                media: files.paths.iter().filter(|p| is_media(p)).count(),
                mode: files.mode,
            },
        }
    }

    pub fn current() -> Self {
        CURRENT.lock().clone()
    }

    pub fn publish(self) {
        *CURRENT.lock() = self;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn files(names: &[&str], mode: ClipboardMode) -> ClipboardFiles {
        ClipboardFiles { paths: names.iter().map(|n| PathBuf::from(format!("/x/{n}"))).collect(), mode, rejected: 0 }
    }

    #[test]
    fn media_are_counted_by_extension() {
        let summary = ClipboardSummary::of(Some(&files(&["a.JPG", "b.mp4", "c.txt", "d"], ClipboardMode::Cut)));
        assert_eq!((summary.known, summary.total, summary.media, summary.mode), (true, 4, 2, ClipboardMode::Cut));
    }

    #[test]
    fn an_empty_clipboard_is_known_to_be_empty() {
        let summary = ClipboardSummary::of(None);
        assert!(summary.known && summary.total == 0 && summary.media == 0);
        assert!(!ClipboardSummary::default().known);
    }
}
