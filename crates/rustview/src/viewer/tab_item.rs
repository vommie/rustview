use super::types::{MediaTabSession, TabId};
use rustview_core::library::models::{MediaId, MediaRecord};
use std::collections::HashSet;

/// Centralized, single point of truth (SPoT) formatter for tab titles and duplicate iteration suffixes.
pub struct TabTitleFormatter;

impl TabTitleFormatter {
    /// Allocates a unique, permanent title for a new tab session referencing `media_id`.
    /// Reuses the base filename if not already taken; otherwise finds the lowest available `(1)`, `(2)`, etc.
    pub fn allocate_tab_title(
        existing_tabs: &[MediaTabSession],
        media_id: MediaId,
        base_filename: &str,
    ) -> String {
        let taken_titles: HashSet<&str> = existing_tabs
            .iter()
            .filter(|t| t.media_id == media_id)
            .map(|t| t.title.as_str())
            .collect();

        if !taken_titles.contains(base_filename) {
            return base_filename.to_string();
        }

        let mut counter = 1usize;
        loop {
            let candidate = format!("{} ({})", base_filename, counter);
            if !taken_titles.contains(candidate.as_str()) {
                return candidate;
            }
            counter += 1;
        }
    }

    /// Retrieves the permanent title of a tab by its `TabId`.
    #[inline]
    pub fn compute_title(tabs: &[MediaTabSession], tab_id: TabId) -> String {
        tabs.iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.title.clone())
            .unwrap_or_default()
    }
}

/// Encapsulated, ordered collection managing `MediaTabSession` entities.
/// Provides pure in-memory query, mutation, movement, and reordering operations (SRP).
#[derive(Debug, Default, Clone)]
pub struct TabCollection {
    sessions: Vec<MediaTabSession>,
}

impl TabCollection {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
        }
    }

    // =========================================================================
    // Queries
    // =========================================================================

    #[inline]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    #[inline]
    pub fn all(&self) -> &[MediaTabSession] {
        &self.sessions
    }

    #[inline]
    pub fn to_vec(&self) -> Vec<MediaTabSession> {
        self.sessions.clone()
    }

    pub fn get(&self, tab_id: TabId) -> Option<&MediaTabSession> {
        self.sessions.iter().find(|t| t.id == tab_id)
    }

    pub fn get_mut(&mut self, tab_id: TabId) -> Option<&mut MediaTabSession> {
        self.sessions.iter_mut().find(|t| t.id == tab_id)
    }

    pub fn get_at(&self, index: usize) -> Option<&MediaTabSession> {
        self.sessions.get(index)
    }

    pub fn index_of(&self, tab_id: TabId) -> Option<usize> {
        self.sessions.iter().position(|t| t.id == tab_id)
    }

    pub fn get_by_media_id(&self, media_id: MediaId) -> Option<&MediaTabSession> {
        self.sessions.iter().find(|t| t.media_id == media_id)
    }

    pub fn get_by_media_id_mut(&mut self, media_id: MediaId) -> Option<&mut MediaTabSession> {
        self.sessions.iter_mut().find(|t| t.media_id == media_id)
    }

    pub fn get_all_by_media_id(&self, media_id: MediaId) -> Vec<MediaTabSession> {
        self.sessions
            .iter()
            .filter(|t| t.media_id == media_id)
            .cloned()
            .collect()
    }

    // =========================================================================
    // Mutations & Lifecycle
    // =========================================================================

    pub fn push(&mut self, session: MediaTabSession) {
        self.sessions.push(session);
    }

    /// Inserts a new session immediately after `after_tab_id` (used for tab duplication).
    pub fn insert_after(&mut self, after_tab_id: TabId, session: MediaTabSession) -> usize {
        let insert_idx = self
            .index_of(after_tab_id)
            .map(|idx| idx + 1)
            .unwrap_or(self.sessions.len());

        let clamped_idx = insert_idx.min(self.sessions.len());
        self.sessions.insert(clamped_idx, session);
        clamped_idx
    }

    /// Removes a tab session by its `TabId` and returns the removed session and its original 0-based index.
    pub fn remove(&mut self, tab_id: TabId) -> Option<(usize, MediaTabSession)> {
        let pos = self.index_of(tab_id)?;
        let removed = self.sessions.remove(pos);
        Some((pos, removed))
    }

    /// Clears all sessions and returns them for resource cleanup.
    pub fn clear(&mut self) -> Vec<MediaTabSession> {
        std::mem::take(&mut self.sessions)
    }

    /// Retains only the tab matching `keep_tab_id` and returns the evicted sessions for cleanup.
    pub fn retain_only(&mut self, keep_tab_id: TabId) -> Vec<MediaTabSession> {
        let mut evicted = Vec::new();
        let mut kept = Vec::new();

        for session in self.sessions.drain(..) {
            if session.id == keep_tab_id {
                kept.push(session);
            } else {
                evicted.push(session);
            }
        }

        self.sessions = kept;
        evicted
    }

    /// Removes all tabs positioned to the left of `tab_id` and returns the evicted sessions.
    pub fn remove_to_left(&mut self, tab_id: TabId) -> Vec<MediaTabSession> {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        if pos == 0 {
            return Vec::new();
        }

        self.sessions.drain(0..pos).collect()
    }

    /// Removes all tabs positioned to the right of `tab_id` and returns the evicted sessions.
    pub fn remove_to_right(&mut self, tab_id: TabId) -> Vec<MediaTabSession> {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return Vec::new(),
        };

        if pos + 1 >= self.sessions.len() {
            return Vec::new();
        }

        self.sessions.drain((pos + 1)..).collect()
    }

    // =========================================================================
    // Reordering & Movement Algorithms (Retains Permanent Tab Titles)
    // =========================================================================

    /// Relocates a tab to a target index position in the list without mutating its permanent title.
    pub fn move_tab(&mut self, tab_id: TabId, target_index: usize) -> bool {
        let current_pos = match self.index_of(tab_id) {
            Some(pos) => pos,
            None => return false,
        };

        let clamped_target = target_index.min(self.sessions.len().saturating_sub(1));
        if current_pos == clamped_target {
            return false;
        }

        let item = self.sessions.remove(current_pos);
        self.sessions.insert(clamped_target, item);
        true
    }

    pub fn move_to_start(&mut self, tab_id: TabId) -> bool {
        self.move_tab(tab_id, 0)
    }

    pub fn move_left(&mut self, tab_id: TabId) -> bool {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos > 0 {
            self.move_tab(tab_id, pos - 1)
        } else {
            false
        }
    }

    pub fn move_right(&mut self, tab_id: TabId) -> bool {
        let pos = match self.index_of(tab_id) {
            Some(p) => p,
            None => return false,
        };
        if pos + 1 < self.sessions.len() {
            self.move_tab(tab_id, pos + 1)
        } else {
            false
        }
    }

    pub fn move_to_end(&mut self, tab_id: TabId) -> bool {
        let count = self.sessions.len();
        if count > 0 {
            self.move_tab(tab_id, count - 1)
        } else {
            false
        }
    }

    // =========================================================================
    // Synchronous Reactive State Updates
    // =========================================================================

    pub fn update_filename(&mut self, media_id: MediaId, new_filename: &str) {
        for tab in self.sessions.iter_mut() {
            let mut guard = tab.multiview.instance.lock();
            guard.member_renamed(media_id, new_filename);
            drop(guard);
            // A tab of one medium is described by that medium and follows the file's new name (its title keeps the suffix of
            // a numbered duplicate).
            if tab.media_id == media_id {
                let suffix = tab.title.strip_prefix(&tab.filename).unwrap_or("").to_string();
                tab.title = format!("{new_filename}{suffix}");
                tab.filename = new_filename.to_string();
                if let Some(parent) = tab.file_path.parent() {
                    tab.file_path = parent.join(new_filename);
                }
                tab.record.filename = new_filename.to_string();
            }
        }
    }

    pub fn mark_deleted(&mut self, media_id: MediaId) {
        for tab in self.sessions.iter_mut() {
            if tab.media_id == media_id {
                tab.record.is_deleted = true;
            }
            tab.multiview.instance.lock().member_deleted(media_id);
        }
    }

    pub fn update_record(&mut self, media_id: MediaId, updated_record: &MediaRecord) {
        for tab in self.sessions.iter_mut() {
            if tab.media_id == media_id {
                tab.record = updated_record.clone();
            }
            tab.multiview.instance.lock().member_refreshed(media_id, updated_record);
        }
    }

    pub fn get_by_instance_id(&self, instance_id: u64) -> Option<&MediaTabSession> {
        self.sessions.iter().find(|t| t.multiview.instance_id == instance_id)
    }
}
