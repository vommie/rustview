use super::error::LibraryError;
use super::models::*;
use super::schema::*;
use crate::media::{FileHash, MediaHasher};
use heed::byteorder::BE;
use heed::types::{Bytes, SerdeBincode, Str, U32, U64};
use heed::{Database, Env, EnvOpenOptions};
use std::fs;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct LibraryDatabase {
    pub env: Arc<Env>,

    // Tables
    pub meta: Database<Str, SerdeBincode<DatabaseMeta>>,
    pub sequences: Database<Str, U64<BE>>,
    pub folders: Database<U64<BE>, FolderRecordCodec>,
    pub folders_by_path: Database<Str, U64<BE>>,
    pub media: Database<U64<BE>, MediaRecordCodec>,
    pub media_by_folder: Database<U64<BE>, SerdeBincode<Vec<MediaId>>>,
    pub media_by_hash: Database<Bytes, SerdeBincode<Vec<MediaId>>>,
    pub tags: Database<U32<BE>, SerdeBincode<TagRecord>>,
    pub tags_by_parent_name: Database<Str, U32<BE>>,
    pub tags_by_media: Database<U64<BE>, SerdeBincode<Vec<TagId>>>,
    pub media_by_tag: Database<U32<BE>, SerdeBincode<Vec<MediaId>>>,
    pub thumbnails: Database<U64<BE>, SerdeBincode<ThumbnailLocation>>,
    pub video_previews: Database<U64<BE>, SerdeBincode<VideoPreviewRecord>>,
}

impl LibraryDatabase {
    pub fn open(path: &Path) -> Result<Self, LibraryError> {
        fs::create_dir_all(path)?;

        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(128 * 1024 * 1024 * 1024)
                .max_dbs(20)
                .open(path)?
        };

        let mut wtxn = env.write_txn()?;

        let meta: Database<Str, SerdeBincode<DatabaseMeta>> =
            env.create_database(&mut wtxn, Some(DB_META))?;
        let sequences: Database<Str, U64<BE>> =
            env.create_database(&mut wtxn, Some(DB_SEQUENCES))?;
        let folders: Database<U64<BE>, FolderRecordCodec> =
            env.create_database(&mut wtxn, Some(DB_FOLDERS))?;
        let folders_by_path: Database<Str, U64<BE>> =
            env.create_database(&mut wtxn, Some(DB_FOLDERS_BY_PATH))?;
        let media: Database<U64<BE>, MediaRecordCodec> =
            env.create_database(&mut wtxn, Some(DB_MEDIA))?;
        let media_by_folder: Database<U64<BE>, SerdeBincode<Vec<MediaId>>> =
            env.create_database(&mut wtxn, Some(DB_MEDIA_BY_FOLDER))?;
        let media_by_hash: Database<Bytes, SerdeBincode<Vec<MediaId>>> =
            env.create_database(&mut wtxn, Some(DB_MEDIA_BY_HASH))?;
        let tags: Database<U32<BE>, SerdeBincode<TagRecord>> =
            env.create_database(&mut wtxn, Some(DB_TAGS))?;
        let tags_by_parent_name: Database<Str, U32<BE>> =
            env.create_database(&mut wtxn, Some(DB_TAGS_BY_PARENT_NAME))?;
        let tags_by_media: Database<U64<BE>, SerdeBincode<Vec<TagId>>> =
            env.create_database(&mut wtxn, Some(DB_TAGS_BY_MEDIA))?;
        let media_by_tag: Database<U32<BE>, SerdeBincode<Vec<MediaId>>> =
            env.create_database(&mut wtxn, Some(DB_MEDIA_BY_TAG))?;
        let thumbnails: Database<U64<BE>, SerdeBincode<ThumbnailLocation>> =
            env.create_database(&mut wtxn, Some(DB_THUMBNAILS))?;
        let video_previews: Database<U64<BE>, SerdeBincode<VideoPreviewRecord>> =
            env.create_database(&mut wtxn, Some(DB_VIDEO_PREVIEWS))?;

        if let Some(existing_meta) = meta.get(&wtxn, "meta")? {
            if existing_meta.schema_version != CURRENT_SCHEMA_VERSION {
                return Err(LibraryError::SchemaVersionMismatch {
                    found: existing_meta.schema_version,
                    expected: CURRENT_SCHEMA_VERSION,
                });
            }
        } else {
            let metadata = DatabaseMeta {
                schema_version: CURRENT_SCHEMA_VERSION,
                hash_algorithm: HASH_ALGORITHM.to_string(),
                thumbnail_version: CURRENT_THUMBNAIL_VERSION,
                database_uuid: MediaHasher::to_hex(&MediaHasher::hash_bytes(
                    path.to_string_lossy().as_bytes(),
                )),
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            };
            meta.put(&mut wtxn, "meta", &metadata)?;
        }

        wtxn.commit()?;

        Ok(Self {
            env: Arc::new(env),
            meta,
            sequences,
            folders,
            folders_by_path,
            media,
            media_by_folder,
            media_by_hash,
            tags,
            tags_by_parent_name,
            tags_by_media,
            media_by_tag,
            thumbnails,
            video_previews,
        })
    }

    pub fn next_id(&self, wtxn: &mut heed::RwTxn, seq_key: &str) -> Result<u64, LibraryError> {
        let current = self.sequences.get(wtxn, seq_key)?.unwrap_or(0);
        let next = current + 1;
        self.sequences.put(wtxn, seq_key, &next)?;
        Ok(next)
    }

    pub fn next_generation(&self, wtxn: &mut heed::RwTxn) -> Result<u64, LibraryError> {
        self.next_id(wtxn, SEQ_REFRESH_GEN)
    }

    /// Atomically updates the secondary `media_by_hash` index for a given `media_id`.
    /// Handles unregistering from an old hash bucket and registering into a new hash bucket.
    pub fn update_media_hash_index(
        &self,
        wtxn: &mut heed::RwTxn,
        media_id: MediaId,
        old_hash: Option<&FileHash>,
        new_hash: Option<&FileHash>,
    ) -> Result<(), LibraryError> {
        if let Some(old) = old_hash
            && let Some(mut hash_list) = self.media_by_hash.get(wtxn, &old[..])?
        {
            hash_list.retain(|&id| id != media_id);
            if hash_list.is_empty() {
                self.media_by_hash.delete(wtxn, &old[..])?;
            } else {
                self.media_by_hash.put(wtxn, &old[..], &hash_list)?;
            }
        }

        if let Some(new) = new_hash {
            let mut hash_list = self.media_by_hash.get(wtxn, &new[..])?.unwrap_or_default();
            if !hash_list.contains(&media_id) {
                hash_list.push(media_id);
                self.media_by_hash.put(wtxn, &new[..], &hash_list)?;
            }
        }

        Ok(())
    }
}
