//! Thumbnail-only regeneration for a batch of media files.
//!
//! This is deliberately NOT a refresh: it never reads or writes a `MediaRecord`, never re-hashes a file, never marks
//! anything deleted or corrupted and never touches viewer tabs. It only replaces the stored thumbnail (and the video
//! preview frames) of each target. Videos are re-sampled at a fresh random position per file. A failing target keeps
//! its previous thumbnail because `ThumbnailScheduler::regenerate_sync` writes to the database only after a
//! successful generation.

use super::ram_cache::CachedThumbnail;
use super::types::{generate_random_video_offset_percent, ThumbnailConfig};
use super::ThumbnailManager;
use crate::library::models::MediaId;
use crate::media::{MediaFormat, MediaType};
use rayon::prelude::*;
use std::path::PathBuf;

/// One file whose thumbnail should be regenerated.
#[derive(Debug, Clone)]
pub struct RegenerationTarget {
    pub media_id: MediaId,
    pub path: PathBuf,
}

/// A successfully regenerated thumbnail (already persisted and inserted into the L1 cache).
#[derive(Debug, Clone)]
pub struct RegeneratedThumbnail {
    pub media_id: MediaId,
    pub path: PathBuf,
    pub thumbnail: CachedThumbnail,
    /// Random video position (1..=99 %) used for this file; `None` for images.
    pub offset_percent: Option<u8>,
}

/// A target that could not be regenerated; its previous thumbnail is untouched.
#[derive(Debug, Clone)]
pub struct FailedRegeneration {
    pub media_id: MediaId,
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct RegenerationBatchResult {
    pub succeeded: Vec<RegeneratedThumbnail>,
    pub failed: Vec<FailedRegeneration>,
}

/// Regenerates the thumbnails of all targets in parallel.
///
/// `on_item_done` is called exactly once per target (success or failure) so progress bars always complete.
pub fn regenerate_batch(
    manager: &ThumbnailManager,
    config: &ThumbnailConfig,
    targets: Vec<RegenerationTarget>,
    on_item_done: &(dyn Fn() + Sync),
) -> RegenerationBatchResult {
    let outcomes: Vec<Result<RegeneratedThumbnail, FailedRegeneration>> = targets
        .into_par_iter()
        .map(|target| {
            let outcome = regenerate_one(manager, config, target);
            on_item_done();
            outcome
        })
        .collect();

    let mut result = RegenerationBatchResult::default();
    for outcome in outcomes {
        match outcome {
            Ok(done) => result.succeeded.push(done),
            Err(failed) => result.failed.push(failed),
        }
    }
    result
}

fn regenerate_one(
    manager: &ThumbnailManager,
    config: &ThumbnailConfig,
    target: RegenerationTarget,
) -> Result<RegeneratedThumbnail, FailedRegeneration> {
    let RegenerationTarget { media_id, path } = target;
    let fail = |path: PathBuf, reason: String| FailedRegeneration { media_id, path, reason };

    if !path.is_file() {
        tracing::warn!(media_id, "[ThumbnailRegeneration: Skip] File not found on disk; library left untouched");
        return Err(fail(path, "File not found on disk".to_string()));
    }

    let is_video = MediaFormat::from_path(&path).media_type() == Some(MediaType::Video);
    let (offset_percent, result) = if is_video {
        let pct = generate_random_video_offset_percent();
        (Some(pct), manager.regenerate_with_offset_sync(media_id, &path, config, pct))
    } else {
        (None, manager.regenerate_sync(media_id, &path, config))
    };

    match result {
        Ok(thumbnail) => {
            tracing::debug!(media_id, ?offset_percent, "[ThumbnailRegeneration: Item] Thumbnail regenerated");
            Ok(RegeneratedThumbnail { media_id, path, thumbnail, offset_percent })
        }
        Err(e) => {
            tracing::warn!(media_id, error = %e, "[ThumbnailRegeneration: Item] Generation failed; previous thumbnail kept");
            Err(fail(path, e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn manager_in(dir: &std::path::Path) -> (Arc<Library>, ThumbnailManager) {
        let library = Arc::new(Library::open(&dir.join("library")).expect("open library"));
        let manager = ThumbnailManager::new(library.clone(), &dir.join("thumbs"), 16, 1).expect("thumbnail manager");
        (library, manager)
    }

    fn write_png(path: &std::path::Path) {
        image::RgbImage::from_pixel(64, 48, image::Rgb([200, 40, 40])).save(path).expect("write png");
    }

    fn stored_location(library: &Library, media_id: MediaId) -> Option<crate::thumbnails::ThumbnailLocation> {
        let rtxn = library.raw_db().env.read_txn().expect("read txn");
        library.raw_db().thumbnails.get(&rtxn, &media_id).expect("lookup")
    }

    #[test]
    fn regenerates_image_and_persists_thumbnail_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (library, manager) = manager_in(dir.path());
        let file = dir.path().join("a.png");
        write_png(&file);

        let done = AtomicUsize::new(0);
        let result = regenerate_batch(
            &manager,
            &ThumbnailConfig::default(),
            vec![RegenerationTarget { media_id: 7, path: file }],
            &|| {
                done.fetch_add(1, Ordering::Relaxed);
            },
        );

        assert_eq!(done.load(Ordering::Relaxed), 1);
        assert!(result.failed.is_empty());
        assert_eq!(result.succeeded.len(), 1);
        assert_eq!(result.succeeded[0].offset_percent, None);
        assert!(stored_location(&library, 7).is_some());
    }

    #[test]
    fn missing_file_fails_without_touching_the_store_and_still_reports_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (library, manager) = manager_in(dir.path());

        let done = AtomicUsize::new(0);
        let result = regenerate_batch(
            &manager,
            &ThumbnailConfig::default(),
            vec![RegenerationTarget { media_id: 9, path: dir.path().join("gone.png") }],
            &|| {
                done.fetch_add(1, Ordering::Relaxed);
            },
        );

        assert_eq!(done.load(Ordering::Relaxed), 1);
        assert!(result.succeeded.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert!(stored_location(&library, 9).is_none());
    }

    #[test]
    fn undecodable_file_keeps_the_previous_thumbnail() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (library, manager) = manager_in(dir.path());
        let file = dir.path().join("b.png");
        write_png(&file);
        let cfg = ThumbnailConfig::default();

        let first = regenerate_batch(&manager, &cfg, vec![RegenerationTarget { media_id: 3, path: file.clone() }], &|| {});
        assert_eq!(first.succeeded.len(), 1);
        let before = stored_location(&library, 3).expect("thumbnail stored");

        std::fs::write(&file, b"not an image").expect("corrupt file");
        let second = regenerate_batch(&manager, &cfg, vec![RegenerationTarget { media_id: 3, path: file }], &|| {});
        assert_eq!(second.failed.len(), 1);
        assert_eq!(stored_location(&library, 3), Some(before));
    }
}
