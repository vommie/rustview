//! The keys of a Multiview that act on one medium, on the UI side (UI thread only).
//!
//! Every key source (the fullscreen window, the main window for the tab view, the bar windows) runs its key as a
//! `MultiviewCommand` on the instance: the target is the selected medium (`instance_commands`), a refusal shows the hint with
//! its reason, and what the application still has to do (a dialog, saving) is handed back to the caller, which carries it out
//! without holding any session borrow.

use super::hint;
use crate::viewer::multiview::command::MultiviewCommand;
use crate::viewer::multiview::hint::{HintAnchor, HintKind};
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::route::UiAction;
use rustview_core::library::models::MediaId;
use std::time::Instant;

/// Runs `command` on `instance`; returns the action that follows (`None` after a refusal, whose hint is shown).
pub(super) fn run(instance: &SharedInstance, command: MultiviewCommand) -> UiAction {
    let (result, anchor) = {
        let mut guard = instance.lock();
        (guard.run_command(command), guard.hint_anchor())
    };
    match result {
        Ok(action) => action,
        Err(kind) => {
            refused(anchor, kind);
            UiAction::None
        }
    }
}

/// The selected medium of `instance` for a key the host carries out itself (rating, refreshing, F2 in the tab view), or `None`
/// after the hint was shown.
pub(super) fn target_media(instance: &SharedInstance) -> Option<MediaId> {
    let (target, anchor) = {
        let mut guard = instance.lock();
        (guard.command_target(Instant::now()), guard.hint_anchor())
    };
    match target {
        Ok((_, media)) => Some(media),
        Err(kind) => {
            refused(anchor, kind);
            None
        }
    }
}

/// A key did nothing: the hint says why, centered over the view (nothing while no host shows the Multiview).
pub(super) fn refused(anchor: Option<HintAnchor>, kind: HintKind) {
    tracing::info!(?kind, "[Multiview: Keys] Refused; the hint says why");
    if let Some(anchor) = anchor {
        hint::show(anchor, kind);
    }
}
