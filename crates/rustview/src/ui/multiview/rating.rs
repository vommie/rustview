//! Ratings given with the rating keys (Settings > Hotkeys) in a Multiview (fullscreen, viewer tab, tab shown fullscreen).
//!
//! The key arrives from the fullscreen window or from the main window (a viewer tab); a key pressed over a video reaches the
//! window under it (mpv takes no keys). All of them end in [`MultiviewHandle::rate_visible`]: it rates the selected medium of the Multiview (`focus`,
//! wherever the pointer is) through the one path of the Tag & Rate panel and the context menu (`BridgeTags::apply_rating_to_ids`),
//! then starts the feedback pulse on the tiles of that medium. With several tiles and none selected it shows the hint instead.

use super::handle::MultiviewHandle;
use super::session;
use crate::ui::rating_hotkey::{rateable_ids, standing_of};
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::viewer::multiview::hint::HintKind;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::prefs;
use crate::viewer::multiview::route::HostMode;
use std::time::Instant;

impl MultiviewHandle {
    /// Rates what the open fullscreen Multiview shows.
    pub fn rate_fullscreen(&self, rating: u8) {
        if let Some(instance) = session::instance() {
            self.rate_visible(&instance, rating);
        }
    }

    /// Rates the selected medium of `instance` (the only one of a single tile) and lets every tile of it show the new rating.
    /// With several tiles and none selected nothing is rated and the hint asks for a selection. Ignored while a dialog has the
    /// main window locked or a gutter is being dragged. UI thread only.
    pub fn rate_visible(&self, instance: &SharedInstance, rating: u8) {
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        if window.get_is_modal_locked() {
            tracing::debug!(rating, "[Multiview: Rating] Ignored: a dialog is open");
            return;
        }
        let (mode, dragging, host) = {
            let guard = instance.lock();
            (guard.host_mode(), guard.is_dragging(), guard.hint_host())
        };
        if dragging || !matches!(mode, HostMode::Fullscreen | HostMode::TabView) {
            tracing::debug!(rating, ?mode, dragging, "[Multiview: Rating] Ignored");
            return;
        }
        let Some(target) = super::commands::target_media(instance) else {
            return;
        };
        let ids = {
            let state = app.state.read();
            rateable_ids(vec![target], |id| standing_of(&state, &app.library, id))
        };
        if ids.is_empty() {
            super::commands::refused(host, HintKind::NotRateable);
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
            "[Multiview: Rating] Rated the selected medium"
        );
        if rated.is_empty() || !prefs::current().rating_feedback {
            return;
        }
        instance.lock().show_rating_feedback(&rated, rating, Instant::now());
        self.feedback_started(instance);
    }
}
