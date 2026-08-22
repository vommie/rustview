use crate::media::{FileHash, MediaFormat, MediaType};
use crate::query::{MatchMode, ScopeMode};
pub use crate::thumbnails::types::{ThumbnailLocation, VideoPreviewRecord};
use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;

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
    #[serde(default)]
    pub creation_time: i64,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub is_corrupted: bool,
}

#[derive(Deserialize)]
struct LegacyMediaRecordV1 {
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

#[derive(Deserialize)]
struct LegacyMediaRecordV2 {
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
    is_deleted: bool,
}

#[derive(Deserialize)]
struct LegacyMediaRecordV3 {
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
    is_deleted: bool,
    creation_time: i64,
    duration_ms: u64,
}

/// Custom codec providing seamless backward compatibility across all schema iterations.
pub struct MediaRecordCodec;

impl<'a> BytesDecode<'a> for MediaRecordCodec {
    type DItem = MediaRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Ok(record) = bincode::deserialize::<MediaRecord>(bytes) {
            return Ok(record);
        }
        if let Ok(legacy3) = bincode::deserialize::<LegacyMediaRecordV3>(bytes) {
            return Ok(MediaRecord {
                id: legacy3.id,
                folder_id: legacy3.folder_id,
                filename: legacy3.filename,
                file_hash: legacy3.file_hash,
                file_size: legacy3.file_size,
                modification_time: legacy3.modification_time,
                media_type: legacy3.media_type,
                format: legacy3.format,
                width: legacy3.width,
                height: legacy3.height,
                rating: legacy3.rating,
                last_seen: legacy3.last_seen,
                is_deleted: legacy3.is_deleted,
                creation_time: legacy3.creation_time,
                duration_ms: legacy3.duration_ms,
                is_corrupted: false,
            });
        }
        if let Ok(legacy2) = bincode::deserialize::<LegacyMediaRecordV2>(bytes) {
            return Ok(MediaRecord {
                id: legacy2.id,
                folder_id: legacy2.folder_id,
                filename: legacy2.filename,
                file_hash: legacy2.file_hash,
                file_size: legacy2.file_size,
                modification_time: legacy2.modification_time,
                media_type: legacy2.media_type,
                format: legacy2.format,
                width: legacy2.width,
                height: legacy2.height,
                rating: legacy2.rating,
                last_seen: legacy2.last_seen,
                is_deleted: legacy2.is_deleted,
                creation_time: legacy2.modification_time,
                duration_ms: 0,
                is_corrupted: false,
            });
        }
        let legacy1: LegacyMediaRecordV1 = bincode::deserialize(bytes)?;
        Ok(MediaRecord {
            id: legacy1.id,
            folder_id: legacy1.folder_id,
            filename: legacy1.filename,
            file_hash: legacy1.file_hash,
            file_size: legacy1.file_size,
            modification_time: legacy1.modification_time,
            media_type: legacy1.media_type,
            format: legacy1.format,
            width: legacy1.width,
            height: legacy1.height,
            rating: legacy1.rating,
            last_seen: legacy1.last_seen,
            is_deleted: false,
            creation_time: legacy1.modification_time,
            duration_ms: 0,
            is_corrupted: false,
        })
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

/// Dedicated external hash record storing checksums and timestamps from external sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalHashRecord {
    pub hash_id: u64,
    pub hash: String,
    pub date_time: String,
}

/// Dynamic search parameters for combinatorial tag & rating queries against LMDB storage.
#[derive(Debug, Clone, Default)]
pub struct MediaSearchParams {
    pub rating: Option<u8>,
    pub tag_ids: HashSet<TagId>,
    pub match_mode: MatchMode,
    pub scope_mode: ScopeMode,
    pub folder_id: Option<FolderId>,
}

/// Supported file transfer modes when dropping files onto folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileTransferKind {
    Copy,
    Move,
}

/// Supported user decisions when a target file name collision occurs during a file move/copy operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileConflictResolution {
    Skip,
    AutoRename,
    Overwrite,
    Cancel,
}

/// Detailed file inspection attributes contrasting conflicting source and destination files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflictItemDetails {
    pub media_id: Option<MediaId>,
    pub folder_id: FolderId,
    pub folder_path: String,
    pub filename: String,
    pub full_path: PathBuf,
    pub file_size: u64,
    pub modification_time: i64,
    pub creation_time: i64,
    pub duration_ms: u64,
    pub file_hash: FileHash,
    pub width: u32,
    pub height: u32,
    pub format: MediaFormat,
    pub rating: u8,
}

/// Comprehensive conflict report pairing the source candidate and the conflicting target item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflictReport {
    pub source: FileConflictItemDetails,
    pub destination: FileConflictItemDetails,
}

/// Summary metrics returned after executing a batch file move or copy operation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchMoveSummary {
    pub total_requested: usize,
    pub moved_count: usize,
    pub auto_renamed_count: usize,
    pub overwritten_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub affected_folder_ids: Vec<FolderId>,
}
