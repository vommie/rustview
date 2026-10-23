use super::error::ThumbnailError;
use crate::library::Library;
use std::collections::HashSet;

pub struct ThumbnailGarbageCollector;

impl ThumbnailGarbageCollector {
    /// Identifies and reports unreachable pack entries.
    pub fn collect_dead_entries(library: &Library) -> Result<HashSet<u64>, ThumbnailError> {
        let rtxn = library.raw_db().env.read_txn()?;
        let mut live_media_ids = HashSet::new();

        for entry in library.raw_db().media.iter(&rtxn)? {
            let (id, _) = entry?;
            live_media_ids.insert(id);
        }

        let mut orphaned_media_ids = HashSet::new();
        for entry in library.raw_db().thumbnails.iter(&rtxn)? {
            let (media_id, _) = entry?;
            if !live_media_ids.contains(&media_id) {
                orphaned_media_ids.insert(media_id);
            }
        }

        Ok(orphaned_media_ids)
    }
}
