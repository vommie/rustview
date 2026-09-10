use crate::library::models::{MediaId, MediaRecord};
use crate::media::MediaFormat;
use std::collections::{HashMap, HashSet};

/// Thread-safe file selection coordinator implementing standard OS file manager selection behaviors.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileSelectionModel {
    selected_ids: HashSet<MediaId>,
    last_clicked_id: Option<MediaId>,
    anchor_id: Option<MediaId>,
}

impl FileSelectionModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Computes the symmetric difference of changed IDs between two selection states.
    #[inline]
    pub fn compute_selection_diff(
        prev: &HashSet<MediaId>,
        current: &HashSet<MediaId>,
    ) -> Vec<MediaId> {
        let mut changed = Vec::new();
        for &id in prev {
            if !current.contains(&id) {
                changed.push(id);
            }
        }
        for &id in current {
            if !prev.contains(&id) {
                changed.push(id);
            }
        }
        changed
    }

    /// Selects only the specified media ID, clearing any previous selection.
    pub fn select_single(&mut self, id: MediaId) {
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.last_clicked_id = Some(id);
        self.anchor_id = Some(id);
    }

    /// Toggles the selection state of the specified ID (Ctrl + Click).
    pub fn toggle(&mut self, id: MediaId) {
        if self.selected_ids.contains(&id) {
            self.selected_ids.remove(&id);
        } else {
            self.selected_ids.insert(id);
        }
        self.last_clicked_id = Some(id);
        self.anchor_id = Some(id);
    }

    /// Deselects a specific subset of media IDs while retaining the rest of the active selection.
    pub fn deselect_many(&mut self, ids: &HashSet<MediaId>) {
        self.selected_ids.retain(|id| !ids.contains(id));
        if let Some(anchor) = self.anchor_id
            && ids.contains(&anchor)
        {
            self.anchor_id = None;
        }
        if let Some(last) = self.last_clicked_id
            && ids.contains(&last)
        {
            self.last_clicked_id = None;
        }
    }

    /// Selects a contiguous range between the anchor and the target ID (Shift + Click / Shift + Arrow).
    /// If `additive` is true (Ctrl + Shift), existing selections outside the range are preserved.
    pub fn select_range(
        &mut self,
        target_id: MediaId,
        ordered_visible_ids: &[MediaId],
        additive: bool,
    ) {
        if ordered_visible_ids.is_empty() {
            return;
        }

        let anchor = self.anchor_id.unwrap_or(target_id);
        if self.anchor_id.is_none() {
            self.anchor_id = Some(anchor);
        }

        let anchor_idx = ordered_visible_ids
            .iter()
            .position(|&id| id == anchor)
            .unwrap_or(0);

        let target_idx = ordered_visible_ids
            .iter()
            .position(|&id| id == target_id)
            .unwrap_or(0);

        let start = anchor_idx.min(target_idx);
        let end = anchor_idx.max(target_idx);

        if !additive {
            self.selected_ids.clear();
        }

        for &id in &ordered_visible_ids[start..=end] {
            self.selected_ids.insert(id);
        }

        self.last_clicked_id = Some(target_id);
    }

    /// Selects all visible media items.
    pub fn select_all(&mut self, visible_ids: &[MediaId]) {
        self.selected_ids = visible_ids.iter().copied().collect();
    }

    /// Inverts selection across visible items.
    pub fn invert(&mut self, visible_ids: &[MediaId]) {
        let mut new_set = HashSet::with_capacity(visible_ids.len());
        for &id in visible_ids {
            if !self.selected_ids.contains(&id) {
                new_set.insert(id);
            }
        }
        self.selected_ids = new_set;
    }

    /// Sets the selection to an exact set of media IDs.
    pub fn set_selection(&mut self, ids: HashSet<MediaId>) {
        self.selected_ids = ids;
    }

    /// Clears the entire file selection state.
    pub fn clear(&mut self) {
        self.selected_ids.clear();
        self.last_clicked_id = None;
        self.anchor_id = None;
    }

    /// Checks if a media item is currently selected.
    #[inline]
    pub fn is_selected(&self, id: MediaId) -> bool {
        self.selected_ids.contains(&id)
    }

    /// Returns the number of selected files.
    #[inline]
    pub fn count(&self) -> usize {
        self.selected_ids.len()
    }

    /// Returns a borrowed reference to the selected media IDs.
    #[inline]
    pub fn selected_ids(&self) -> &HashSet<MediaId> {
        &self.selected_ids
    }

    /// Returns the active anchor item for range selections.
    #[inline]
    pub fn anchor_id(&self) -> Option<MediaId> {
        self.anchor_id
    }

    /// Explicitly sets the anchor item.
    #[inline]
    pub fn set_anchor_id(&mut self, id: Option<MediaId>) {
        self.anchor_id = id;
    }

    /// Returns the last clicked or focused file ID.
    #[inline]
    pub fn last_clicked_id(&self) -> Option<MediaId> {
        self.last_clicked_id
    }

    /// Explicitly sets the last clicked or focused file ID.
    #[inline]
    pub fn set_last_clicked_id(&mut self, id: Option<MediaId>) {
        self.last_clicked_id = id;
    }

    /// Prunes IDs that no longer exist in the active dataset.
    pub fn prune_missing(&mut self, valid_ids: &HashSet<MediaId>) {
        self.selected_ids.retain(|id| valid_ids.contains(id));
        if let Some(anchor) = self.anchor_id
            && !valid_ids.contains(&anchor)
        {
            self.anchor_id = None;
        }
        if let Some(last) = self.last_clicked_id
            && !valid_ids.contains(&last)
        {
            self.last_clicked_id = None;
        }
    }
}

/// Cumulative metadata summary computed across single or multiple selected media files.
#[derive(Debug, Clone, Default)]
pub struct FileSelectionSummary {
    pub count: usize,
    pub total_size_bytes: u64,
    pub min_dimensions: Option<(u32, u32)>,
    pub max_dimensions: Option<(u32, u32)>,
    pub is_uniform_dimensions: bool,
    pub format_breakdown: Vec<(MediaFormat, usize)>,
    pub avg_rating: f32,
    pub rated_count: usize,
    pub single_record: Option<MediaRecord>,
}

impl FileSelectionSummary {
    pub fn from_records(records: &[MediaRecord]) -> Self {
        let count = records.len();
        if count == 0 {
            return Self::default();
        }

        if count == 1 {
            let rec = &records[0];
            let format_breakdown = vec![(rec.format, 1)];
            return Self {
                count: 1,
                total_size_bytes: rec.file_size,
                min_dimensions: Some((rec.width, rec.height)),
                max_dimensions: Some((rec.width, rec.height)),
                is_uniform_dimensions: true,
                format_breakdown,
                avg_rating: rec.rating as f32,
                rated_count: if rec.rating > 0 { 1 } else { 0 },
                single_record: Some(rec.clone()),
            };
        }

        let mut total_size_bytes = 0u64;
        let mut min_dim = (u32::MAX, u32::MAX, u64::MAX); // (w, h, area)
        let mut max_dim = (0u32, 0u32, 0u64); // (w, h, area)
        let mut first_dim: Option<(u32, u32)> = None;
        let mut is_uniform_dimensions = true;

        let mut format_counts: HashMap<MediaFormat, usize> = HashMap::new();
        let mut total_rating = 0u32;
        let mut rated_count = 0usize;

        for rec in records {
            total_size_bytes += rec.file_size;

            let area = (rec.width as u64) * (rec.height as u64);
            if area < min_dim.2 || (area == min_dim.2 && rec.width < min_dim.0) {
                min_dim = (rec.width, rec.height, area);
            }
            if area > max_dim.2 || (area == max_dim.2 && rec.width > max_dim.0) {
                max_dim = (rec.width, rec.height, area);
            }

            if let Some(fd) = first_dim {
                if fd != (rec.width, rec.height) {
                    is_uniform_dimensions = false;
                }
            } else {
                first_dim = Some((rec.width, rec.height));
            }

            *format_counts.entry(rec.format).or_insert(0) += 1;

            if rec.rating > 0 {
                total_rating += rec.rating as u32;
                rated_count += 1;
            }
        }

        let mut format_breakdown: Vec<(MediaFormat, usize)> = format_counts.into_iter().collect();
        format_breakdown.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
        });

        let avg_rating = if rated_count > 0 {
            total_rating as f32 / rated_count as f32
        } else {
            0.0
        };

        Self {
            count,
            total_size_bytes,
            min_dimensions: Some((min_dim.0, min_dim.1)),
            max_dimensions: Some((max_dim.0, max_dim.1)),
            is_uniform_dimensions,
            format_breakdown,
            avg_rating,
            rated_count,
            single_record: None,
        }
    }

    /// Zero-copy summary aggregation directly from an iterator over indexed media records.
    pub fn from_indexed_records<'a>(
        records: impl IntoIterator<Item = &'a crate::query::filters::IndexedMediaRecord>,
    ) -> Self {
        let mut count = 0usize;
        let mut total_size_bytes = 0u64;
        let mut min_dim = (u32::MAX, u32::MAX, u64::MAX);
        let mut max_dim = (0u32, 0u32, 0u64);
        let mut first_dim: Option<(u32, u32)> = None;
        let mut is_uniform_dimensions = true;

        let mut format_counts: HashMap<MediaFormat, usize> = HashMap::new();
        let mut total_rating = 0u32;
        let mut rated_count = 0usize;
        let mut single_rec: Option<MediaRecord> = None;

        for item in records {
            count += 1;
            total_size_bytes += item.record.file_size;

            let area = (item.record.width as u64) * (item.record.height as u64);
            if area < min_dim.2 || (area == min_dim.2 && item.record.width < min_dim.0) {
                min_dim = (item.record.width, item.record.height, area);
            }
            if area > max_dim.2 || (area == max_dim.2 && item.record.width > max_dim.0) {
                max_dim = (item.record.width, item.record.height, area);
            }

            if let Some(fd) = first_dim {
                if fd != (item.record.width, item.record.height) {
                    is_uniform_dimensions = false;
                }
            } else {
                first_dim = Some((item.record.width, item.record.height));
            }

            *format_counts.entry(item.record.format).or_insert(0) += 1;

            if item.record.rating > 0 {
                total_rating += item.record.rating as u32;
                rated_count += 1;
            }

            if count == 1 {
                single_rec = Some(item.record.clone());
            }
        }

        if count == 0 {
            return Self::default();
        }

        if count == 1 {
            let single = single_rec.unwrap();
            let format_breakdown = vec![(single.format, 1)];
            return Self {
                count: 1,
                total_size_bytes: single.file_size,
                min_dimensions: Some((single.width, single.height)),
                max_dimensions: Some((single.width, single.height)),
                is_uniform_dimensions: true,
                format_breakdown,
                avg_rating: single.rating as f32,
                rated_count: if single.rating > 0 { 1 } else { 0 },
                single_record: Some(single),
            };
        }

        let mut format_breakdown: Vec<(MediaFormat, usize)> = format_counts.into_iter().collect();
        format_breakdown.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)))
        });

        let avg_rating = if rated_count > 0 {
            total_rating as f32 / rated_count as f32
        } else {
            0.0
        };

        Self {
            count,
            total_size_bytes,
            min_dimensions: Some((min_dim.0, min_dim.1)),
            max_dimensions: Some((max_dim.0, max_dim.1)),
            is_uniform_dimensions,
            format_breakdown,
            avg_rating,
            rated_count,
            single_record: None,
        }
    }
}
