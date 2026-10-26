//! Activating and closing tabs, and the orderly shutdown of all of them.

use super::ViewerTabManager;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use crate::viewer::multiview::teardown;
use crate::viewer::tab_item::TabCollection;
use crate::viewer::types::{MediaTabSession, TabId};
use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::settings::schema::SHUTDOWN_TEARDOWN_WAIT_MS;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl ViewerTabManager {
    pub fn set_active_tab_internal(&self, tab_id: TabId, force_resume: bool) -> bool {
        let current_active = *self.active_tab_id.read();

        let mut tabs = self.tabs.write();

        if current_active == Some(tab_id) && !force_resume {
            return true;
        }

        if let Some(prev_id) = current_active
            && prev_id != tab_id
            && let Some(prev_tab) = tabs.get_mut(prev_id)
        {
            Self::pause_session_playback(prev_tab);
        }

        if let Some(new_tab) = tabs.get_mut(tab_id) {
            new_tab.last_accessed = Instant::now();
            *self.active_tab_id.write() = Some(tab_id);
            tracing::info!(tab_id, "[ViewerTabManager: Switch] Activated viewer tab");
            true
        } else {
            false
        }
    }

    pub fn activate_browse(&self) {
        let mut tabs = self.tabs.write();
        if let Some(current_id) = *self.active_tab_id.read()
            && let Some(current_tab) = tabs.get_mut(current_id)
        {
            Self::pause_session_playback(current_tab);
        }
        *self.active_tab_id.write() = None;
        tracing::info!("[ViewerTabManager: Browse] Returned to library media browser");
    }

    pub fn close_tab(&self, tab_id: TabId) -> Option<TabId> {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let (pos, removed) = tabs.remove(tab_id)?;
        let removed_id = removed.id;
        Self::cleanup_session_resources(&removed);

        tracing::info!(
            tab_id = removed_id,
            remaining = tabs.len(),
            "[ViewerTabManager: Close] Closed tab session"
        );

        if *active == Some(tab_id) {
            *active = Self::resolve_next_active_tab(&mut tabs, pos);
        }

        *active
    }

    pub fn close_tabs_to_left(&self, tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let current_active = *active;
        let evicted = tabs.remove_to_left(tab_id);
        let closed_count = evicted.len();

        if closed_count == 0 {
            return (0, current_active);
        }

        let evicted_ids: HashSet<TabId> = evicted.iter().map(|t| t.id).collect();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        tracing::info!(
            target_tab_id = tab_id,
            closed_count,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: CloseToLeft] Closed all tabs to the left of target"
        );

        if let Some(active_id) = current_active
            && evicted_ids.contains(&active_id)
        {
            *active = Some(tab_id);
            if let Some(target_tab) = tabs.get_mut(tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        (closed_count, *active)
    }

    pub fn close_tabs_to_right(&self, tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let current_active = *active;
        let evicted = tabs.remove_to_right(tab_id);
        let closed_count = evicted.len();

        if closed_count == 0 {
            return (0, current_active);
        }

        let evicted_ids: HashSet<TabId> = evicted.iter().map(|t| t.id).collect();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        tracing::info!(
            target_tab_id = tab_id,
            closed_count,
            remaining_tabs = tabs.len(),
            "[ViewerTabManager: CloseToRight] Closed all tabs to the right of target"
        );

        if let Some(active_id) = current_active
            && evicted_ids.contains(&active_id)
        {
            *active = Some(tab_id);
            if let Some(target_tab) = tabs.get_mut(tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        (closed_count, *active)
    }

    pub fn close_other_tabs(&self, keep_tab_id: TabId) -> (usize, Option<TabId>) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let evicted = tabs.retain_only(keep_tab_id);
        let closed_count = evicted.len();

        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        if tabs.is_empty() {
            *active = None;
        } else {
            *active = Some(keep_tab_id);
            if let Some(target_tab) = tabs.get_mut(keep_tab_id) {
                target_tab.last_accessed = Instant::now();
            }
        }

        tracing::info!(
            keep_tab_id,
            closed_count,
            "[ViewerTabManager: CloseOthers] Closed all other tabs, keeping single active tab"
        );

        (closed_count, *active)
    }

    pub fn close_all_tabs(&self) {
        let mut tabs = self.tabs.write();
        let mut active = self.active_tab_id.write();

        let evicted = tabs.clear();
        for session in evicted {
            Self::cleanup_session_resources(&session);
        }

        *active = None;
        tracing::info!("[ViewerTabManager: CloseAll] Closed all open viewer tabs");
    }

    /// The application exits: every tab is closed and its native resources are destroyed on the calling thread, in the safe
    /// order (a player is shut down completely before the window it rendered into is destroyed). Blocks until everything is
    /// gone, including the teardowns that closed Multiviews left to worker threads.
    pub fn shutdown(&self) {
        let sessions = {
            let mut tabs = self.tabs.write();
            *self.active_tab_id.write() = None;
            tabs.clear()
        };
        self.observed_instances.lock().clear();
        let count = sessions.len();
        for session in sessions {
            Self::shutdown_session(&session);
        }
        let settled = teardown::wait_for_teardowns(Duration::from_millis(SHUTDOWN_TEARDOWN_WAIT_MS));
        tracing::info!(
            tabs = count,
            settled,
            "[ViewerTabManager: Shutdown] All tabs closed and their players shut down"
        );
    }

    pub(super) fn shutdown_session(session: &MediaTabSession) {
        session.multiview.instance.lock().shutdown_now();
    }

    // =========================================================================
    // Reactive Event Synchronization
    // =========================================================================

    /// Lets library events (rename, delete, refresh) reach a Multiview that no tab owns. Dead references are dropped.
    pub fn observe_instance(&self, instance: &SharedInstance) {
        let mut observed = self.observed_instances.lock();
        observed.retain(|weak| weak.strong_count() > 0);
        let id = instance.lock().id();
        if !observed
            .iter()
            .filter_map(WeakInstance::upgrade)
            .any(|other| other.lock().id() == id)
        {
            observed.push(Arc::downgrade(instance));
        }
    }

    /// The Multiview with `instance_id` is owned by a tab now (or gone), so the tabs deliver its events.
    pub fn stop_observing_instance(&self, instance_id: u64) {
        self.observed_instances
            .lock()
            .retain(|weak| weak.upgrade().is_some_and(|other| other.lock().id() != instance_id));
    }

    /// Every Multiview the manager knows: the ones of the tabs and the observed ones (the fullscreen that follows the Browse view).
    pub fn all_instances(&self) -> Vec<SharedInstance> {
        let mut instances: Vec<SharedInstance> = self
            .tabs
            .read()
            .all()
            .iter()
            .map(|t| t.multiview.instance.clone())
            .collect();
        instances.extend(self.observed_instances.lock().iter().filter_map(WeakInstance::upgrade));
        instances
    }

    pub(super) fn for_each_observed(&self, mut apply: impl FnMut(&mut MultiviewInstance)) {
        let instances: Vec<SharedInstance> = self
            .observed_instances
            .lock()
            .iter()
            .filter_map(WeakInstance::upgrade)
            .collect();
        for instance in instances {
            apply(&mut instance.lock());
        }
    }

    pub fn notify_media_renamed(&self, media_id: MediaId, new_filename: &str) {
        self.tabs.write().update_filename(media_id, new_filename);
        self.for_each_observed(|mv| mv.member_renamed(media_id, new_filename));
    }

    pub fn notify_media_deleted(&self, media_id: MediaId) {
        self.tabs.write().mark_deleted(media_id);
        self.for_each_observed(|mv| mv.member_deleted(media_id));
    }

    pub fn notify_media_refreshed(&self, media_id: MediaId, updated_record: &MediaRecord) {
        self.tabs.write().update_record(media_id, updated_record);
        self.for_each_observed(|mv| mv.member_refreshed(media_id, updated_record));
    }

    /// The file of `media_id` was refreshed (it may have changed on disk): every tab and observed Multiview that shows it decodes the
    /// picture or loads the video again.
    pub fn reload_media(&self, media_id: MediaId) {
        let instances: Vec<SharedInstance> = self
            .tabs
            .read()
            .all()
            .iter()
            .map(|t| t.multiview.instance.clone())
            .collect();
        for instance in instances {
            instance.lock().reload_member(media_id);
        }
        self.for_each_observed(|mv| mv.reload_member(media_id));
    }

    // =========================================================================
    // Internal Helper Methods
    // =========================================================================

    pub(super) fn pause_session_playback(tab: &mut MediaTabSession) {
        tab.deactivate();
    }

    /// The tab is closed: playback stops and the windows disappear. The Multiview shuts its players down before their windows are
    /// destroyed (see `teardown`; dropping the tab would destroy a window under a running player and end the application).
    pub(super) fn cleanup_session_resources(session: &MediaTabSession) {
        session.stop();
    }

    /// The tab that is shown after the tab at `closed_index` was closed: the one that took its place, else the one before.
    pub(super) fn resolve_next_active_tab(tabs: &mut TabCollection, closed_index: usize) -> Option<TabId> {
        if tabs.is_empty() {
            return None;
        }
        let next_idx = if closed_index < tabs.len() {
            closed_index
        } else {
            closed_index.saturating_sub(1)
        };
        tabs.get_at(next_idx).map(|tab| tab.id)
    }
}
