use crate::media::{FileHash, MediaFormat, MediaType};
pub use crate::thumbnails::types::ThumbnailLocation;
use serde::{Deserialize, Serialize};

pub type FolderId = u64;
pub type MediaId = u64;
pub type TagId = u32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRecord {
    pub id: FolderId,
    pub path: String,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaRecord {
    pub id: MediaId,
    pub folder_id: FolderId,
    pub filename: String,
    pub file_hash: FileHash,
    pub file_size: u64,
    pub modification_time: i64,
    pub media_type: MediaType,
    pub format: MediaFormat,
    pub width: u32,
    pub height: u32,
    pub rating: u8,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagRecord {
    pub id: TagId,
    pub parent_id: Option<TagId>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseMeta {
    pub schema_version: u32,
    pub hash_algorithm: String,
    pub thumbnail_version: u32,
    pub database_uuid: String,
    pub created_at: i64,
}
