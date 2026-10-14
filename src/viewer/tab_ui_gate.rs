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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_gate_has_nothing_to_render() {
        assert!(!TabUiGate::default().is_stale());
    }

    #[test]
    fn a_created_tab_is_stale_until_the_ui_has_rendered_it() {
        let gate = TabUiGate::default();
        let revision = gate.changed();
        assert!(gate.is_stale());
        gate.mark_synced(revision);
        assert!(!gate.is_stale());
    }

    #[test]
    fn a_tab_created_while_the_ui_renders_stays_stale() {
        let gate = TabUiGate::default();
        gate.changed();
        let seen_by_the_ui = gate.revision();
        gate.changed();
        gate.mark_synced(seen_by_the_ui);
        assert!(gate.is_stale(), "the second tab was not part of that render");
        gate.mark_synced(gate.revision());
        assert!(!gate.is_stale());
    }

    #[test]
    fn an_older_report_never_makes_the_gate_stale_again_or_hides_a_tab() {
        let gate = TabUiGate::default();
        let first = gate.changed();
        let second = gate.changed();
        gate.mark_synced(second);
        gate.mark_synced(first);
        assert!(!gate.is_stale());
        gate.changed();
        assert!(gate.is_stale());
    }
}
