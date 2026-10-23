//! Editing pictures in the viewer (cropping now; later rotating and more): the state of a tile that is edited, the one-edit-per-
//! file rule, the order Esc closes crop tools in, the life of the Save button and the question before closing unsaved pictures.
//! Everything here is pure or plain state; the Multiview connects it to its tiles (`multiview::instance_edit`) and the UI
//! layer saves (`ui::image_edit`). The decisions about pictures and files themselves live in `rustview_core::image_edit`.

pub mod claims;
pub mod prefs;
pub mod save_badge;
pub mod tile_edit;
pub mod tool_order;
pub mod unsaved;

use std::sync::atomic::{AtomicU64, Ordering};

static UNSAVED_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Moves whenever a picture may have become dirty or clean (the tab bar looks at its dots only then).
pub fn unsaved_epoch() -> u64 {
    UNSAVED_EPOCH.load(Ordering::Relaxed)
}

pub fn touch_unsaved() {
    UNSAVED_EPOCH.fetch_add(1, Ordering::Relaxed);
}
