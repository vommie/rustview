//! Synthetic library records shared by the benchmark and the scale example (scaffolding of the performance work).

use rustview::library::models::MediaRecord;
use rustview::media::{MediaFormat, MediaType};
use rustview::query::{FolderDir, IndexedMediaRecord};
use std::sync::{Arc, OnceLock};

/// All synthetic records live in one folder, like a large real folder does.
fn folder() -> Arc<FolderDir> {
    static FOLDER: OnceLock<Arc<FolderDir>> = OnceLock::new();
    FOLDER.get_or_init(|| FolderDir::new("/media/library/photos/2024")).clone()
}

pub fn synthetic_record(i: usize) -> IndexedMediaRecord {
    let record = MediaRecord {
        id: i as u64 + 1,
        folder_id: 1,
        filename: format!("IMG_{:07}_holiday_photo.webp", (i * 7919) % 10_000_000),
        file_hash: [0u8; 16],
        file_size: 10_000 + (i as u64 * 31) % 5_000_000,
        modification_time: 1_600_000_000 + i as i64 * 17,
        media_type: MediaType::Image,
        format: MediaFormat::Webp,
        width: 300,
        height: 200,
        rating: (i % 6) as u8,
        last_seen: 0,
        is_deleted: false,
        creation_time: 1_600_000_000 + i as i64 * 13,
        duration_ms: 0,
        is_corrupted: false,
        source: None,
    };
    IndexedMediaRecord::new(i, record, folder(), 0)
}
