//! Tells whether the tab UI (tab bar, panels) shows the tabs that exist.
//!
//! Every place that creates a tab bumps the revision; the UI marks the revision it has rendered. A created tab that the UI
//! has not rendered yet is "stale", so no creation path (foreground, background, duplicate, a Multiview moved into a tab)
//! can leave a tab that exists but is not visible: whoever notices the staleness renders the tab list again.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct TabUiGate {
    revision: AtomicU64,
    synced: AtomicU64,
}

impl TabUiGate {
    /// A tab was created. Returns the new revision.
    pub fn changed(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::SeqCst)
    }

    /// The UI rendered every tab up to `revision` (a late, older report never lowers what is known).
    pub fn mark_synced(&self, revision: u64) {
        self.synced.fetch_max(revision, Ordering::SeqCst);
    }

    /// True while a tab exists that the UI has not rendered yet.
    pub fn is_stale(&self) -> bool {
        self.synced.load(Ordering::SeqCst) < self.revision.load(Ordering::SeqCst)
    }
}
