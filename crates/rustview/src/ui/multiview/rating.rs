//! Ratings given with Ctrl+0..5 in a Multiview (fullscreen, viewer tab, tab shown fullscreen).
//!
//! The key arrives from the fullscreen window, from the main window (a viewer tab) or from a video's own window (mpv client
//! message). All of them end in [`MultiviewHandle::rate_visible`]: it rates every medium the Multiview shows through the one
//! path of the Tag & Rate panel and the context menu (`BridgeTags::apply_rating_to_ids`), then starts the feedback pulse on the
//! tiles of the media that were rated (`feedback`).

use super::handle::MultiviewHandle;
use super::session;
use crate::ui::rating_hotkey::{rateable_ids, standing_of};
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::prefs;
use crate::viewer::multiview::rating_keys::rating_of_key;
use crate::viewer::multiview::route::HostMode;
use std::time::Instant;

impl MultiviewHandle {
    /// A rating key in the fullscreen window. Returns whether `text` was a rating key.
    pub(super) fn fullscreen_key(&self, text: &str, modifiers: (bool, bool, bool), repeat: bool) -> bool {
        let (ctrl, alt, meta) = modifiers;
        let Some(rating) = rating_of_key(text, ctrl, alt, meta, false) else {
            return false;
        };
        if !repeat {
            self.rate_fullscreen(rating);
        }
        true
    }

    /// Rates what the open fullscreen Multiview shows.
    pub fn rate_fullscreen(&self, rating: u8) {
        if let Some(instance) = session::instance() {
            self.rate_visible(&instance, rating);
        }
    }

    /// Rates every medium `instance` shows (also sticky tiles and tiles the Browse view has not selected; a medium on several
    /// tiles once) and lets every tile of a rated medium show the new rating. Ignored while a dialog has the main window locked
    /// or a gutter is being dragged. UI thread only.
    pub fn rate_visible(&self, instance: &SharedInstance, rating: u8) {
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        if window.get_is_modal_locked() {
            tracing::debug!(rating, "[Multiview: Rating] Ignored: a dialog is open");
            return;
        }
        let (media, mode, dragging) = {
            let guard = instance.lock();
            (guard.rating_targets(), guard.host_mode(), guard.is_dragging())
        };
        if dragging || !matches!(mode, HostMode::Fullscreen | HostMode::TabView) {
            tracing::debug!(rating, ?mode, dragging, "[Multiview: Rating] Ignored");
            return;
        }
        let ids = {
            let state = app.state.read();
            rateable_ids(media, |id| standing_of(&state, &app.library, id))
        };
        if ids.is_empty() {
            tracing::debug!(rating, "[Multiview: Rating] Nothing to rate");
            return;
        }
        let rated = BridgeTags::apply_rating_to_ids(
            &ids,
            rating,
            &TagActionContext {
                window: &window,
                library: &app.library,
                thumbnails: &app.thumbnails,
                tab_manager: &app.tab_manager,
                settings: &app.settings,
                state: &app.state,
            },
        );
        tracing::info!(
            rating,
            media = rated.len(),
            ?mode,
            "[Multiview: Rating] Rated the visible media"
        );
        if rated.is_empty() || !prefs::current().rating_feedback {
            return;
        }
        instance.lock().show_rating_feedback(&rated, rating, Instant::now());
        self.feedback_started(instance);
    }
}
