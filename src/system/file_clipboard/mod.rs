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

    /// Empties the clipboard whoever filled it (after a cut was pasted, like a file manager does).
    pub fn empty() {
        if Self::is_supported() {
            x11::empty();
        }
    }

    /// Follows the clipboard: `on_change` is called on a worker thread, once now and after every change, with the files on the
    /// clipboard (`None` when it holds none). Changes that follow each other quickly are read once. Returns false when the session
    /// cannot report changes; the caller then reads on demand.
    pub fn watch(on_change: impl Fn(Option<ClipboardFiles>) + Send + 'static) -> bool {
        if !Self::is_supported() {
            return false;
        }
        let (wake, signals) = std::sync::mpsc::channel::<()>();
        let wake = std::sync::Mutex::new(wake);
        let hook: Box<dyn Fn() + Send + Sync> = Box::new(move || {
            if let Ok(wake) = wake.lock() {
                let _ = wake.send(());
            }
        });
        if !x11::set_change_hook(hook) {
            return false;
        }
        let spawned = std::thread::Builder::new().name("file-clipboard-watch".into()).spawn(move || {
            let read = || match x11::read_files() {
                Ok(files) => files,
                Err(e) => {
                    tracing::debug!(error = %e, "[FileClipboard: Watch] The clipboard could not be read");
                    None
                }
            };
            on_change(read());
            while signals.recv().is_ok() {
                // A copy often changes the owner twice (the manager takes over): read the settled state once.
                while signals.recv_timeout(std::time::Duration::from_millis(60)).is_ok() {}
                on_change(read());
            }
        });
        spawned.is_ok()
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
