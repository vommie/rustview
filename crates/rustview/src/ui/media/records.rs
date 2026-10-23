//! Reading and indexing the records of the open folder for the Browse list.

use super::BridgeMedia;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rayon::prelude::*;
use rustview_core::library::Library;
use rustview_core::library::models::{FolderId, MediaId, MediaRecord};
use rustview_core::media::{FormatPolicy, MediaType};
use rustview_core::query::FolderDir;
use rustview_core::query::filters::IndexedMediaRecord;
use rustview_core::thumbnails::PipelineItem;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl BridgeMedia {
    /// Drops records whose id was already seen (a record can be listed by more than one scope) and records of formats the
    /// user switched off: every list, query result and count of the UI is built from this, so a switched-off format
    /// is hidden everywhere while its records stay in the library.
    pub(super) fn visible_unique_by_id(raw_records: Vec<MediaRecord>) -> Vec<MediaRecord> {
        let mask = FormatPolicy::mask();
        let mut seen = HashSet::with_capacity(raw_records.len());
        raw_records
            .into_iter()
            .filter(|r| mask.shows(r.format) && seen.insert(r.id))
            .collect()
    }

    pub(super) fn tag_counts(library: &Library, records: &[MediaRecord]) -> HashMap<MediaId, usize> {
        let Ok(rtxn) = library.raw_db().env.read_txn() else {
            return HashMap::new();
        };
        records
            .iter()
            .filter_map(|r| match library.raw_db().tags_by_media.get(&rtxn, &r.id) {
                Ok(Some(tags)) => Some((r.id, tags.len())),
                _ => None,
            })
            .collect()
    }

    /// The pictures among `records` that have stored hover preview frames (a database read per animatable picture, no file access).
    pub(super) fn animated_ids(library: &Library, records: &[MediaRecord]) -> HashSet<MediaId> {
        let candidates = || {
            records.iter().filter(|r| {
                r.media_type == MediaType::Image && rustview_core::thumbnails::animated_frames::can_animate(r.format)
            })
        };
        if candidates().next().is_none() {
            return HashSet::new();
        }
        let Ok(rtxn) = library.raw_db().env.read_txn() else {
            return HashSet::new();
        };
        candidates()
            .filter(|r| matches!(library.raw_db().video_previews.get(&rtxn, &r.id), Ok(Some(p)) if p.frame_count > 1))
            .map(|r| r.id)
            .collect()
    }

    pub fn preindex(library: &Library, raw_records: Vec<MediaRecord>, folder_path: &Path) -> Vec<IndexedMediaRecord> {
        let folder = FolderDir::new(folder_path);
        let records = Self::visible_unique_by_id(raw_records);
        let tag_counts = Self::tag_counts(library, &records);
        let animated = Self::animated_ids(library, &records);

        records
            .into_par_iter()
            .enumerate()
            .map(|(index, r)| {
                let tags_count = tag_counts.get(&r.id).copied().unwrap_or(0);
                let is_animated = animated.contains(&r.id);
                let mut item = IndexedMediaRecord::new(index, r, folder.clone(), tags_count);
                item.is_animated = is_animated;
                item
            })
            .collect()
    }

    pub fn preindex_records(library: &Library, raw_records: Vec<MediaRecord>) -> Vec<IndexedMediaRecord> {
        let records = Self::visible_unique_by_id(raw_records);
        let tag_counts = Self::tag_counts(library, &records);
        let animated = Self::animated_ids(library, &records);
        let mut folders: HashMap<FolderId, Arc<FolderDir>> = HashMap::new();

        records
            .into_iter()
            .enumerate()
            .map(|(index, r)| {
                let folder = folders
                    .entry(r.folder_id)
                    .or_insert_with(|| {
                        let path = library
                            .folders()
                            .get_folder(r.folder_id)
                            .ok()
                            .flatten()
                            .map(|f| PathBuf::from(f.path));
                        FolderDir::new(path.unwrap_or_default())
                    })
                    .clone();
                let tags_count = tag_counts.get(&r.id).copied().unwrap_or(0);
                let is_animated = animated.contains(&r.id);
                let mut item = IndexedMediaRecord::new(index, r, folder, tags_count);
                item.is_animated = is_animated;
                item
            })
            .collect()
    }

    /// The items of the folder that can have a thumbnail (not deleted, not a pending stub), without their paths.
    /// `explicit_refresh`: the user started a refresh, so pictures that could be animated and were never checked are checked now.
    pub fn pipeline_items(records: &[IndexedMediaRecord], explicit_refresh: bool) -> Vec<PipelineItem> {
        records
            .iter()
            .filter(|item| !item.is_deleted && !item.is_pending)
            .map(|item| PipelineItem {
                media_id: item.record.id,
                folder_id: item.record.folder_id,
                is_video: matches!(item.record.media_type, MediaType::Video),
                animatable: item.record.media_type == MediaType::Image
                    && rustview_core::thumbnails::animated_frames::can_animate(item.record.format),
                upgrade_animated: explicit_refresh,
            })
            .collect()
    }

    /// How many records the folder (or, for the recursive view, its whole subtree) holds; a cheap index read.
    pub(in crate::ui) fn folder_record_count(library: &Library, folder_id: FolderId, is_virtual_folder: bool) -> usize {
        let folder_ids = if is_virtual_folder {
            library
                .folders()
                .get_descendant_folder_ids(folder_id)
                .unwrap_or_else(|_| vec![folder_id])
        } else {
            vec![folder_id]
        };
        let Ok(rtxn) = library.raw_db().env.read_txn() else {
            return 0;
        };
        folder_ids
            .iter()
            .filter_map(|fid| library.raw_db().media_by_folder.get(&rtxn, fid).ok().flatten())
            .map(|ids| ids.len())
            .sum()
    }

    /// The indexed records of a folder (or of its whole subtree), read from the library; no UI access, safe on any thread.
    pub(in crate::ui) fn fetch_folder_records(
        library: &Library,
        folder_id: FolderId,
        folder_path: &Path,
        is_virtual_folder: bool,
    ) -> Vec<IndexedMediaRecord> {
        if is_virtual_folder {
            let descendant_ids = library
                .folders()
                .get_descendant_folder_ids(folder_id)
                .unwrap_or_else(|_| vec![folder_id]);
            let mut known = Vec::new();
            let mut seen = HashSet::new();
            if let Ok(rtxn) = library.raw_db().env.read_txn() {
                for fid in descendant_ids {
                    let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, &fid) else {
                        continue;
                    };
                    for mid in ids {
                        if seen.insert(mid)
                            && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &mid)
                        {
                            known.push(rec);
                        }
                    }
                }
            }
            Self::preindex_records(library, known)
        } else {
            let raw_media = library.media().get_all_by_folder(folder_id).unwrap_or_default();
            Self::preindex(library, raw_media, folder_path)
        }
    }

    /// Loads the records of the open folder from the library into the state (the sync path of a folder reload).
    pub(super) fn load_current_folder_records(library: &Library, state: &RwLock<UiState>) {
        let (folder_id, path_opt, is_virtual_folder, is_query_active) = {
            let s = state.read();
            (
                s.current_folder_id,
                s.current_folder_path.clone(),
                s.is_virtual_folder_mode,
                s.is_query_active,
            )
        };
        let (Some(f_id), Some(p)) = (folder_id, path_opt) else {
            return;
        };
        if is_query_active {
            return;
        }
        let indexed = Self::fetch_folder_records(library, f_id, &p, is_virtual_folder);
        state.write().all_folder_media.replace(indexed);
    }
}
