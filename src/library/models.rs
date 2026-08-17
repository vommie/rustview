use crate::media::{FileHash, MediaFormat, MediaType};
use crate::query::{MatchMode, ScopeMode};
pub use crate::thumbnails::types::{ThumbnailLocation, VideoPreviewRecord};
use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;

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
/// created before the `is_deleted` field was introduced for folders.
pub struct FolderRecordCodec;

impl<'a> BytesDecode<'a> for FolderRecordCodec {
    type DItem = FolderRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        match bincode::deserialize::<FolderRecord>(bytes) {
            Ok(record) => Ok(record),
            Err(_) => {
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
    #[serde(default)]
    pub is_deleted: bool,
}

#[derive(Deserialize)]
struct LegacyMediaRecord {
    id: MediaId,
    folder_id: FolderId,
    filename: String,
    file_hash: FileHash,
    file_size: u64,
    modification_time: i64,
    media_type: MediaType,
    format: MediaFormat,
    width: u32,
    height: u32,
    rating: u8,
    last_seen: u64,
}

/// Custom codec providing seamless backward compatibility for databases
/// created before the `is_deleted` field was introduced for media records.
pub struct MediaRecordCodec;

impl<'a> BytesDecode<'a> for MediaRecordCodec {
    type DItem = MediaRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        match bincode::deserialize::<MediaRecord>(bytes) {
            Ok(record) => Ok(record),
            Err(_) => {
                let legacy: LegacyMediaRecord = bincode::deserialize(bytes)?;
                Ok(MediaRecord {
                    id: legacy.id,
                    folder_id: legacy.folder_id,
                    filename: legacy.filename,
                    file_hash: legacy.file_hash,
                    file_size: legacy.file_size,
                    modification_time: legacy.modification_time,
                    media_type: legacy.media_type,
                    format: legacy.format,
                    width: legacy.width,
                    height: legacy.height,
                    rating: legacy.rating,
                    last_seen: legacy.last_seen,
                    is_deleted: false,
                })
            }
        }
    }
}

impl<'a> BytesEncode<'a> for MediaRecordCodec {
    type EItem = MediaRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let bytes = bincode::serialize(item)?;
        Ok(Cow::Owned(bytes))
    }
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

/// Dynamic search parameters for combinatorial tag & rating queries against LMDB storage.
#[derive(Debug, Clone, Default)]
pub struct MediaSearchParams {
    pub rating: Option<u8>, // None or Some(0) = wildcard, Some(1..=5) = exact match
    pub tag_ids: HashSet<TagId>,
    pub match_mode: MatchMode,
    pub scope_mode: ScopeMode,
    pub folder_id: Option<FolderId>,
}
