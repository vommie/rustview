use crate::media::{FileHash, MediaFormat, MediaType};
pub use crate::thumbnails::types::ThumbnailLocation;
use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub type FolderId = u64;
pub type MediaId = u64;
pub type TagId = u32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRecord {
    pub id: FolderId,
    pub path: String,
    pub added_at: i64,
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Deserialize)]
struct LegacyFolderRecord {
    id: FolderId,
    path: String,
    added_at: i64,
}

/// Zero-copy custom codec providing seamless backward compatibility for databases
/// created before the `is_deleted` field was introduced.
pub struct FolderRecordCodec;

impl<'a> BytesDecode<'a> for FolderRecordCodec {
    type DItem = FolderRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        match bincode::deserialize::<FolderRecord>(bytes) {
            Ok(record) => Ok(record),
            Err(_) => {
                // Fallback: deserialize legacy 3-field FolderRecord without `is_deleted`
                let legacy: LegacyFolderRecord = bincode::deserialize(bytes)?;
                Ok(FolderRecord {
                    id: legacy.id,
                    path: legacy.path,
                    added_at: legacy.added_at,
                    is_deleted: false,
                })
            }
        }
    }
}

impl<'a> BytesEncode<'a> for FolderRecordCodec {
    type EItem = FolderRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let bytes = bincode::serialize(item)?;
        Ok(Cow::Owned(bytes))
    }
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
