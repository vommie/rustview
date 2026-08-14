use serde::{Deserialize, Serialize};
use std::path::Path;

pub const THUMBNAIL_SIZE: u32 = 256;
pub const THUMBNAIL_CHANNELS: usize = 3;
pub const THUMBNAIL_PIXELS: usize = (THUMBNAIL_SIZE * THUMBNAIL_SIZE) as usize;
pub const THUMBNAIL_BYTE_SIZE: usize = THUMBNAIL_PIXELS * THUMBNAIL_CHANNELS; // 196,608 Bytes (192 KiB)

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ThumbnailPriority {
    P0Visible = 0,
    P1Scrolling = 1,
    P2Prefetch = 2,
    P3Background = 3,
    P4Maintenance = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailLocation {
    pub pack_id: u32,
    pub offset: u64,
    pub length: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Contain,
    Cover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailHandle {
    pub slot_index: u32,
    pub generation: u32,
}

/// Anonymizes a filesystem path for logging to guarantee privacy while preserving debugging utility.
/// Output format: `MediaId(42) [*.ext]` or `Unknown [*.ext]`
pub fn anonymize_target(path: &Path, media_id: Option<u64>) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_else(|| ".unknown".to_string());

    let size_hint = std::fs::metadata(path)
        .map(|m| format!(", {:.2} KB", m.len() as f64 / 1024.0))
        .unwrap_or_default();

    match media_id {
        Some(id) => format!("MediaId({}) [*{}{}]", id, ext, size_hint),
        None => format!("Anonymous [*{}{}]", ext, size_hint),
    }
}
