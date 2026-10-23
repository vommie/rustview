use super::error::ApiError;
use super::models::*;
use crate::library::folders::normalize_path;
use crate::library::models::{ExternalHashRecord, MediaRecord, TagId};
use crate::library::schema::*;
use crate::library::Library;
use crate::media::{MediaFormat, MediaHasher, MetadataExtractor};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Instant;

pub struct ApiHandler;

impl ApiHandler {
    /// Retrieves all tags in a clean hierarchical list.
    pub fn get_tag_tree(lib: &Library, json: bool) -> Result<i32, ApiError> {
        let tags = lib.tags().list_all_tags()?;
        let mut tag_items = Vec::with_capacity(tags.len());

        for t in tags {
            tag_items.push(ApiTagItem {
                id: t.id,
                parent_id: t.parent_id,
                name: t.name,
            });
        }
        tag_items.sort_by_key(|t| t.id);

        let response = ApiTagTreeResponse {
            success: true,
            total_tags: tag_items.len(),
            tags: tag_items,
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!("RustView Tag Tree ({} total tags):", response.total_tags);
            for t in &response.tags {
                let parent_str = match t.parent_id {
                    Some(pid) => format!(" (Parent: #{})", pid),
                    None => " (Root)".to_string(),
                };
                println!("  • [#{:<3}] {}{}", t.id, t.name, parent_str);
            }
        }

        Ok(0)
    }

    /// Queries a single media file by path without creating or modifying records.
    pub fn get_media(lib: &Library, path: &Path, json: bool) -> Result<i32, ApiError> {
        let (folder_path, filename) = split_folder_and_filename(path)?;
        let folder_opt = lib.folders().get_folder_by_path(&folder_path)?;

        let media_opt = match folder_opt {
            Some(folder) => {
                let rtxn = lib.raw_db().env.read_txn()?;
                let media_ids = lib
                    .raw_db()
                    .media_by_folder
                    .get(&rtxn, &folder.id)?
                    .unwrap_or_default();

                let mut found_record = None;
                for mid in media_ids {
                    if let Some(rec) = lib.raw_db().media.get(&rtxn, &mid)?
                        && rec.filename.eq_ignore_ascii_case(&filename)
                    {
                        found_record = Some(rec);
                        break;
                    }
                }
                drop(rtxn);

                if let Some(rec) = found_record {
                    let tags = lib
                        .tags()
                        .get_tags_for_media(rec.id)?
                        .into_iter()
                        .map(|t| t.id)
                        .collect();
                    let hash_hex = MediaHasher::to_hex(&rec.file_hash);

                    Some(ApiMediaDetails {
                        id: rec.id,
                        folder_id: folder.id,
                        folder_path: folder.path.clone(),
                        filename: rec.filename.clone(),
                        full_path: format!("{}/{}", folder.path, rec.filename),
                        file_size: rec.file_size,
                        modification_time: rec.modification_time,
                        rating: rec.rating,
                        tags,
                        file_hash: hash_hex,
                        width: rec.width,
                        height: rec.height,
                        media_type: format!("{:?}", rec.media_type),
                        format: format!("{:?}", rec.format),
                        is_deleted: rec.is_deleted,
                        is_corrupted: rec.is_corrupted,
                        source: rec.source.clone(),
                    })
                } else {
                    None
                }
            }
            None => None,
        };

        let found = media_opt.is_some();
        let response = ApiGetMediaResponse {
            success: true,
            found,
            media: media_opt,
            message: if found {
                None
            } else {
                Some(format!(
                    "File '{}' is not registered in the database.",
                    path.display()
                ))
            },
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else if let Some(ref m) = response.media {
            println!("Media Record Found:");
            println!("  • ID        : {}", m.id);
            println!("  • Path      : {}", m.full_path);
            println!("  • Rating    : {} ★", m.rating);
            println!("  • Tags      : {:?}", m.tags);
            println!("  • Hash (MD5): {}", m.file_hash);
            println!("  • Dimensions: {}x{}", m.width, m.height);
            println!("  • Size      : {} bytes", m.file_size);
            if let Some(ref src) = m.source {
                println!("  • Source    : {}", src);
            }
            println!("  • Corrupted : {}", m.is_corrupted);
            println!("  • Deleted   : {}", m.is_deleted);
        } else {
            println!(
                "Notice: File '{}' is not registered in the database.",
                path.display()
            );
        }

        Ok(if found { 0 } else { 2 })
    }

    /// Atomically sets/upserts media record, updates rating, tags, source application, and ensures folder registration.
    /// If the file does not yet exist on disk (render queued/in-progress), it initializes mtime to the current timestamp
    /// so that it appears at the top when sorting by newest, and sets is_corrupted = false so it displays as a valid pending stub.
    pub fn set_media(
        lib: &Library,
        path: &Path,
        rating_opt: Option<u8>,
        tags_opt: Option<Vec<u32>>,
        hash_opt: Option<String>,
        source_opt: Option<String>,
        json: bool,
    ) -> Result<i32, ApiError> {
        let (folder_path, filename) = split_folder_and_filename(path)?;

        let folder = match lib.folders().get_folder_by_path(&folder_path)? {
            Some(f) => f,
            None => lib.folders().add_folder(&folder_path)?,
        };

        let mut existing_record: Option<MediaRecord> = None;

        {
            let rtxn = lib.raw_db().env.read_txn()?;
            let media_ids = lib
                .raw_db()
                .media_by_folder
                .get(&rtxn, &folder.id)?
                .unwrap_or_default();

            for mid in media_ids {
                if let Some(rec) = lib.raw_db().media.get(&rtxn, &mid)?
                    && rec.filename.eq_ignore_ascii_case(&filename)
                {
                    existing_record = Some(rec);
                    break;
                }
            }
        }

        let mut wtxn = lib.raw_db().env.write_txn()?;
        let current_gen = lib.raw_db().next_generation(&mut wtxn)?;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        let (action, final_record, target_media_id) = match existing_record {
            Some(mut rec) => {
                let mid = rec.id;
                let old_hash = rec.file_hash;

                if let Some(r) = rating_opt {
                    rec.rating = r.min(5);
                }

                if let Some(ref s) = source_opt {
                    rec.source = Some(s.clone());
                }

                if let Some(ref h_str) = hash_opt
                    && let Ok(new_hash) = MediaHasher::from_hex(h_str)
                    && new_hash != old_hash
                {
                    lib.raw_db().update_media_hash_index(
                        &mut wtxn,
                        mid,
                        Some(&old_hash),
                        Some(&new_hash),
                    )?;
                    rec.file_hash = new_hash;
                }

                let is_corrupted = false;

                if path.exists() {
                    if let Ok(meta) = std::fs::metadata(path) {
                        rec.file_size = meta.len();
                        rec.modification_time = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(rec.modification_time);

                        rec.creation_time = meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(rec.modification_time);
                    }
                    if rec.file_size > 0
                        && let Ok(raw_meta) = MetadataExtractor::extract(path)
                    {
                        rec.width = raw_meta.width;
                        rec.height = raw_meta.height;
                        rec.duration_ms = raw_meta.duration_ms;
                    }
                } else if rec.modification_time == 0 {
                    rec.modification_time = now_secs;
                    rec.creation_time = now_secs;
                }

                rec.last_seen = current_gen;
                rec.is_deleted = false;
                rec.is_corrupted = is_corrupted;

                lib.raw_db().media.put(&mut wtxn, &mid, &rec)?;
                ("updated", rec, mid)
            }
            None => {
                let new_id = lib.raw_db().next_id(&mut wtxn, SEQ_MEDIA_ID)?;

                let format = MediaFormat::from_path(Path::new(&filename));
                let mut media_type = format
                    .media_type()
                    .unwrap_or(crate::media::MediaType::Image);

                let mut file_size = 0u64;
                let mut mtime = now_secs; // Default to current time so newest sort places it at top
                let mut ctime = now_secs;
                let mut width = 0u32;
                let mut height = 0u32;
                let mut duration_ms = 0u64;
                let is_corrupted = false; // Fresh stub is pending, not corrupted

                if path.exists() {
                    if let Ok(meta) = std::fs::metadata(path) {
                        file_size = meta.len();
                        mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(now_secs);
                        ctime = meta
                            .created()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(mtime);
                    }
                    if file_size > 0
                        && let Ok(raw_meta) = MetadataExtractor::extract(path)
                    {
                        width = raw_meta.width;
                        height = raw_meta.height;
                        duration_ms = raw_meta.duration_ms;
                        media_type = raw_meta.media_type;
                    }
                }

                let file_hash = if let Some(ref h_str) = hash_opt
                    && let Ok(parsed) = MediaHasher::from_hex(h_str)
                {
                    parsed
                } else if path.exists() && file_size > 0 {
                    MediaHasher::hash_file(path).unwrap_or_else(|_| {
                        MediaHasher::hash_bytes(path.to_string_lossy().as_bytes())
                    })
                } else {
                    MediaHasher::hash_bytes(format!("{}/{}", folder.id, filename).as_bytes())
                };

                let assigned_source = source_opt.or_else(|| Some("CLI-API".to_string()));

                let new_rec = MediaRecord {
                    id: new_id,
                    folder_id: folder.id,
                    filename: filename.clone(),
                    file_hash,
                    file_size,
                    modification_time: mtime,
                    creation_time: ctime,
                    duration_ms,
                    media_type,
                    format,
                    width,
                    height,
                    rating: rating_opt.unwrap_or(0).min(5),
                    last_seen: current_gen,
                    is_deleted: false,
                    is_corrupted,
                    source: assigned_source,
                };

                lib.raw_db().media.put(&mut wtxn, &new_id, &new_rec)?;
                lib.raw_db().update_media_hash_index(
                    &mut wtxn,
                    new_id,
                    None,
                    Some(&file_hash),
                )?;

                let mut folder_media = lib
                    .raw_db()
                    .media_by_folder
                    .get(&wtxn, &folder.id)?
                    .unwrap_or_default();
                if !folder_media.contains(&new_id) {
                    folder_media.push(new_id);
                    lib.raw_db()
                        .media_by_folder
                        .put(&mut wtxn, &folder.id, &folder_media)?;
                }

                ("created", new_rec, new_id)
            }
        };

        if let Some(target_tag_ids) = tags_opt {
            let unique_tags: Vec<TagId> = target_tag_ids
                .into_iter()
                .collect::<HashSet<_>>()
                .into_iter()
                .collect();

            if let Some(old_tags) = lib.raw_db().tags_by_media.get(&wtxn, &target_media_id)? {
                for old_t in old_tags {
                    if let Some(mut t_media) = lib.raw_db().media_by_tag.get(&wtxn, &old_t)? {
                        t_media.retain(|&id| id != target_media_id);
                        if t_media.is_empty() {
                            lib.raw_db().media_by_tag.delete(&mut wtxn, &old_t)?;
                        } else {
                            lib.raw_db()
                                .media_by_tag
                                .put(&mut wtxn, &old_t, &t_media)?;
                        }
                    }
                }
            }

            for &tag_id in &unique_tags {
                if lib.raw_db().tags.get(&wtxn, &tag_id)?.is_some() {
                    let mut t_media = lib
                        .raw_db()
                        .media_by_tag
                        .get(&wtxn, &tag_id)?
                        .unwrap_or_default();
                    if !t_media.contains(&target_media_id) {
                        t_media.push(target_media_id);
                        lib.raw_db()
                            .media_by_tag
                            .put(&mut wtxn, &tag_id, &t_media)?;
                    }
                }
            }

            if unique_tags.is_empty() {
                lib.raw_db()
                    .tags_by_media
                    .delete(&mut wtxn, &target_media_id)?;
            } else {
                lib.raw_db()
                    .tags_by_media
                    .put(&mut wtxn, &target_media_id, &unique_tags)?;
            }
        }

        wtxn.commit()?;

        let final_tags = lib
            .tags()
            .get_tags_for_media(target_media_id)?
            .into_iter()
            .map(|t| t.id)
            .collect();
        let hash_hex = MediaHasher::to_hex(&final_record.file_hash);

        let response = ApiSetMediaResponse {
            success: true,
            action: action.to_string(),
            media_id: target_media_id,
            folder_id: folder.id,
            filename: final_record.filename,
            rating: final_record.rating,
            tags: final_tags,
            file_hash: hash_hex,
            source: final_record.source.clone(),
            message: format!("Media record successfully {} in LMDB.", action),
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!(
                "✓ Media #{} successfully {}:",
                response.media_id, response.action
            );
            println!("  • File      : {}/{}", folder.path, response.filename);
            println!("  • Rating    : {} ★", response.rating);
            println!("  • Tags      : {:?}", response.tags);
            println!("  • Hash (MD5): {}", response.file_hash);
            if let Some(ref src) = response.source {
                println!("  • Source    : {}", src);
            }
            println!("  • Corrupted : {}", final_record.is_corrupted);
        }

        Ok(0)
    }

    /// Deletes media record from database and cleans up tag associations.
    pub fn delete_media(lib: &Library, path: &Path, json: bool) -> Result<i32, ApiError> {
        let (folder_path, filename) = split_folder_and_filename(path)?;
        let folder_opt = lib.folders().get_folder_by_path(&folder_path)?;

        let mut deleted_id = None;

        if let Some(folder) = folder_opt {
            let mut wtxn = lib.raw_db().env.write_txn()?;
            let mut media_ids = lib
                .raw_db()
                .media_by_folder
                .get(&wtxn, &folder.id)?
                .unwrap_or_default();

            let mut found_id = None;
            for &mid in &media_ids {
                if let Some(rec) = lib.raw_db().media.get(&wtxn, &mid)?
                    && rec.filename.eq_ignore_ascii_case(&filename)
                {
                    found_id = Some((mid, rec.file_hash));
                    break;
                }
            }

            if let Some((mid, hash)) = found_id {
                deleted_id = Some(mid);

                lib.raw_db()
                    .update_media_hash_index(&mut wtxn, mid, Some(&hash), None)?;
                lib.raw_db().media.delete(&mut wtxn, &mid)?;
                lib.raw_db().thumbnails.delete(&mut wtxn, &mid)?;
                lib.raw_db().video_previews.delete(&mut wtxn, &mid)?;

                if let Some(tags) = lib.raw_db().tags_by_media.get(&wtxn, &mid)? {
                    for tag_id in tags {
                        if let Some(mut t_media) =
                            lib.raw_db().media_by_tag.get(&wtxn, &tag_id)?
                        {
                            t_media.retain(|&id| id != mid);
                            if t_media.is_empty() {
                                lib.raw_db().media_by_tag.delete(&mut wtxn, &tag_id)?;
                            } else {
                                lib.raw_db()
                                    .media_by_tag
                                    .put(&mut wtxn, &tag_id, &t_media)?;
                            }
                        }
                    }
                    lib.raw_db().tags_by_media.delete(&mut wtxn, &mid)?;
                }

                media_ids.retain(|&id| id != mid);
                lib.raw_db()
                    .media_by_folder
                    .put(&mut wtxn, &folder.id, &media_ids)?;

                wtxn.commit()?;
            }
        }

        let is_deleted = deleted_id.is_some();
        let response = ApiDeleteMediaResponse {
            success: true,
            deleted: is_deleted,
            media_id: deleted_id,
            path: path.to_string_lossy().to_string(),
            message: if is_deleted {
                format!("Media #{} removed from database.", deleted_id.unwrap())
            } else {
                "Media file was not found in database.".to_string()
            },
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!("{}", response.message);
        }

        Ok(if is_deleted { 0 } else { 2 })
    }

    /// Checks if a single MD5 hash is known in the database and lists all associated file paths.
    pub fn check_hash(lib: &Library, hash_str: &str, json: bool) -> Result<i32, ApiError> {
        let hash_clean = hash_str.trim().to_lowercase();
        let file_hash_res = MediaHasher::from_hex(&hash_clean);

        let rtxn = lib.raw_db().env.read_txn()?;

        let mut found_ext: Option<ExternalHashRecord> = None;
        for entry in lib.raw_db().external_hashes.iter(&rtxn)? {
            let (_, rec) = entry?;
            if rec.hash.eq_ignore_ascii_case(&hash_clean) {
                found_ext = Some(rec);
                break;
            }
        }

        let mut associated_files = Vec::new();
        if let Ok(bin_hash) = file_hash_res
            && let Some(media_ids) = lib.raw_db().media_by_hash.get(&rtxn, &bin_hash[..])?
        {
            for mid in media_ids {
                if let Some(m_rec) = lib.raw_db().media.get(&rtxn, &mid)?
                    && let Some(f_rec) = lib.raw_db().folders.get(&rtxn, &m_rec.folder_id)?
                {
                    associated_files.push(format!("{}/{}", f_rec.path, m_rec.filename));
                }
            }
        }

        let is_known = found_ext.is_some() || !associated_files.is_empty();

        let response = ApiHashCheckResponse {
            success: true,
            known: is_known,
            hash: hash_clean,
            hash_id: found_ext.as_ref().map(|e| e.hash_id),
            date_time: found_ext.as_ref().map(|e| e.date_time.clone()),
            associated_files,
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else if response.known {
            println!("✓ Hash is KNOWN in database:");
            println!("  • Hash     : {}", response.hash);
            if let Some(id) = response.hash_id {
                println!("  • Hash ID  : #{}", id);
            }
            if let Some(ref dt) = response.date_time {
                println!("  • Date     : {}", dt);
            }
            if !response.associated_files.is_empty() {
                println!("  • Files ({}):", response.associated_files.len());
                for f in &response.associated_files {
                    println!("    - {}", f);
                }
            }
        } else {
            println!("Notice: Hash '{}' is NOT known in database.", response.hash);
        }

        Ok(if is_known { 0 } else { 2 })
    }

    /// High-performance batched hash check executing over a slice of hashes in a single LMDB read transaction.
    pub fn check_hashes(lib: &Library, hashes: &[String], json: bool) -> Result<i32, ApiError> {
        let t0 = Instant::now();
        let rtxn = lib.raw_db().env.read_txn()?;

        let mut external_map: HashMap<String, (u64, String)> = HashMap::new();
        for entry in lib.raw_db().external_hashes.iter(&rtxn)? {
            let (_, rec) = entry?;
            external_map.insert(rec.hash.to_lowercase(), (rec.hash_id, rec.date_time));
        }

        let mut folder_path_cache: HashMap<u64, String> = HashMap::new();

        let mut results = Vec::with_capacity(hashes.len());
        let mut known_count = 0usize;
        let mut unknown_count = 0usize;

        for raw_hash in hashes {
            let hash_clean = raw_hash.trim().to_lowercase();
            if hash_clean.is_empty() {
                continue;
            }

            let ext_info = external_map.get(&hash_clean).cloned();

            let mut associated_files = Vec::new();
            if let Ok(bin_hash) = MediaHasher::from_hex(&hash_clean)
                && let Some(media_ids) = lib.raw_db().media_by_hash.get(&rtxn, &bin_hash[..])?
            {
                for mid in media_ids {
                    if let Some(m_rec) = lib.raw_db().media.get(&rtxn, &mid)? {
                        let folder_path =
                            folder_path_cache.entry(m_rec.folder_id).or_insert_with(|| {
                                lib.raw_db()
                                    .folders
                                    .get(&rtxn, &m_rec.folder_id)
                                    .ok()
                                    .flatten()
                                    .map(|f| f.path)
                                    .unwrap_or_default()
                            });

                        if !folder_path.is_empty() {
                            associated_files.push(format!("{}/{}", folder_path, m_rec.filename));
                        } else {
                            associated_files.push(m_rec.filename);
                        }
                    }
                }
            }

            let is_known = ext_info.is_some() || !associated_files.is_empty();
            if is_known {
                known_count += 1;
            } else {
                unknown_count += 1;
            }

            results.push(ApiBatchHashCheckItem {
                hash: hash_clean,
                known: is_known,
                hash_id: ext_info.as_ref().map(|e| e.0),
                date_time: ext_info.as_ref().map(|e| e.1.clone()),
                associated_files,
            });
        }

        let elapsed_us = t0.elapsed().as_micros();

        let response = ApiBatchHashCheckResponse {
            success: true,
            total_requested: results.len(),
            known_count,
            unknown_count,
            results,
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!(
                "RustView Batch Hash Check Summary ({} requested in {} µs):",
                response.total_requested, elapsed_us
            );
            println!("  • Known in Database : {}", response.known_count);
            println!("  • Unknown / Unique  : {}", response.unknown_count);
            println!();
            for item in &response.results {
                if item.known {
                    let id_str = item
                        .hash_id
                        .map(|id| format!(" [Ext #{}]", id))
                        .unwrap_or_default();
                    println!("  ✓ {} -> KNOWN{}", item.hash, id_str);
                    for f in &item.associated_files {
                        println!("      └─ {}", f);
                    }
                } else {
                    println!("  ✗ {} -> UNKNOWN", item.hash);
                }
            }
        }

        Ok(0)
    }

    /// Inserts an external hash entry into the dedicated `external_hashes` table if not already present.
    pub fn insert_hash(lib: &Library, hash_str: &str, json: bool) -> Result<i32, ApiError> {
        let hash_clean = hash_str.trim().to_lowercase();
        if hash_clean.len() != 32 {
            return Err(ApiError::InvalidArgument(
                "Invalid MD5 hash format. Expected 32-character hexadecimal string.".into(),
            ));
        }

        let rtxn = lib.raw_db().env.read_txn()?;
        let mut existing_entry: Option<ExternalHashRecord> = None;
        for entry in lib.raw_db().external_hashes.iter(&rtxn)? {
            let (_, rec) = entry?;
            if rec.hash.eq_ignore_ascii_case(&hash_clean) {
                existing_entry = Some(rec);
                break;
            }
        }
        drop(rtxn);

        let (is_new, hash_id, date_time) = match existing_entry {
            Some(existing) => (false, existing.hash_id, existing.date_time),
            None => {
                let mut wtxn = lib.raw_db().env.write_txn()?;
                let new_id = lib.raw_db().next_id(&mut wtxn, SEQ_EXTERNAL_HASH_ID)?;
                let now_str = current_datetime_string();

                let record = ExternalHashRecord {
                    hash_id: new_id,
                    hash: hash_clean.clone(),
                    date_time: now_str.clone(),
                };

                lib.raw_db()
                    .external_hashes
                    .put(&mut wtxn, &new_id, &record)?;
                wtxn.commit()?;
                (true, new_id, now_str)
            }
        };

        let response = ApiHashInsertResponse {
            success: true,
            is_new,
            hash: hash_clean,
            hash_id,
            date_time,
            message: if is_new {
                "External hash registered successfully.".to_string()
            } else {
                "Hash was already present in database.".to_string()
            },
        };

        if json {
            println!("{}", serde_json::to_string_pretty(&response)?);
        } else {
            println!(
                "✓ Hash #{}: {} ({})",
                response.hash_id, response.hash, response.message
            );
        }

        Ok(0)
    }
}

fn split_folder_and_filename(path: &Path) -> Result<(std::path::PathBuf, String), ApiError> {
    let normalized = normalize_path(path);
    let filename = normalized
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .ok_or_else(|| ApiError::InvalidArgument("Invalid file path specified.".into()))?;

    let folder_path = normalized
        .parent()
        .map(normalize_path)
        .ok_or_else(|| ApiError::InvalidArgument("Path has no parent directory.".into()))?;

    Ok((folder_path, filename))
}

fn current_datetime_string() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
