use super::db::LibraryDatabase;
use super::error::LibraryError;
use super::folder_mount_filter::FolderMountFilter;
use super::folder_snapshot::FolderSnapshotBuilder;
use super::folder_walker::{ScanProgressCallback, ScannedDirItem, walk_directory_tree};
use super::models::{FolderHierarchySnapshot, FolderId, FolderRecord};
use super::schema::SEQ_FOLDER_ID;
use crate::settings::{DEFAULT_IGNORE_NESTED_MOUNTS, GeneralSettings, LogObscurer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

/// Normalizes a filesystem path by resolving symlinks/canonical form and removing any trailing slash.
pub fn normalize_path(path: &Path) -> PathBuf {
    let canonical = if let Ok(p) = path.canonicalize() {
        p
    } else {
        let mut normalized = PathBuf::new();
        for comp in path.components() {
            match comp {
                std::path::Component::CurDir => {}
                _ => normalized.push(comp.as_os_str()),
            }
        }
        normalized
    };
    let s = canonical.to_string_lossy();
    if s.len() > 1 && (s.ends_with('/') || s.ends_with('\\')) {
        PathBuf::from(s.trim_end_matches(['/', '\\']))
    } else {
        canonical
    }
}

/// User-configurable options for a manual folder tree scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderScanOptions {
    /// Skip mount points located below the scan root (the system mount table is read on every scan).
    pub ignore_nested_mounts: bool,
}

impl Default for FolderScanOptions {
    fn default() -> Self {
        Self {
            ignore_nested_mounts: DEFAULT_IGNORE_NESTED_MOUNTS,
        }
    }
}

impl FolderScanOptions {
    /// Derives the scan options from the general settings.
    pub fn from_general(general: &GeneralSettings) -> Self {
        Self {
            ignore_nested_mounts: general.ignore_nested_mounts,
        }
    }
}

/// Describes how a scan root attaches to the existing hierarchy.
/// Full root scans use `parent_id = None` / `base_depth = 0`; subtree scans keep the existing parent link and depth.
struct ScanAnchor {
    parent_id: Option<FolderId>,
    base_depth: u32,
}

/// Outcome of the database reconciliation phase of a scan.
struct ScanOutcome {
    scanned_folders: usize,
}

pub struct FolderManager<'a> {
    db: &'a LibraryDatabase,
}

impl<'a> FolderManager<'a> {
    pub fn new(db: &'a LibraryDatabase) -> Self {
        Self { db }
    }

    /// Loads the folder hierarchy of the configured root into an immutable in-memory Single Point of Truth snapshot.
    /// Returns an empty snapshot if no root is configured or the root is not registered yet.
    pub fn load_hierarchy_snapshot(&self) -> Result<FolderHierarchySnapshot, LibraryError> {
        FolderSnapshotBuilder::new(self.db).build()
    }

    /// Full root directory scan: walks the entire folder tree, parses local `.hidden` files, counts supported media,
    /// performs bottom-up aggregation of counts, and updates LMDB with relational indices.
    /// MANUAL ONLY: this must only be triggered by explicit user refresh actions, never automatically.
    pub fn scan_directory_tree<F>(
        &self,
        root_path: &Path,
        options: FolderScanOptions,
        progress_callback: Option<F>,
    ) -> Result<FolderHierarchySnapshot, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let anchor = ScanAnchor {
            parent_id: None,
            base_depth: 0,
        };
        let outcome = self.run_scan(root_path, anchor, options, progress_callback)?;
        tracing::debug!(
            scanned_folders = outcome.scanned_folders,
            "[FolderScan: FullRootSnapshot] Reloading SPoT snapshot after full scan"
        );
        self.load_hierarchy_snapshot()
    }

    /// Surgical targeted scan: Scans ONLY the given subfolder on disk, reconciles changes,
    /// computes deltas, and rolls them up through the parent chain up to root in LMDB.
    /// MANUAL ONLY: this must only be triggered by explicit user refresh actions, never automatically.
    pub fn scan_subtree<F>(
        &self,
        subtree_root_id: FolderId,
        options: FolderScanOptions,
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
            parent_id = ?target_rec.parent_id,
            depth = target_rec.depth,
            "[FolderScan: SubtreeStart] Executing surgical targeted subtree rescan..."
        );

        let old_recursive_media = i64::from(target_rec.recursive_media_count);
        let old_subfolders = i64::from(target_rec.subfolder_count);

        let anchor = ScanAnchor {
            parent_id: target_rec.parent_id,
            base_depth: target_rec.depth,
        };
        let outcome = self.run_scan(&target_path, anchor, options, progress_callback)?;

        let rtxn2 = self.db.env.read_txn()?;
        let updated_target = self
            .db
            .folders
            .get(&rtxn2, &subtree_root_id)?
            .ok_or(LibraryError::FolderNotFound(subtree_root_id))?;
        drop(rtxn2);

        let delta_media = i64::from(updated_target.recursive_media_count) - old_recursive_media;
        let delta_subfolders = i64::from(updated_target.subfolder_count) - old_subfolders;

        if delta_media != 0 && updated_target.parent_id.is_some() {
            let mut wtxn = self.db.env.write_txn()?;
            let mut current_pid = updated_target.parent_id;

            while let Some(pid) = current_pid {
                if let Some(mut ancestor) = self.db.folders.get(&wtxn, &pid)? {
                    let adjusted = (i64::from(ancestor.recursive_media_count) + delta_media).max(0) as u32;
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
            scanned_folders = outcome.scanned_folders,
            elapsed_ms = t0.elapsed().as_millis(),
            "[FolderScan: SubtreeDone] Surgical subtree rescan completed successfully"
        );

        self.load_hierarchy_snapshot()
    }

    /// Core scan: filesystem walk followed by one atomic LMDB reconciliation transaction.
    /// Existing folder ids are re-used by path so media records keep resolving to their folders.
    fn run_scan<F>(
        &self,
        root_path: &Path,
        anchor: ScanAnchor,
        options: FolderScanOptions,
        progress_callback: Option<F>,
    ) -> Result<ScanOutcome, LibraryError>
    where
        F: Fn(usize) + Send + Sync + 'static,
    {
        let t0 = Instant::now();
        let canonical_root = normalize_path(root_path);
        tracing::info!(
            root_path = %LogObscurer::path(&canonical_root),
            anchor_parent_id = ?anchor.parent_id,
            anchor_base_depth = anchor.base_depth,
            ignore_nested_mounts = options.ignore_nested_mounts,
            "[FolderScan: FullRoot] Starting manual filesystem hierarchy scan..."
        );

        let cb_arc: Option<ScanProgressCallback> = progress_callback.map(|cb| Arc::new(cb) as ScanProgressCallback);

        // The system mount table is read on every scan so changes (new/removed mounts) are always respected.
        let ignored_mounts = FolderMountFilter::resolve_ignored_mounts(&canonical_root, options.ignore_nested_mounts);

        let t_walk = Instant::now();
        let walk = walk_directory_tree(&canonical_root, cb_arc.as_ref(), &ignored_mounts);
        let walk_ms = t_walk.elapsed().as_millis();
        let skipped_mounts = walk.skipped_mounts.len();
        let scanned_items = walk.items;

        tracing::info!(
            scanned_folders = scanned_items.len(),
            skipped_mounts,
            walk_ms = walk_ms as u64,
            "[FolderScan: WalkDone] Filesystem walk completed"
        );

        let t_db = Instant::now();
        let mut wtxn = self.db.env.write_txn()?;

        let existing_all = self.collect_existing_folders(&wtxn, &canonical_root)?;
        let existing_in_scope = existing_all.len();

        // Records at or below ignored mounts are detached/removed here and never marked as missing.
        let (mount_cleanup, mut existing_folders) =
            FolderMountFilter::cleanup_ignored_records(self.db, &mut wtxn, existing_all, &ignored_mounts)?;

        let mut path_to_id: HashMap<PathBuf, FolderId> = HashMap::with_capacity(scanned_items.len());
        let mut new_records: Vec<FolderRecord> = Vec::with_capacity(scanned_items.len());
        let mut previous_root_parent: Option<FolderId> = None;
        let mut created_folders = 0usize;
        let mut reused_folders = 0usize;
        let mut healed_path_entries = 0usize;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);

        for (idx, item) in scanned_items.iter().enumerate() {
            let p_str = item.path.to_string_lossy().to_string();

            let (folder_id, added_at) = if let Some((id, rec)) = existing_folders.remove(&p_str) {
                reused_folders += 1;
                if idx == 0 {
                    previous_root_parent = rec.parent_id;
                }
                if self.db.folders_by_path.get(&wtxn, &p_str)? != Some(id) {
                    self.db.folders_by_path.put(&mut wtxn, &p_str, &id)?;
                    healed_path_entries += 1;
                }
                (id, rec.added_at)
            } else {
                created_folders += 1;
                let new_id = self.db.next_id(&mut wtxn, SEQ_FOLDER_ID)?;
                self.db.folders_by_path.put(&mut wtxn, &p_str, &new_id)?;
                self.db.media_by_folder.put(&mut wtxn, &new_id, &Vec::new())?;
                (new_id, now_secs)
            };

            path_to_id.insert(item.path.clone(), folder_id);

            new_records.push(Self::build_scanned_record(
                folder_id,
                item,
                p_str,
                anchor.base_depth,
                added_at,
                now_secs,
            ));
        }

        let mut marked_missing = 0usize;
        for (id, mut missing_rec) in existing_folders.into_values() {
            if !missing_rec.is_deleted {
                tracing::info!(
                    folder_id = id,
                    path = %LogObscurer::path(Path::new(&missing_rec.path)),
                    "[FolderScan: Missing] Folder missing from filesystem, marked is_deleted = true"
                );
                missing_rec.is_deleted = true;
                self.db.folders.put(&mut wtxn, &id, &missing_rec)?;
                marked_missing += 1;
            }
        }

        let parent_to_children = Self::link_parents(&scanned_items, &path_to_id, &anchor, &mut new_records);
        Self::aggregate_counts(&mut new_records, &parent_to_children);

        for r in &new_records {
            self.db.folders.put(&mut wtxn, &r.id, r)?;
            let children = parent_to_children.get(&r.id).cloned().unwrap_or_default();
            self.db.folders_by_parent.put(&mut wtxn, &r.id, &children)?;
        }

        if anchor.parent_id.is_none()
            && let (Some(old_pid), Some(root_rec)) = (previous_root_parent, new_records.first())
        {
            self.detach_from_old_parent(&mut wtxn, old_pid, root_rec.id)?;
        }

        wtxn.commit()?;

        tracing::info!(
            root_path = %LogObscurer::path(&canonical_root),
            scanned_folders = new_records.len(),
            existing_in_scope,
            created_folders,
            reused_folders,
            marked_missing,
            healed_path_entries,
            skipped_mounts,
            mount_records_detached = mount_cleanup.detached_with_media,
            mount_records_removed = mount_cleanup.removed_empty,
            walk_ms = walk_ms as u64,
            db_ms = t_db.elapsed().as_millis() as u64,
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "[FolderScan: FullRootDone] Directory scan committed to LMDB"
        );

        Ok(ScanOutcome {
            scanned_folders: new_records.len(),
        })
    }

    /// Collects all existing folder records located at or below `canonical_root`, keyed by their stored path.
    fn collect_existing_folders(
        &self,
        wtxn: &heed::RwTxn,
        canonical_root: &Path,
    ) -> Result<HashMap<String, (FolderId, FolderRecord)>, LibraryError> {
        let mut existing: HashMap<String, (FolderId, FolderRecord)> = HashMap::new();
        for entry in self.db.folders.iter(wtxn)? {
            let (id, rec) = entry?;
            if Path::new(&rec.path).starts_with(canonical_root) {
                existing.insert(rec.path.clone(), (id, rec));
            }
        }
        Ok(existing)
    }

    fn build_scanned_record(
        folder_id: FolderId,
        item: &ScannedDirItem,
        path: String,
        base_depth: u32,
        added_at: i64,
        now_secs: i64,
    ) -> FolderRecord {
        FolderRecord {
            id: folder_id,
            parent_id: None,
            name: item.name.clone(),
            path,
            depth: base_depth + item.depth,
            is_hidden: item.is_hidden,
            is_deleted: false,
            subfolder_count: 0,
            media_count: item.media_files_count,
            recursive_media_count: item.media_files_count,
            added_at,
            last_scanned_at: now_secs,
        }
    }

    /// Resolves `parent_id` for each scanned record. The scan root keeps the anchor parent.
    fn link_parents(
        scanned_items: &[ScannedDirItem],
        path_to_id: &HashMap<PathBuf, FolderId>,
        anchor: &ScanAnchor,
        new_records: &mut [FolderRecord],
    ) -> HashMap<FolderId, Vec<FolderId>> {
        let mut parent_to_children: HashMap<FolderId, Vec<FolderId>> = HashMap::new();

        for (idx, item) in scanned_items.iter().enumerate() {
            let folder_id = new_records[idx].id;
            let parent_id = match &item.parent_path {
                Some(p) => path_to_id.get(p).copied(),
                None => anchor.parent_id,
            };

            new_records[idx].parent_id = parent_id;

            if item.parent_path.is_some()
                && let Some(pid) = parent_id
            {
                parent_to_children.entry(pid).or_default().push(folder_id);
            }
        }

        parent_to_children
    }

    /// Bottom-up aggregation of subfolder counts and recursive media counts (deepest folders first).
    fn aggregate_counts(new_records: &mut [FolderRecord], parent_to_children: &HashMap<FolderId, Vec<FolderId>>) {
        let id_to_record_idx: HashMap<FolderId, usize> =
            new_records.iter().enumerate().map(|(idx, r)| (r.id, idx)).collect();

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
    }

    /// Removes a folder from a previous parent's persisted children list (used when a scanned root becomes the tree top).
    fn detach_from_old_parent(
        &self,
        wtxn: &mut heed::RwTxn,
        old_parent_id: FolderId,
        folder_id: FolderId,
    ) -> Result<(), LibraryError> {
        if let Some(mut siblings) = self.db.folders_by_parent.get(wtxn, &old_parent_id)?
            && siblings.contains(&folder_id)
        {
            siblings.retain(|&x| x != folder_id);
            self.db.folders_by_parent.put(wtxn, &old_parent_id, &siblings)?;
            tracing::info!(
                folder_id,
                old_parent_id,
                "[FolderScan: Detach] Removed scanned root from its previous parent's children index"
            );
        }
        Ok(())
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
            folder_rec.media_count = (i64::from(folder_rec.media_count) + delta).max(0) as u32;
            folder_rec.recursive_media_count = (i64::from(folder_rec.recursive_media_count) + delta).max(0) as u32;
            db.folders.put(wtxn, &folder_id, &folder_rec)?;

            let mut current_pid = folder_rec.parent_id;
            while let Some(pid) = current_pid {
                if let Some(mut ancestor) = db.folders.get(wtxn, &pid)? {
                    ancestor.recursive_media_count = (i64::from(ancestor.recursive_media_count) + delta).max(0) as u32;
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
        let depth = parent_rec.as_ref().map_or(0, |p| p.depth + 1);
        let name = canonical_path
            .file_name()
            .map_or_else(|| path_str.clone(), |n| n.to_string_lossy().to_string());

        let is_hidden = name.starts_with('.') || parent_rec.as_ref().is_some_and(|p| p.is_hidden);

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
                .map_or(0, |d| d.as_secs() as i64),
            last_scanned_at: 0,
        };

        self.db.folders.put(&mut wtxn, &folder_id, &record)?;
        self.db.folders_by_path.put(&mut wtxn, &path_str, &folder_id)?;
        self.db.media_by_folder.put(&mut wtxn, &folder_id, &Vec::new())?;

        if let Some(pid) = parent_id {
            let mut siblings = self.db.folders_by_parent.get(&wtxn, &pid)?.unwrap_or_default();
            if !siblings.contains(&folder_id) {
                siblings.push(folder_id);
                self.db.folders_by_parent.put(&mut wtxn, &pid, &siblings)?;
            }

            if let Some(mut p_rec) = self.db.folders.get(&wtxn, &pid)? {
                p_rec.subfolder_count = siblings.len() as u32;
                self.db.folders.put(&mut wtxn, &pid, &p_rec)?;
            }
        }

        wtxn.commit()?;

        tracing::debug!(
            folder_id,
            parent_id = ?parent_id,
            path = %LogObscurer::path(&canonical_path),
            "[FolderManager: AddFolder] Registered new folder record on demand (no filesystem scan)"
        );

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
                        self.db
                            .update_media_hash_index(&mut wtxn, media_id, Some(&media_record.file_hash), None)?;
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
