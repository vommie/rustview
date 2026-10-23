//! The drag ghost window shown while dragging files out of `RustView`: a small card with the thumbnail, the file name and the number
//! of files that follows the pointer (`GhostWindow`, drawn by `ghost_image`).

use super::ghost_image::{self, GhostSpec};
use super::{ACTIVE_GHOST, SystemDndHandler};
use crate::viewer::x11::GhostWindow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// The ghost sits this far right of and below the pointer (pixels).
const POINTER_OFFSET: (i32, i32) = (12, 12);
/// Pointer tracking runs at about 60 frames per second.
const FRAME: Duration = Duration::from_millis(16);
/// A drag never lasts this long; the ghost disappears on its own if the end of the drag was missed.
const MAX_LIFETIME: Duration = Duration::from_secs(60);

impl SystemDndHandler {
    /// Shows the ghost window next to the pointer. Without a real thumbnail it shows the generic file icon. Everything (fonts, drawing,
    /// the X11 connection) happens on a worker thread; where there is no X11 display no ghost appears and nothing else changes.
    pub fn start_ghost_overlay(title: &str, count: usize, thumb_path: Option<&Path>) {
        Self::cancel_ghost_overlay();

        let title = title.to_owned();
        let thumbnail: Option<PathBuf> = thumb_path.map(Path::to_path_buf).or_else(Self::fallback_icon_path);
        let (stop, stopped) = mpsc::channel();
        *ACTIVE_GHOST.lock() = Some(stop);
        std::thread::Builder::new()
            .name("sys-dnd-ghost".into())
            .spawn(move || run_ghost(&title, count, thumbnail.as_deref(), &stopped))
            .ok();
    }

    /// Dismisses the ghost window at once (the worker thread notices within the same moment).
    pub fn cancel_ghost_overlay() {
        if let Some(stop) = ACTIVE_GHOST.lock().take() {
            let _ = stop.send(());
        }
    }
}

fn run_ghost(title: &str, count: usize, thumbnail: Option<&Path>, stopped: &Receiver<()>) {
    let spec = GhostSpec {
        title,
        count,
        thumbnail,
    };
    let mut window = match GhostWindow::open(|compositor| ghost_image::render(&spec, !compositor), POINTER_OFFSET) {
        Ok(window) => window,
        Err(error) => {
            tracing::debug!(%error, "[SystemDnd] No drag ghost window");
            return;
        }
    };
    let deadline = Instant::now() + MAX_LIFETIME;
    // The window closes with the first signal, with a dropped sender (a newer drag replaced this one), at the deadline or when the
    // X connection breaks.
    while matches!(stopped.recv_timeout(FRAME), Err(RecvTimeoutError::Timeout))
        && Instant::now() < deadline
        && window.follow_pointer()
    {}
}
