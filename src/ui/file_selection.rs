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

    /// Selects a contiguous range between the anchor and the target ID (Shift + Click).
    /// If `additive` is true (Ctrl + Shift + Click), existing selections outside the range are preserved.
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

    /// Returns the last clicked file ID.
    #[inline]
    pub fn last_clicked_id(&self) -> Option<MediaId> {
        self.last_clicked_id
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::MediaType;

    fn sample_record(
        id: MediaId,
        filename: &str,
        size: u64,
        w: u32,
        h: u32,
        fmt: MediaFormat,
        rating: u8,
    ) -> MediaRecord {
        MediaRecord {
            id,
            folder_id: 1,
            filename: filename.to_string(),
            file_hash: [0u8; 32],
            file_size: size,
            modification_time: 0,
            media_type: MediaType::Image,
            format: fmt,
            width: w,
            height: h,
            rating,
            last_seen: 0,
        }
    }

    #[test]
    fn test_single_file_selection() {
        let mut fsm = FileSelectionModel::new();
        fsm.select_single(10);
        assert_eq!(fsm.count(), 1);
        assert!(fsm.is_selected(10));

        fsm.select_single(20);
        assert_eq!(fsm.count(), 1);
        assert!(!fsm.is_selected(10));
        assert!(fsm.is_selected(20));
        assert_eq!(fsm.anchor_id(), Some(20));
    }

    #[test]
    fn test_ctrl_toggle_file_selection() {
        let mut fsm = FileSelectionModel::new();
        fsm.select_single(10);
        fsm.toggle(20);
        fsm.toggle(30);
        assert_eq!(fsm.count(), 3);
        assert!(fsm.is_selected(10));
        assert!(fsm.is_selected(20));
        assert!(fsm.is_selected(30));

        fsm.toggle(20);
        assert_eq!(fsm.count(), 2);
        assert!(!fsm.is_selected(20));
        assert_eq!(fsm.anchor_id(), Some(20));
    }

    #[test]
    fn test_shift_range_file_selection() {
        let mut fsm = FileSelectionModel::new();
        let visible = vec![1, 2, 3, 4, 5, 6, 7];

        fsm.select_single(2);
        assert_eq!(fsm.anchor_id(), Some(2));

        fsm.select_range(5, &visible, false);
        assert_eq!(fsm.count(), 4);
        assert!(fsm.is_selected(2));
        assert!(fsm.is_selected(3));
        assert!(fsm.is_selected(4));
        assert!(fsm.is_selected(5));
        assert_eq!(fsm.anchor_id(), Some(2));

        fsm.select_range(1, &visible, false);
        assert_eq!(fsm.count(), 2);
        assert!(fsm.is_selected(1));
        assert!(fsm.is_selected(2));
        assert!(!fsm.is_selected(3));
    }

    #[test]
    fn test_file_summary_aggregation() {
        let r1 = sample_record(1, "a.jpg", 1_000, 800, 600, MediaFormat::Jpeg, 5);
        let r2 = sample_record(2, "b.jpg", 2_000, 1920, 1080, MediaFormat::Jpeg, 3);
        let r3 = sample_record(3, "c.png", 3_000, 1280, 720, MediaFormat::Png, 0);

        let summary = FileSelectionSummary::from_records(&[r1, r2, r3]);
        assert_eq!(summary.count, 3);
        assert_eq!(summary.total_size_bytes, 6_000);
        assert_eq!(summary.min_dimensions, Some((800, 600)));
        assert_eq!(summary.max_dimensions, Some((1920, 1080)));
        assert!(!summary.is_uniform_dimensions);
        assert_eq!(summary.avg_rating, 4.0);
        assert_eq!(summary.rated_count, 2);
        assert_eq!(summary.format_breakdown[0], (MediaFormat::Jpeg, 2));
        assert_eq!(summary.format_breakdown[1], (MediaFormat::Png, 1));
    }
}
