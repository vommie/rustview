use super::error::MigrationError;
use super::mapper::MappedData;
use super::stats::MigrationStats;
use crate::library::db::LibraryDatabase;
use crate::library::models::DatabaseMeta;
use crate::library::schema::{
    CURRENT_SCHEMA_VERSION, CURRENT_THUMBNAIL_VERSION, HASH_ALGORITHM, SEQ_EXTERNAL_HASH_ID, SEQ_FOLDER_ID,
    SEQ_MEDIA_ID, SEQ_REFRESH_GEN, SEQ_TAG_ID,
};
use crate::media::MediaHasher;
use crate::settings::LogObscurer;
use crate::util::cast::SaturatingAs;
use std::fs;
use std::path::Path;
use std::time::Instant;

pub struct XnViewDbWriter;

impl XnViewDbWriter {
    /// Wipes all existing `RustView` LMDB database files and thumbnail caches,
    /// then initializes fresh databases and commits the mapped records atomically.
    pub fn wipe_and_write(
        data_dir: &Path,
        mapped: &MappedData,
        stats: &mut MigrationStats,
    ) -> Result<(), MigrationError> {
        let t0 = Instant::now();

        let db_path = data_dir.join("library.lmdb");
        let cache_path = data_dir.join("thumbnails");

        tracing::warn!(
            db_path = %LogObscurer::path(&db_path),
            cache_path = %LogObscurer::path(&cache_path),
            "[XnViewWriter] Destructive wipe: Removing existing RustView databases and thumbnail caches..."
        );

        if db_path.exists() {
            fs::remove_dir_all(&db_path)?;
        }
        if cache_path.exists() {
            fs::remove_dir_all(&cache_path)?;
        }

        fs::create_dir_all(&db_path)?;
        fs::create_dir_all(&cache_path)?;

        tracing::info!("[XnViewWriter] Initializing fresh clean LMDB database environment...");
        let db = LibraryDatabase::open(&db_path)?;

        let mut wtxn = db.env.write_txn()?;

        // 1. Write Database Metadata
        let meta_record = DatabaseMeta {
            schema_version: CURRENT_SCHEMA_VERSION,
            hash_algorithm: HASH_ALGORITHM.to_string(),
            thumbnail_version: CURRENT_THUMBNAIL_VERSION,
            database_uuid: MediaHasher::to_hex(&MediaHasher::hash_bytes(db_path.to_string_lossy().as_bytes())),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs().saturating_as::<i64>()),
        };
        db.meta.put(&mut wtxn, "meta", &meta_record)?;

        // 2. Write Folders, Folders-by-Path, and Folders-by-Parent
        for folder in &mapped.folders {
            db.folders.put(&mut wtxn, &folder.id, folder)?;
            db.folders_by_path.put(&mut wtxn, &folder.path, &folder.id)?;

            let folder_media = mapped.media_by_folder.get(&folder.id).cloned().unwrap_or_default();
            db.media_by_folder.put(&mut wtxn, &folder.id, &folder_media)?;
        }

        for (parent_id, children) in &mapped.folders_by_parent {
            db.folders_by_parent.put(&mut wtxn, parent_id, children)?;
        }

        db.sequences.put(&mut wtxn, SEQ_FOLDER_ID, &mapped.max_folder_id)?;

        // 3. Write Tags & Tags-by-Parent-Name
        for tag in &mapped.tags {
            db.tags.put(&mut wtxn, &tag.id, tag)?;
            let key = format!("{}:{}", tag.parent_id.unwrap_or(0), tag.name.to_lowercase());
            db.tags_by_parent_name.put(&mut wtxn, &key, &tag.id)?;

            let tag_media = mapped.media_by_tag.get(&tag.id).cloned().unwrap_or_default();
            db.media_by_tag.put(&mut wtxn, &tag.id, &tag_media)?;
        }
        db.sequences.put(&mut wtxn, SEQ_TAG_ID, &u64::from(mapped.max_tag_id))?;

        // 4. Write Media, Hash Index, and Tag Associations
        for item in &mapped.media {
            db.media.put(&mut wtxn, &item.id, item)?;

            if let Some(item_tags) = mapped.tags_by_media.get(&item.id) {
                db.tags_by_media.put(&mut wtxn, &item.id, item_tags)?;
            }
        }

        for (hash, media_ids) in &mapped.media_by_hash {
            db.media_by_hash.put(&mut wtxn, &hash[..], media_ids)?;
        }

        db.sequences.put(&mut wtxn, SEQ_MEDIA_ID, &mapped.max_media_id)?;
        db.sequences.put(&mut wtxn, SEQ_REFRESH_GEN, &1u64)?;

        // 5. Write Dedicated External Hashes Schema
        for ext_hash in &mapped.external_hashes {
            db.external_hashes.put(&mut wtxn, &ext_hash.hash_id, ext_hash)?;
        }
        db.sequences
            .put(&mut wtxn, SEQ_EXTERNAL_HASH_ID, &mapped.max_external_hash_id)?;

        // 6. Atomic Transaction Commit
        wtxn.commit()?;
        stats.write_time_ms = t0.elapsed().as_millis();

        tracing::info!(
            elapsed_ms = stats.write_time_ms,
            folders_count = mapped.folders.len(),
            media_count = mapped.media.len(),
            tags_count = mapped.tags.len(),
            "[XnViewWriter] Atomically committed all re-indexed tables and sequences to LMDB"
        );

        Ok(())
    }
}
