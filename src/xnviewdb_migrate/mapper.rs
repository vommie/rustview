use super::logger::MigrationLogger;
use super::stats::MigrationStats;
use super::types::*;
use crate::library::folders::normalize_path;
use crate::library::models::{
    ExternalHashRecord, FolderId, FolderRecord, MediaId, MediaRecord, TagId, TagRecord,
};
use crate::media::{FileHash, MediaFormat, MediaHasher, MediaType, MetadataExtractor};
use crate::settings::LogObscurer;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
struct DirItemInfo {
    is_dir: bool,
    is_file: bool,
    size: u64,
    mtime: i64,
    ctime: i64,
}

struct TagReindexContext<'a> {
    children_map: &'a HashMap<Option<i64>, Vec<i64>>,
    old_tags_map: &'a HashMap<i64, XnViewTag>,
    output_tags: &'a mut Vec<TagRecord>,
    tag_id_map: &'a mut HashMap<i64, TagId>,
    next_id: &'a mut TagId,
    logger: &'a mut MigrationLogger,
}

pub struct MappedData {
    pub folders: Vec<FolderRecord>,
    pub folder_id_map: HashMap<i64, FolderId>,
    pub tags: Vec<TagRecord>,
    pub tag_id_map: HashMap<i64, TagId>,
    pub media: Vec<MediaRecord>,
    pub media_id_map: HashMap<i64, MediaId>,
    pub external_hashes: Vec<ExternalHashRecord>,
    pub tags_by_media: HashMap<MediaId, Vec<TagId>>,
    pub media_by_tag: HashMap<TagId, Vec<MediaId>>,
    pub media_by_folder: HashMap<FolderId, Vec<MediaId>>,
    pub media_by_hash: HashMap<FileHash, Vec<MediaId>>,
    pub max_folder_id: u64,
    pub max_tag_id: u32,
    pub max_media_id: u64,
    pub max_external_hash_id: u64,
}

pub struct XnViewMapper;

impl XnViewMapper {
    /// Maps, normalizes, validates, filters non-existent entities & non-root paths, logs granular events, and re-indexes all IDs into clean 1-based ranges.
    pub fn map_data(
        raw: XnViewRawData,
        skip_missing: bool,
        root_filter: Option<&Path>,
        stats: &mut MigrationStats,
        logger: &mut MigrationLogger,
    ) -> MappedData {
        let t0 = Instant::now();
        tracing::info!(
            skip_missing = skip_missing,
            root_filter = ?root_filter,
            obfuscate_logs = LogObscurer::is_active(),
            "[XnViewMapper] Starting entity filtering, root validation, contiguous ID re-indexing, and relational mapping..."
        );

        stats.extracted_folders = raw.folders.len();
        stats.extracted_media = raw.images.len();
        stats.extracted_tags = raw.tags.len();
        stats.extracted_tag_assignments = raw.tag_links.len();
        stats.extracted_external_hashes = raw.hashes.len();
        stats.hashes_table_found = raw.hashes_table_found;

        // 1. External Hashes Table Mapping (Contiguous 1-based IDs)
        let mut external_hashes = Vec::with_capacity(raw.hashes.len());
        let mut hash_lookup_map: HashMap<i64, FileHash> = HashMap::with_capacity(raw.hashes.len());

        for (idx, h) in raw.hashes.into_iter().enumerate() {
            let next_ext_hash_id = (idx + 1) as u64;
            let hash_clean = h.hash.trim().to_lowercase();
            if let Ok(file_hash) = MediaHasher::from_hex(&hash_clean) {
                hash_lookup_map.insert(h.hash_id, file_hash);
            }

            logger.log_external_hash_migrated(
                h.hash_id,
                next_ext_hash_id,
                &hash_clean,
                &h.date_time,
            );

            external_hashes.push(ExternalHashRecord {
                hash_id: next_ext_hash_id,
                hash: hash_clean,
                date_time: h.date_time,
            });
        }
        stats.total_external_hashes = external_hashes.len();

        // 2. Folder Filtering & Bulk Directory Cache
        let mut folders = Vec::with_capacity(raw.folders.len());
        let mut folder_id_map: HashMap<i64, FolderId> = HashMap::with_capacity(raw.folders.len());
        let mut folder_path_map: HashMap<FolderId, PathBuf> =
            HashMap::with_capacity(raw.folders.len());
        let mut folder_dir_entries_cache: HashMap<FolderId, HashMap<String, DirItemInfo>> =
            HashMap::new();

        let mut next_folder_id = 1u64;

        for f in raw.folders {
            let normalized_path_buf = normalize_path(Path::new(f.pathname.trim()));

            if let Some(root_path) = root_filter {
                let is_under_root =
                    normalized_path_buf == root_path || normalized_path_buf.starts_with(root_path);

                if !is_under_root {
                    stats.skipped_folders_outside_root += 1;
                    logger.log_folder_outside_root(f.folder_id, &normalized_path_buf, root_path);
                    tracing::debug!(
                        old_folder_id = f.folder_id,
                        path = %LogObscurer::path(&normalized_path_buf),
                        root = %LogObscurer::path(root_path),
                        "[XnViewMapper: Skip] Folder is outside configured base root directory."
                    );
                    continue;
                }
            }

            let exists_on_disk = normalized_path_buf.is_dir();
            if skip_missing && !exists_on_disk {
                stats.skipped_folders_missing += 1;
                logger.log_folder_skipped(
                    f.folder_id,
                    &normalized_path_buf,
                    "Directory does not exist on disk",
                );
                tracing::debug!(
                    old_folder_id = f.folder_id,
                    path = %LogObscurer::path(&normalized_path_buf),
                    "[XnViewMapper: Skip] Folder does not exist on disk. Skipping folder and all child entries."
                );
                continue;
            }

            let new_folder_id = next_folder_id;
            next_folder_id += 1;

            let normalized_path_str = normalized_path_buf.to_string_lossy().to_string();
            folder_id_map.insert(f.folder_id, new_folder_id);
            folder_path_map.insert(new_folder_id, normalized_path_buf.clone());

            logger.log_folder_migrated(f.folder_id, new_folder_id, &normalized_path_str);

            if exists_on_disk {
                let mut dir_items = HashMap::new();
                if let Ok(entries) = std::fs::read_dir(&normalized_path_buf) {
                    for entry in entries.flatten() {
                        if let Ok(ft) = entry.file_type() {
                            let name = entry.file_name().to_string_lossy().to_string();
                            let is_dir = ft.is_dir();
                            let is_file = ft.is_file() || ft.is_symlink();
                            let (size, mtime, ctime) = if is_file {
                                entry
                                    .metadata()
                                    .map(|m| {
                                        let s = m.len();
                                        let mt = m
                                            .modified()
                                            .ok()
                                            .and_then(|t| {
                                                t.duration_since(std::time::UNIX_EPOCH).ok()
                                            })
                                            .map(|d| d.as_secs() as i64)
                                            .unwrap_or(0);
                                        let ct = m
                                            .created()
                                            .ok()
                                            .and_then(|t| {
                                                t.duration_since(std::time::UNIX_EPOCH).ok()
                                            })
                                            .map(|d| d.as_secs() as i64)
                                            .unwrap_or(mt);
                                        (s, mt, ct)
                                    })
                                    .unwrap_or((0, 0, 0))
                            } else {
                                (0, 0, 0)
                            };
                            dir_items.insert(
                                name,
                                DirItemInfo {
                                    is_dir,
                                    is_file,
                                    size,
                                    mtime,
                                    ctime,
                                },
                            );
                        }
                    }
                }
                folder_dir_entries_cache.insert(new_folder_id, dir_items);
            }

            folders.push(FolderRecord {
                id: new_folder_id,
                path: normalized_path_str,
                added_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
                is_deleted: false,
            });
        }
        stats.total_folders = folders.len();

        // 3. Hierarchical Tag Ordering, Re-indexing & Visual Tree Logging
        let (tags, tag_id_map, tag_tree_roots) = Self::reindex_tags(raw.tags, logger);
        stats.total_tags = tags.len();

        logger.log_tag_tree(&tag_tree_roots);

        tracing::info!(
            tags_count = tags.len(),
            root_tags_count = tag_tree_roots.len(),
            "[XnViewMapper] Hierarchical tag tree re-indexed and logged to audit trail"
        );
        // 4. Media Filtering & Resolving via in-memory O(1) Cache
        let mut media = Vec::with_capacity(raw.images.len());
        let mut media_id_map: HashMap<i64, MediaId> = HashMap::with_capacity(raw.images.len());
        let mut media_by_folder: HashMap<FolderId, Vec<MediaId>> = HashMap::new();
        let mut media_by_hash: HashMap<FileHash, Vec<MediaId>> = HashMap::new();
        let mut next_media_id = 1u64;

        for img in raw.images {
            let Some(&mapped_folder_id) = folder_id_map.get(&img.folder_id) else {
                if root_filter.is_some() {
                    stats.skipped_media_outside_root += 1;
                    logger.log_media_outside_root(
                        img.image_id,
                        &img.filename,
                        &format!("XnView Folder #{}", img.folder_id),
                    );
                } else {
                    stats.skipped_media_missing_folder += 1;
                    logger.log_media_skipped(
                        img.image_id,
                        &img.filename,
                        &format!("XnView Folder #{}", img.folder_id),
                        "Parent folder was skipped (does not exist on disk)",
                    );
                }
                continue;
            };

            let folder_dir = folder_path_map.get(&mapped_folder_id);
            let dir_cache = folder_dir_entries_cache.get(&mapped_folder_id);
            let folder_display = folder_dir
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let item_in_cache = dir_cache.and_then(|c| c.get(&img.filename));

            if let Some(info) = item_in_cache
                && info.is_dir
            {
                stats.skipped_subfolders_in_images += 1;
                logger.log_subfolder_entry_skipped(img.image_id, &img.filename, &folder_display);
                continue;
            }

            let format = MediaFormat::from_path(Path::new(&img.filename));
            if !format.is_supported() && item_in_cache.is_some_and(|i| i.is_file) {
                stats.skipped_unsupported_media += 1;
                logger.log_unsupported_file_skipped(img.image_id, &img.filename, &folder_display);
                continue;
            }

            let file_physically_exists = item_in_cache.is_some_and(|i| i.is_file);
            if skip_missing && !file_physically_exists {
                stats.skipped_media_missing_file += 1;
                logger.log_media_skipped(
                    img.image_id,
                    &img.filename,
                    &folder_display,
                    "Media file not found in folder directory on disk",
                );
                continue;
            }

            let new_media_id = next_media_id;
            next_media_id += 1;
            media_id_map.insert(img.image_id, new_media_id);

            let file_path = folder_dir.map(|p| p.join(&img.filename));
            let media_type = format.media_type().unwrap_or(MediaType::Image);

            let (mut width, mut height, mut duration_ms) =
                if let Some(info) = raw.image_info.get(&img.image_id) {
                    (info.width, info.height, info.duration)
                } else {
                    (0, 0, 0)
                };

            let mut file_size = img.size.max(0) as u64;
            let mut mtime = img.modified_date.max(0);
            let mut ctime = mtime;

            if let Some(cached_info) = item_in_cache {
                if file_size == 0 {
                    file_size = cached_info.size;
                }
                if mtime == 0 {
                    mtime = cached_info.mtime;
                }
                ctime = cached_info.ctime;
            }

            let file_hash = if let Some(hash_id) = img.hash_id
                && let Some(&h) = hash_lookup_map.get(&hash_id)
            {
                stats.resolved_hashes_from_table += 1;
                h
            } else if let Some(ref path) = file_path
                && file_physically_exists
            {
                if (width == 0
                    || height == 0
                    || (media_type == MediaType::Video && duration_ms == 0))
                    && let Ok(raw_meta) = MetadataExtractor::extract(path)
                {
                    width = raw_meta.width;
                    height = raw_meta.height;
                    duration_ms = raw_meta.duration_ms;
                }

                match MediaHasher::hash_file(path) {
                    Ok(computed) => {
                        stats.resolved_hashes_computed_live += 1;
                        computed
                    }
                    Err(_) => {
                        stats.fallback_hashes_generated += 1;
                        MediaHasher::hash_bytes(path.to_string_lossy().as_bytes())
                    }
                }
            } else {
                stats.fallback_hashes_generated += 1;
                let fallback_seed = format!("{}/{}", mapped_folder_id, img.filename);
                MediaHasher::hash_bytes(fallback_seed.as_bytes())
            };

            let hash_hex = MediaHasher::to_hex(&file_hash);
            logger.log_media_migrated(
                img.image_id,
                new_media_id,
                mapped_folder_id,
                &img.filename,
                img.rating.clamp(0, 5) as u8,
                &hash_hex,
            );

            let is_corrupted = file_size == 0 || (width == 0 && height == 0);

            let record = MediaRecord {
                id: new_media_id,
                folder_id: mapped_folder_id,
                filename: img.filename,
                file_hash,
                file_size,
                modification_time: mtime,
                creation_time: ctime,
                duration_ms,
                media_type,
                format,
                width,
                height,
                rating: (img.rating.clamp(0, 5)) as u8,
                last_seen: 1,
                is_deleted: false,
                is_corrupted,
                source: None,
            };

            media_by_folder
                .entry(mapped_folder_id)
                .or_default()
                .push(new_media_id);

            media_by_hash
                .entry(file_hash)
                .or_default()
                .push(new_media_id);

            media.push(record);
        }
        stats.total_media = media.len();

        // 5. Relational Tag Associations (TagsTree) Mapping
        let mut tags_by_media: HashMap<MediaId, Vec<TagId>> = HashMap::new();
        let mut media_by_tag: HashMap<TagId, Vec<MediaId>> = HashMap::new();
        let mut mapped_link_count = 0usize;

        for link in raw.tag_links {
            if let (Some(&new_media_id), Some(&new_tag_id)) = (
                media_id_map.get(&link.image_id),
                tag_id_map.get(&link.tag_id),
            ) {
                let m_tags = tags_by_media.entry(new_media_id).or_default();
                if !m_tags.contains(&new_tag_id) {
                    m_tags.push(new_tag_id);
                }

                let t_media = media_by_tag.entry(new_tag_id).or_default();
                if !t_media.contains(&new_media_id) {
                    t_media.push(new_media_id);
                }

                logger.log_tag_assignment_migrated(
                    new_media_id,
                    new_tag_id,
                    link.image_id,
                    link.tag_id,
                );
                mapped_link_count += 1;
            } else {
                stats.skipped_tag_assignments_orphaned += 1;
                logger.log_tag_assignment_skipped(
                    link.image_id,
                    link.tag_id,
                    "Associated media or tag was skipped/missing",
                );
            }
        }
        stats.total_tag_assignments = mapped_link_count;

        let max_folder_id = folders.len() as u64;
        let max_tag_id = tags.len() as u32;
        let max_media_id = media.len() as u64;
        let max_external_hash_id = external_hashes.len() as u64;

        tracing::info!(
            folders = folders.len(),
            tags = tags.len(),
            media = media.len(),
            external_hashes = external_hashes.len(),
            tag_assignments = mapped_link_count,
            elapsed_ms = t0.elapsed().as_millis(),
            "[XnViewMapper] Complete contiguous re-indexing finished"
        );

        MappedData {
            folders,
            folder_id_map,
            tags,
            tag_id_map,
            media,
            media_id_map,
            external_hashes,
            tags_by_media,
            media_by_tag,
            media_by_folder,
            media_by_hash,
            max_folder_id,
            max_tag_id,
            max_media_id,
            max_external_hash_id,
        }
    }

    fn reindex_tags(
        raw_tags: Vec<XnViewTag>,
        logger: &mut MigrationLogger,
    ) -> (
        Vec<TagRecord>,
        HashMap<i64, TagId>,
        Vec<MigratedTagTreeNode>,
    ) {
        let mut old_tags_map: HashMap<i64, XnViewTag> = HashMap::new();
        let mut children_map: HashMap<Option<i64>, Vec<i64>> = HashMap::new();

        for t in raw_tags {
            children_map.entry(t.parent_id).or_default().push(t.tag_id);
            old_tags_map.insert(t.tag_id, t);
        }

        for list in children_map.values_mut() {
            list.sort();
        }

        let mut reindexed_tags = Vec::with_capacity(old_tags_map.len());
        let mut tag_id_map = HashMap::with_capacity(old_tags_map.len());
        let mut next_tag_id = 1u32;

        let mut ctx = TagReindexContext {
            children_map: &children_map,
            old_tags_map: &old_tags_map,
            output_tags: &mut reindexed_tags,
            tag_id_map: &mut tag_id_map,
            next_id: &mut next_tag_id,
            logger,
        };

        let root_nodes = Self::traverse_and_reindex(None, None, &mut ctx);

        (reindexed_tags, tag_id_map, root_nodes)
    }

    fn traverse_and_reindex(
        current_old_parent: Option<i64>,
        current_new_parent: Option<TagId>,
        ctx: &mut TagReindexContext<'_>,
    ) -> Vec<MigratedTagTreeNode> {
        let mut nodes = Vec::new();
        if let Some(children) = ctx.children_map.get(&current_old_parent) {
            for &child_old_id in children {
                if let Some(old_tag) = ctx.old_tags_map.get(&child_old_id) {
                    let new_id = *ctx.next_id;
                    *ctx.next_id += 1;

                    ctx.tag_id_map.insert(child_old_id, new_id);

                    let clean_name = old_tag.label.trim().to_string();
                    let tag_name = if clean_name.is_empty() {
                        format!("Tag_{}", new_id)
                    } else {
                        clean_name
                    };

                    ctx.logger.log_tag_migrated(
                        child_old_id,
                        new_id,
                        &tag_name,
                        current_new_parent,
                    );

                    ctx.output_tags.push(TagRecord {
                        id: new_id,
                        parent_id: current_new_parent,
                        name: tag_name.clone(),
                    });

                    let child_tree =
                        Self::traverse_and_reindex(Some(child_old_id), Some(new_id), ctx);

                    nodes.push(MigratedTagTreeNode {
                        old_id: child_old_id,
                        new_id,
                        name: tag_name,
                        children: child_tree,
                    });
                }
            }
        }
        nodes
    }
}
