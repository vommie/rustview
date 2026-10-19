//! Files on the system clipboard (cut, copy and paste of media, shared with the file managers of the desktop).
//!
//! `format` is the pure part (the formats Dolphin, Nautilus and the others write and read), `x11` the X11 implementation. There is no
//! Wayland implementation: on a session without an X display `FileClipboard::is_supported()` is false and the commands say so.

pub mod format;
mod x11;

pub use format::{ClipboardFiles, ClipboardMode};
pub use x11::ClipboardError;
use std::path::PathBuf;

pub struct FileClipboard;

impl FileClipboard {
    pub fn is_supported() -> bool {
        x11::available()
    }

    /// Puts `files` on the clipboard as a copy or a cut.
    pub fn set(files: &[PathBuf], mode: ClipboardMode) -> Result<(), ClipboardError> {
        if !Self::is_supported() {
            return Err(ClipboardError("the clipboard of this session is not supported (an X11 display is needed)".into()));
        }
        if files.is_empty() {
            return Err(ClipboardError("no files to put on the clipboard".into()));
        }
        x11::set(files, mode)
    }

    /// The files on the clipboard. Blocks until the owner answers: call it from a worker thread.
    pub fn read() -> Result<Option<ClipboardFiles>, ClipboardError> {
        if !Self::is_supported() {
            return Ok(None);
        }
        x11::read_files()
    }

    /// Empties the clipboard if RustView put its content there.
    pub fn clear_if_ours() {
        x11::clear_if_ours();
    }

    /// Hands what RustView put on the clipboard to the clipboard manager, so that it survives the exit.
    pub fn hand_over_to_manager() {
        x11::hand_over_to_manager();
    }

    /// What RustView put on the clipboard, while it is still there (no round trip).
    pub fn own_files() -> Option<(Vec<PathBuf>, ClipboardMode)> {
        x11::own_files()
    }
}
