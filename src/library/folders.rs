use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::models::{CachedFolderNode, FolderHierarchySnapshot, FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use crate::media::MediaFormat;
use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Normalizes a filesystem path by resolving symlinks/canonical form and removing any trailing slash.
pub fn normalize_path(path: &Path) -> PathBuf {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => {
            let mut normalized = PathBuf::new();
            for comp in path.components() {
                match comp {
                    std::path::Component::CurDir => {}
                    _ => normalized.push(comp.as_os_str()),
                }
            }
            normalized
        }
    };
    let s = canonical.to_string_lossy();
    if s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        PathBuf::from(s.trim_end_matches(['/', '\\']))
    } else {
        canonical
    }
}

pub struct FolderManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Loads the entire folder hierarchy into an immutable in-memory Single Point of Truth snapshot in one read transaction.
    pub fn load_hierarchy_snapshot(&self) -> Result<FolderHierarchySnapshot, LibraryError> {
        let t0 = Instant::now();
        let rtxn = self.db.env.read_txn()?;
        let mut nodes = HashMap::new();
        let mut children_map: HashMap<Option<FolderId>, Vec<FolderId>> = HashMap::new();
        let mut path_index = HashMap::new();
        let mut root_ids = Vec::new();

        for entry in self.db.folders.iter(&rtxn)? {
            let (id, rec) = entry?;
            let path_buf = PathBuf::from(&rec.path);
            path_index.insert(path_buf.clone(), id);

            let children_ids = self
                .db
                .folders_by_parent
                .get(&rtxn, &id)?
                .unwrap_or_default();

            let node = CachedFolderNode {
                id,
                parent_id: rec.parent_id,
                name: rec.name,
                path: path_buf,
                depth: rec.depth,
                is_hidden: rec.is_hidden,
                is_deleted: rec.is_deleted,
                subfolder_count: rec.subfolder_count,
                media_count: rec.media_count,
                recursive_media_count: rec.recursive_media_count,
                children: children_ids.clone(),
            };

            if rec.parent_id.is_none() {
                root_ids.push(id);
            }

            children_map
                .entry(rec.parent_id)
                .or_default()
                .push(id);

            nodes.insert(id, node);
        }

        // Sort children alphabetically by name
        for list in children_map.values_mut() {
            list.sort_by(|&a, &b| {
                let name_a = nodes.get(&a).map(|n| n.name.as_str()).unwrap_or("");
                let name_b = nodes.get(&b).map(|n| n.name.as_str()).unwrap_or("");
                name_a.cmp(name_b)
            });
        }
        root_ids.sort_by(|&a, &b| {
            let name_a = nodes.get(&a).map(|n| n.name.as_str()).unwrap_or("");
            let name_b = nodes.get(&b).map(|n| n.name.as_str()).unwrap_or("");
            name_a.cmp(name_b)
        });

        tracing::debug!(
            total_nodes = nodes.len(),
            root_nodes = root_ids.len(),
            elapsed_us = t0.elapsed().as_micros(),
            "[FolderHierarchy: Snapshot] Loaded complete in-memory SPoT folder hierarchy snapshot"
        );

        Ok(FolderHierarchySnapshot {
            nodes,
            children_map,
            path_index,
            root_ids,
        })
    }

    /// Full root directory scan: walks the entire folder tree, parses local `.hidden` files, counts supported media,
    /// performs bottom-up aggregation of counts, and updates LMDB with relational indices.
    pub fn scan_directory_tree<F>(
        &self,
        root_path: &Path,
        progress_callback: Option<F>,
    ) -> Result<FolderHierarchySnapshot, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let t0 = Instant::now();
        let canonical_root = normalize_path(root_path);
        tracing::info!(
            root_path = %LogObscurer::path(&canonical_root),
            "[FolderScan: FullRoot] Starting full filesystem hierarchy scan..."
        );

        let counter = Arc::new(AtomicUsize::new(1));
        let cb_arc: Option<Arc<dyn Fn(usize) + Send + Sync>> = progress_callback.map(|cb| Arc::new(cb) as Arc<dyn Fn(usize) + Send + Sync>);

        struct ScannedDirItem {
            path: PathBuf,
            name: String,
            parent_path: Option<PathBuf>,
            depth: u32,
            is_hidden: bool,
            media_files_count: u32,
        }

        fn walk_dir(
            dir: &Path,
            parent: Option<&Path>,
            depth: u32,
            is_parent_hidden: bool,
            counter: &Arc<AtomicUsize>,
            cb: Option<&Arc<dyn Fn(usize) + Send + Sync>>,
            collector: &mut Vec<ScannedDirItem>,
        ) {
            let dir_name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            let starts_with_dot = dir_name.starts_with('.');

            let mut hidden_set = HashSet::new();
            let dot_hidden_path = dir.join(".hidden");
            if dot_hidden_path.is_file()
                && let Ok(content) = std::fs::read_to_string(&dot_hidden_path)
            {
                for line in content.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        hidden_set.insert(trimmed.to_string());
                    }
                }
            }

            let mut sub_dirs = Vec::new();
            let mut media_count = 0u32;

            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let file_type = entry.file_type();
                    let is_dir = file_type.as_ref().map(|ft| ft.is_dir()).unwrap_or(false);
                    let is_file = file_type.as_ref().map(|ft| ft.is_file()).unwrap_or(false);
                    let entry_name = entry.file_name().to_string_lossy().to_string();

                    if is_dir {
                        let child_path = entry.path();
                        let child_hidden = starts_with_dot
                            || is_parent_hidden
                            || entry_name.starts_with('.')
                            || hidden_set.contains(&entry_name);
                        sub_dirs.push((child_path, child_hidden));
                    } else if is_file {
                        let format = MediaFormat::from_path(Path::new(&entry_name));
                        if format.is_supported() {
                            media_count += 1;
                        }
                    }
                }
            } else {
                tracing::warn!(
                    dir = %LogObscurer::path(dir),
                    "[FolderScan: Warning] Unable to read directory entries from filesystem"
                );
            }

            collector.push(ScannedDirItem {
                path: dir.to_path_buf(),
                name: dir_name,
                parent_path: parent.map(|p| p.to_path_buf()),
                depth,
                is_hidden: is_parent_hidden || starts_with_dot,
                media_files_count: media_count,
            });

            let cur = counter.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(callback) = cb
                && (cur.is_multiple_of(32) || cur == 1)
            {
                callback(cur);
            }

            for (sub_p, sub_hidden) in sub_dirs {
                walk_dir(
                    &sub_p,
                    Some(dir),
                    depth + 1,
                    sub_hidden,
                    counter,
                    cb,
                    collector,
                );
            }
        }

        let mut scanned_items = Vec::with_capacity(1024);
        walk_dir(
            &canonical_root,
            None,
            0,
            canonical_root
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false),
            &counter,
            cb_arc.as_ref(),
            &mut scanned_items,
        );

        if let Some(ref cb) = cb_arc {
            cb(counter.load(Ordering::Relaxed));
        }

        let mut wtxn = self.db.env.write_txn()?;

        let mut existing_folders: HashMap<String, (FolderId, FolderRecord)> = HashMap::new();
        for entry in self.db.folders.iter(&wtxn)? {
            let (id, rec) = entry?;
            let path_obj = Path::new(&rec.path);
            if path_obj.starts_with(&canonical_root) {
                existing_folders.insert(rec.path.clone(), (id, rec));
            }
        }

        let mut path_to_id: HashMap<PathBuf, FolderId> = HashMap::new();
        let mut new_records: Vec<FolderRecord> = Vec::with_capacity(scanned_items.len());

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        for item in &scanned_items {
            let p_str = item.path.to_string_lossy().to_string();
            let folder_id = if let Some((id, _)) = existing_folders.remove(&p_str) {
                id
            } else {
                let new_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
                self.db.folders_by_path.put(&mut wtxn, &p_str, &new_id)?;
                self.db.media_by_folder.put(&mut wtxn, &new_id, &Vec::new())?;
                new_id
            };

            path_to_id.insert(item.path.clone(), folder_id);

            new_records.push(FolderRecord {
                id: folder_id,
                parent_id: None,
                name: item.name.clone(),
                path: p_str,
                depth: item.depth,
                is_hidden: item.is_hidden,
                is_deleted: false,
                subfolder_count: 0,
                media_count: item.media_files_count,
                recursive_media_count: item.media_files_count,
                added_at: now_secs,
                last_scanned_at: now_secs,
            });
        }

        for (id, mut missing_rec) in existing_folders.into_values() {
            if !missing_rec.is_deleted {
                tracing::info!(
                    folder_id = id,
                    path = %LogObscurer::path(Path::new(&missing_rec.path)),
                    "[FolderScan: Missing] Folder missing from filesystem, marked is_deleted = true"
                );
                missing_rec.is_deleted = true;
                self.db.folders.put(&mut wtxn, &id, &missing_rec)?;
            }
        }

        let mut parent_to_children: HashMap<FolderId, Vec<FolderId>> = HashMap::new();

        for (idx, item) in scanned_items.iter().enumerate() {
            let folder_id = path_to_id[&item.path];
            let parent_id = item
                .parent_path
                .as_ref()
                .and_then(|p| path_to_id.get(p).copied());

            new_records[idx].parent_id = parent_id;

            if let Some(pid) = parent_id {
                parent_to_children.entry(pid).or_default().push(folder_id);
            }
        }

        let mut id_to_record_idx: HashMap<FolderId, usize> = HashMap::new();
        for (idx, r) in new_records.iter().enumerate() {
            id_to_record_idx.insert(r.id, idx);
        }

        let mut depth_sorted_indices: Vec<usize> = (0..new_records.len()).collect();
        depth_sorted_indices.sort_by(|&a, &b| new_records[b].depth.cmp(&new_records[a].depth));

        for idx in depth_sorted_indices {
            let fid = new_records[idx].id;
            let children = parent_to_children.get(&fid).cloned().unwrap_or_default();
            new_records[idx].subfolder_count = children.len() as u32;

            let mut rec_media = new_records[idx].media_count;
            for child_id in children {
                if let Some(&c_idx) = id_to_record_idx.get(&child_id) {
                    rec_media += new_records[c_idx].recursive_media_count;
                }
            }
            new_records[idx].recursive_media_count = rec_media;
        }

        for r in &new_records {
            self.db.folders.put(&mut wtxn, &r.id, r)?;
            let children = parent_to_children.get(&r.id).cloned().unwrap_or_default();
            self.db
                .folders_by_parent
                .put(&mut wtxn, &r.id, &children)?;
        }

        wtxn.commit()?;

        tracing::info!(
            root_path = %LogObscurer::path(&canonical_root),
            scanned_folders = new_records.len(),
            elapsed_ms = t0.elapsed().as_millis(),
            "[FolderScan: FullRootDone] Full directory scan committed to LMDB"
        );

        self.load_hierarchy_snapshot()
    }

    /// Surgical targeted scan: Scans ONLY the given subfolder on disk, reconciles changes,
    /// computes deltas, and rolls them up through the parent chain up to root in LMDB.
    pub fn scan_subtree<F>(
        &self,
        subtree_root_id: FolderId,
        progress_callback: Option<F>,
    ) -> Result<FolderHierarchySnapshot, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let t0 = Instant::now();
        let rtxn = self.db.env.read_txn()?;
        let target_rec = self
            .db
            .folders
            .get(&rtxn, &subtree_root_id)?
            .ok_or(LibraryError::FolderNotFound(subtree_root_id))?;
        drop(rtxn);

        let target_path = PathBuf::from(&target_rec.path);
        if !target_path.is_dir() {
            tracing::warn!(
                subtree_root_id,
                path = %LogObscurer::path(&target_path),
                "[FolderScan: SubtreeMissing] Target subtree path is missing on physical disk"
            );
            return Err(LibraryError::FolderNotFound(subtree_root_id));
        }

        tracing::info!(
            subtree_root_id,
            path = %LogObscurer::path(&target_path),
            "[FolderScan: SubtreeStart] Executing surgical targeted subtree rescan..."
        );

        let old_recursive_media = target_rec.recursive_media_count as i64;
        let old_subfolders = target_rec.subfolder_count as i64;

        let _ = self.scan_directory_tree(&target_path, progress_callback)?;

        let rtxn2 = self.db.env.read_txn()?;
        let updated_target = self
            .db
            .folders
            .get(&rtxn2, &subtree_root_id)?
            .ok_or(LibraryError::FolderNotFound(subtree_root_id))?;
        drop(rtxn2);

        let delta_media = updated_target.recursive_media_count as i64 - old_recursive_media;
        let delta_subfolders = updated_target.subfolder_count as i64 - old_subfolders;

        if delta_media != 0 && updated_target.parent_id.is_some() {
            let mut wtxn = self.db.env.write_txn()?;
            let mut current_pid = updated_target.parent_id;

            while let Some(pid) = current_pid {
                if let Some(mut ancestor) = self.db.folders.get(&wtxn, &pid)? {
                    let adjusted = (ancestor.recursive_media_count as i64 + delta_media).max(0) as u32;
                    ancestor.recursive_media_count = adjusted;
                    self.db.folders.put(&mut wtxn, &pid, &ancestor)?;
                    current_pid = ancestor.parent_id;
                } else {
                    break;
                }
            }

            wtxn.commit()?;
            tracing::info!(
                subtree_root_id,
                delta_media,
                delta_subfolders,
                "[FolderScan: DeltaRollup] Rolled up delta media counts through parent chain"
            );
        }

        tracing::info!(
            subtree_root_id,
            elapsed_ms = t0.elapsed().as_millis(),
            "[FolderScan: SubtreeDone] Surgical subtree rescan completed successfully"
        );

        self.load_hierarchy_snapshot()
    }

    /// Transaction helper: Adjusts direct media count for `folder_id` and rolls delta up the parent chain to root.
    pub fn adjust_folder_media_count_txn(
        db: &LibraryDatabase,
        wtxn: &mut heed::RwTxn,
        folder_id: FolderId,
        delta: i64,
    ) -> Result<(), LibraryError> {
        if delta == 0 {
            return Ok(());
        }

        if let Some(mut folder_rec) = db.folders.get(wtxn, &folder_id)? {
            folder_rec.media_count = (folder_rec.media_count as i64 + delta).max(0) as u32;
            folder_rec.recursive_media_count = (folder_rec.recursive_media_count as i64 + delta).max(0) as u32;
            db.folders.put(wtxn, &folder_id, &folder_rec)?;

            let mut current_pid = folder_rec.parent_id;
            while let Some(pid) = current_pid {
                if let Some(mut ancestor) = db.folders.get(wtxn, &pid)? {
                    ancestor.recursive_media_count = (ancestor.recursive_media_count as i64 + delta).max(0) as u32;
                    db.folders.put(wtxn, &pid, &ancestor)?;
                    current_pid = ancestor.parent_id;
                } else {
                    break;
                }
            }
        } else {
            tracing::warn!(
                folder_id,
                "[FolderHierarchy: AdjustCount] Target folder not found in LMDB during count adjustment"
            );
        }

        Ok(())
    }

    pub fn add_folder(&self, path: &Path) -> Result<FolderRecord, LibraryError> {
        let canonical_path = normalize_path(path);
        let path_str = canonical_path.to_string_lossy().to_string();

        let mut wtxn = self.db.env.write_txn()?;

        if let Some(existing_id) = self.db.folders_by_path.get(&wtxn, &path_str)? {
            if let Some(mut record) = self.db.folders.get(&wtxn, &existing_id)? {
                if record.is_deleted {
                    record.is_deleted = false;
                    self.db.folders.put(&mut wtxn, &existing_id, &record)?;
                    wtxn.commit()?;
                }
                return Ok(record);
            }
            return Err(LibraryError::DuplicateFolder(path_str));
        }

        let parent_path_opt = canonical_path.parent().map(normalize_path);
        let parent_rec = if let Some(ref p) = parent_path_opt {
            let p_str = p.to_string_lossy().to_string();
            if let Some(pid) = self.db.folders_by_path.get(&wtxn, &p_str)? {
                self.db.folders.get(&wtxn, &pid)?
            } else {
                None
            }
        } else {
            None
        };

        let parent_id = parent_rec.as_ref().map(|p| p.id);
        let depth = parent_rec.as_ref().map(|p| p.depth + 1).unwrap_or(0);
        let name = canonical_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.clone());

        let is_hidden = name.starts_with('.') || parent_rec.as_ref().map(|p| p.is_hidden).unwrap_or(false);

        let folder_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
        let record = FolderRecord {
            id: folder_id,
            parent_id,
            name,
            path: path_str.clone(),
            depth,
            is_hidden,
            is_deleted: false,
            subfolder_count: 0,
            media_count: 0,
            recursive_media_count: 0,
            added_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            last_scanned_at: 0,
        };

        self.db.folders.put(&mut wtxn, &folder_id, &record)?;
        self.db
            .folders_by_path
            .put(&mut wtxn, &path_str, &folder_id)?;
        self.db
            .media_by_folder
            .put(&mut wtxn, &folder_id, &Vec::new())?;

        if let Some(pid) = parent_id {
            let mut siblings = self
                .db
                .folders_by_parent
                .get(&wtxn, &pid)?
                .unwrap_or_default();
            if !siblings.contains(&folder_id) {
                siblings.push(folder_id);
                self.db
                    .folders_by_parent
                    .put(&mut wtxn, &pid, &siblings)?;
            }

            if let Some(mut p_rec) = self.db.folders.get(&wtxn, &pid)? {
                p_rec.subfolder_count = siblings.len() as u32;
                self.db.folders.put(&mut wtxn, &pid, &p_rec)?;
            }
        }

        wtxn.commit()?;
        Ok(record)
    }

    pub fn get_folder(&self, id: FolderId) -> Result<Option<FolderRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        Ok(self.db.folders.get(&rtxn, &id)?)
    }

    pub fn get_folder_by_path(&self, path: &Path) -> Result<Option<FolderRecord>, LibraryError> {
        let canonical_path = normalize_path(path);
        let path_str = canonical_path.to_string_lossy().to_string();

        let rtxn = self.db.env.read_txn()?;
        if let Some(id) = self.db.folders_by_path.get(&rtxn, &path_str)? {
            Ok(self.db.folders.get(&rtxn, &id)?)
        } else {
            Ok(None)
        }
    }

    /// Recursively collects `folder_id` and all descendant IDs using relational parent indices.
    pub fn get_descendant_folder_ids(&self, folder_id: FolderId) -> Result<Vec<FolderId>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut ids = vec![folder_id];
        let mut queue = vec![folder_id];

        while let Some(current) = queue.pop() {
            if let Some(children) = self.db.folders_by_parent.get(&rtxn, &current)? {
                for cid in children {
                    if let Some(rec) = self.db.folders.get(&rtxn, &cid)?
                        && !rec.is_deleted
                    {
                        ids.push(cid);
                        queue.push(cid);
                    }
                }
            }
        }

        Ok(ids)
    }

    pub fn list_folders(&self) -> Result<Vec<FolderRecord>, LibraryError> {
        let rtxn = self.db.env.read_txn()?;
        let mut folders = Vec::new();
        for entry in self.db.folders.iter(&rtxn)? {
            let (_, record) = entry?;
            folders.push(record);
        }
        Ok(folders)
    }

    pub fn set_deleted(&self, id: FolderId, is_deleted: bool) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;
        if let Some(mut record) = self.db.folders.get(&wtxn, &id)? {
            if record.is_deleted != is_deleted {
                record.is_deleted = is_deleted;
                self.db.folders.put(&mut wtxn, &id, &record)?;
            }
        } else {
            return Err(LibraryError::FolderNotFound(id));
        }
        wtxn.commit()?;
        Ok(())
    }

    pub fn remove_folder(&self, id: FolderId) -> Result<(), LibraryError> {
        let mut wtxn = self.db.env.write_txn()?;

        if let Some(folder) = self.db.folders.get(&wtxn, &id)? {
            if let Some(pid) = folder.parent_id
                && let Some(mut siblings) = self.db.folders_by_parent.get(&wtxn, &pid)?
            {
                siblings.retain(|&x| x != id);
                self.db.folders_by_parent.put(&mut wtxn, &pid, &siblings)?;
            }

            if let Some(media_ids) = self.db.media_by_folder.get(&wtxn, &id)? {
                for media_id in media_ids {
                    if let Some(media_record) = self.db.media.get(&wtxn, &media_id)? {
                        self.db.update_media_hash_index(
                            &mut wtxn,
                            media_id,
                            Some(&media_record.file_hash),
                            None,
                        )?;
                    }
                    self.db.media.delete(&mut wtxn, &media_id)?;
                    self.db.tags_by_media.delete(&mut wtxn, &media_id)?;
                }
            }

            self.db.folders_by_parent.delete(&mut wtxn, &id)?;
            self.db.media_by_folder.delete(&mut wtxn, &id)?;
            self.db.folders_by_path.delete(&mut wtxn, &folder.path)?;
            self.db.folders.delete(&mut wtxn, &id)?;
        }

        wtxn.commit()?;
        Ok(())
    }
}
