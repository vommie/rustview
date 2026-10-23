//! Which tags the selected media carry (all, some or none of them).

use rustview_core::library::Library;
use rustview_core::library::models::{MediaId, TagId};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Default)]
pub struct TagSelectionStats {
    pub selected_files_count: usize,
    pub tag_counts: HashMap<TagId, usize>,
    pub ratings_in_selection: HashSet<u8>,
}

impl TagSelectionStats {
    pub fn compute(selected_ids: &HashSet<MediaId>, library: &Library) -> Self {
        let count = selected_ids.len();
        if count == 0 {
            return Self::default();
        }

        let media_slice: Vec<MediaId> = selected_ids.iter().copied().collect();
        let tag_counts = library.tags().get_tag_frequencies(&media_slice).unwrap_or_default();

        let mut ratings = HashSet::new();
        for &id in selected_ids {
            if let Ok(Some(m)) = library.media().get(id) {
                ratings.insert(m.rating);
            }
        }

        Self {
            selected_files_count: count,
            tag_counts,
            ratings_in_selection: ratings,
        }
    }

    #[inline]
    pub fn is_fully_assigned(&self, tag_id: TagId) -> bool {
        self.selected_files_count > 0 && self.assigned_count(tag_id) == self.selected_files_count
    }

    #[inline]
    pub fn is_partially_assigned(&self, tag_id: TagId) -> bool {
        let count = self.assigned_count(tag_id);
        self.selected_files_count > 1 && count > 0 && count < self.selected_files_count
    }

    #[inline]
    pub fn assigned_count(&self, tag_id: TagId) -> usize {
        self.tag_counts.get(&tag_id).copied().unwrap_or(0)
    }

    pub fn format_display_name(&self, raw_name: &str, tag_id: TagId) -> String {
        let count = self.assigned_count(tag_id);
        if self.selected_files_count > 1 && count > 0 {
            format!("{raw_name} ({count})")
        } else {
            raw_name.to_string()
        }
    }
}
