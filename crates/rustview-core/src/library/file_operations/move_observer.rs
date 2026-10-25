//! Who wants to know that a media file moved on the drive (a rename or a move to another folder): the application registers
//! one observer at startup (the A-B repeats of the mpv script `ab_repeat.lua` are saved under the file's path and go along).
//! Called right after the file was moved, on the thread that moved it; the observer must only hand the paths on.

use std::path::Path;
use std::sync::OnceLock;

type Observer = Box<dyn Fn(&Path, &Path) + Send + Sync>;

static OBSERVER: OnceLock<Observer> = OnceLock::new();

/// Registers the observer (once; a second registration is ignored).
pub fn set_move_observer(observer: impl Fn(&Path, &Path) + Send + Sync + 'static) {
    let _ = OBSERVER.set(Box::new(observer));
}

pub(super) fn notify(from: &Path, to: &Path) {
    if let Some(observer) = OBSERVER.get() {
        observer(from, to);
    }
}
