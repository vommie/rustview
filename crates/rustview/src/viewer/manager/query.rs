//! Read access to the open tabs.

use super::ViewerTabManager;
use crate::viewer::tab_item::TabTitleFormatter;
use crate::viewer::types::{MediaTabSession, TabId};
use rustview_core::library::models::MediaId;

impl ViewerTabManager {
    #[inline]
    pub fn tab_count(&self) -> usize {
        self.tabs.read().len()
    }

    #[inline]
    pub fn active_tab_id(&self) -> Option<TabId> {
        *self.active_tab_id.read()
    }

    pub fn get_active_tab(&self) -> Option<MediaTabSession> {
        let active = (*self.active_tab_id.read())?;
        self.tabs.read().get(active).cloned()
    }

    /// The one medium of the active tab; `None` for the Browse view and for a Multiview tab.
    pub fn get_active_media_id(&self) -> Option<MediaId> {
        self.get_active_tab().and_then(|t| t.single_media_id())
    }

    /// Every medium the active tab shows (the tiles of a Multiview tab); empty for the Browse view.
    pub fn get_active_media_ids(&self) -> Vec<MediaId> {
        self.get_active_tab().map(|t| t.member_ids()).unwrap_or_default()
    }

    /// The tab that owns the Multiview instance `instance_id`.
    pub fn tab_id_of_instance(&self, instance_id: u64) -> Option<TabId> {
        self.tabs.read().get_by_instance_id(instance_id).map(|t| t.id)
    }

    pub fn get_tab(&self, tab_id: TabId) -> Option<MediaTabSession> {
        self.tabs.read().get(tab_id).cloned()
    }

    pub fn get_tab_index(&self, tab_id: TabId) -> Option<usize> {
        self.tabs.read().index_of(tab_id)
    }

    pub fn get_tab_by_media_id(&self, media_id: MediaId) -> Option<MediaTabSession> {
        self.tabs.read().get_by_media_id(media_id).cloned()
    }

    #[inline]
    pub fn get_by_media_id(&self, media_id: MediaId) -> Option<MediaTabSession> {
        self.get_tab_by_media_id(media_id)
    }

    pub fn get_tabs_by_media_id(&self, media_id: MediaId) -> Vec<MediaTabSession> {
        self.tabs.read().get_all_by_media_id(media_id)
    }

    /// Single Point of Truth delegating tab title formatting to `TabTitleFormatter`.
    #[inline]
    pub fn compute_tab_title(tabs: &[MediaTabSession], tab_id: TabId) -> String {
        TabTitleFormatter::compute_title(tabs, tab_id)
    }

    #[inline]
    pub(super) fn sessions_count(&self) -> usize {
        self.tabs.read().len()
    }
}
