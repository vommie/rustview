use crate::ctx_menu::CtxMenuItem;
use crate::media::{FileHash, MediaFormat, MediaType};
use crate::query::{MatchMode, MediaTypeFilter, ScopeMode};
pub use crate::thumbnails::types::{ThumbnailLocation, VideoPreviewRecord};
use heed::{BoxedError, BytesDecode, BytesEncode};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub type FolderId = u64;
pub type MediaId = u64;
pub type TagId = u32;

/// Relational persistent folder record with parent-child linkage, visibility flags, and precomputed counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderRecord {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub path: String,
    pub depth: u32,
    pub is_hidden: bool,
    pub is_deleted: bool,
    pub subfolder_count: u32,
    pub media_count: u32,
    pub recursive_media_count: u32,
    pub added_at: i64,
    pub last_scanned_at: i64,
}

#[derive(Deserialize)]
struct LegacyFolderRecordV1 {
    id: FolderId,
    path: String,
    added_at: i64,
}

#[derive(Deserialize)]
struct LegacyFolderRecordV2 {
    id: FolderId,
    path: String,
    added_at: i64,
    is_deleted: bool,
}

/// Zero-copy custom codec providing backward compatibility for databases created under previous schemas.
pub struct FolderRecordCodec;

impl<'a> BytesDecode<'a> for FolderRecordCodec {
    type DItem = FolderRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Ok(record) = bincode::deserialize::<FolderRecord>(bytes) {
            return Ok(record);
        }

        if let Ok(legacy2) = bincode::deserialize::<LegacyFolderRecordV2>(bytes) {
            let path_buf = PathBuf::from(&legacy2.path);
            let name = path_buf
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| legacy2.path.clone());
            let is_hidden = name.starts_with('.');

            return Ok(FolderRecord {
                id: legacy2.id,
                parent_id: None,
                name,
                path: legacy2.path,
                depth: 0,
                is_hidden,
                is_deleted: legacy2.is_deleted,
                subfolder_count: 0,
                media_count: 0,
                recursive_media_count: 0,
                added_at: legacy2.added_at,
                last_scanned_at: 0,
            });
        }

        let legacy1: LegacyFolderRecordV1 = bincode::deserialize(bytes)?;
        let path_buf = PathBuf::from(&legacy1.path);
        let name = path_buf
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| legacy1.path.clone());
        let is_hidden = name.starts_with('.');

        Ok(FolderRecord {
            id: legacy1.id,
            parent_id: None,
            name,
            path: legacy1.path,
            depth: 0,
            is_hidden,
            is_deleted: false,
            subfolder_count: 0,
            media_count: 0,
            recursive_media_count: 0,
            added_at: legacy1.added_at,
            last_scanned_at: 0,
        })
    }
}

impl<'a> BytesEncode<'a> for FolderRecordCodec {
    type EItem = FolderRecord;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        let bytes = bincode::serialize(item)?;
        Ok(Cow::Owned(bytes))
    }
}

/// In-memory node representation holding resolved relational links and precomputed metric counters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedFolderNode {
    pub id: FolderId,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub path: PathBuf,
    pub depth: u32,
    pub is_hidden: bool,
    pub is_deleted: bool,
    pub subfolder_count: u32,
    pub media_count: u32,
    pub recursive_media_count: u32,
    pub children: Vec<FolderId>,
}

/// Parameters passed during recursive tree flattening to Slint model items.
pub struct FolderTreeFlattenFilter<'a> {
    pub expanded_ids: &'a HashSet<FolderId>,
    pub selected_id: Option<FolderId>,
    pub show_deleted: bool,
    pub show_metrics: bool,
    pub show_hidden: bool,
}

/// Central in-memory Single Point of Truth (SPoT) hierarchy snapshot.
#[derive(Debug, Clone, Default)]
pub struct FolderHierarchySnapshot {
    pub nodes: HashMap<FolderId, CachedFolderNode>,
    pub children_map: HashMap<Option<FolderId>, Vec<FolderId>>,
    pub path_index: HashMap<PathBuf, FolderId>,
    pub root_ids: Vec<FolderId>,
}

impl FolderHierarchySnapshot {
    #[inline]
    pub fn get_node(&self, id: FolderId) -> Option<&CachedFolderNode> {
        self.nodes.get(&id)
    }

    #[inline]
    pub fn get_children(&self, parent_id: Option<FolderId>) -> &[FolderId] {
        self.children_map
            .get(&parent_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    #[inline]
    pub fn get_by_path(&self, path: &Path) -> Option<&CachedFolderNode> {
        let canonical = crate::library::folders::normalize_path(path);
        self.path_index
            .get(&canonical)
            .and_then(|id| self.nodes.get(id))
    }

    /// Verifies if `candidate` is equal to or a transitive descendant of `ancestor`.
    pub fn is_descendant_of(&self, candidate: FolderId, ancestor: FolderId) -> bool {
        if candidate == ancestor {
            return true;
        }

        let mut current = self.get_node(candidate).and_then(|n| n.parent_id);
        while let Some(pid) = current {
            if pid == ancestor {
                return true;
            }
            current = self.get_node(pid).and_then(|n| n.parent_id);
        }

        false
    }

    /// Collects all descendant IDs under `root_id` in a single traversal pass.
    pub fn collect_subtree_ids(&self, root_id: FolderId) -> Vec<FolderId> {
        let mut result = vec![root_id];
        let mut queue = vec![root_id];

        while let Some(current) = queue.pop() {
            for &child_id in self.get_children(Some(current)) {
                result.push(child_id);
                queue.push(child_id);
            }
        }

        result
    }

    /// Projects the hierarchical folder tree into Slint's `FolderNodeView` list with zero disk I/O.
    pub fn project_tree(
        &self,
        expanded_ids: &HashSet<FolderId>,
        selected_id: Option<FolderId>,
        show_deleted: bool,
        show_metrics: bool,
        show_hidden: bool,
    ) -> Vec<crate::ui::FolderNodeView> {
        let mut output = Vec::with_capacity(self.nodes.len().min(128));

        let filter = FolderTreeFlattenFilter {
            expanded_ids,
            selected_id,
            show_deleted,
            show_metrics,
            show_hidden,
        };

        fn flatten_recursive(
            snapshot: &FolderHierarchySnapshot,
            parent_id: Option<FolderId>,
            depth: i32,
            filter: &FolderTreeFlattenFilter<'_>,
            output: &mut Vec<crate::ui::FolderNodeView>,
        ) {
            for &child_id in snapshot.get_children(parent_id) {
                let Some(node) = snapshot.get_node(child_id) else { continue };

                if node.is_deleted && !filter.show_deleted {
                    continue;
                }
                if node.is_hidden && !filter.show_hidden {
                    continue;
                }

                let has_children = !node.children.is_empty();
                let is_expanded = filter.expanded_ids.contains(&node.id);
                let is_selected = filter.selected_id == Some(node.id);

                let formatted_name = if filter.show_metrics {
                    format!(
                        "{} ({}, {})",
                        node.name, node.subfolder_count, node.media_count
                    )
                } else {
                    node.name.clone()
                };

                output.push(crate::ui::FolderNodeView {
                    id: node.id as i32,
                    name: formatted_name.into(),
                    depth,
                    path: node.path.to_string_lossy().to_string().into(),
                    has_children,
                    is_expanded,
                    is_selected,
                    is_deleted: node.is_deleted,
                    subfolder_count: node.subfolder_count as i32,
                    file_count: node.media_count as i32,
                });

                if has_children && is_expanded {
                    flatten_recursive(
                        snapshot,
                        Some(node.id),
                        depth + 1,
                        filter,
                        output,
                    );
                }
            }
        }

        flatten_recursive(self, None, 0, &filter, &mut output);
        output
    }

    /// Single Point of Truth (SPoT): Universal context menu generator for "Move to..." and "Copy to..." folder selection trees.
    /// Operates purely in-memory with zero disk I/O, respecting hidden-folder settings and excluding invalid drop targets.
    pub fn project_target_folder_menu(
        &self,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
    ) -> Vec<CtxMenuItem> {
        let mut menu_items = Vec::new();

        for &root_id in &self.root_ids {
            if let Some(item) = self.build_menu_item_recursive(
                root_id,
                base_action_id,
                exclude_subtree_id,
                current_parent_id,
                show_hidden,
                true,
            ) {
                menu_items.push(item);
            }
        }

        menu_items
    }

    fn build_menu_item_recursive(
        &self,
        node_id: FolderId,
        base_action_id: u64,
        exclude_subtree_id: Option<FolderId>,
        current_parent_id: Option<FolderId>,
        show_hidden: bool,
        is_root: bool,
    ) -> Option<CtxMenuItem> {
        let node = self.get_node(node_id)?;

        if node.is_deleted {
            return None;
        }
        if node.is_hidden && !show_hidden {
            return None;
        }

        // Prevent moving a folder into itself or its own descendants
        if let Some(exclude_id) = exclude_subtree_id
            && (node.id == exclude_id || self.is_descendant_of(node.id, exclude_id))
        {
            return None;
        }

        // Do not allow moving or copying to the current folder where files already reside
        let is_current_location = current_parent_id == Some(node.id);

        let mut valid_child_items = Vec::new();
        for &child_id in self.get_children(Some(node.id)) {
            if let Some(child_menu_item) = self.build_menu_item_recursive(
                child_id,
                base_action_id,
                exclude_subtree_id,
                current_parent_id,
                show_hidden,
                false,
            ) {
                valid_child_items.push(child_menu_item);
            }
        }

        let display_title = if is_root {
            format!("📁 Root ({})", node.name)
        } else {
            format!("📁 {}", node.name)
        };

        if valid_child_items.is_empty() {
            Some(CtxMenuItem {
                id: base_action_id + node.id,
                text: display_title,
                shortcut: None,
                icon_name: Some("folder".to_string()),
                icon_path: None,
                kind: crate::ctx_menu::CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_current_location,
                children: Vec::new(),
                menu_index: 0,
                tooltip: if is_current_location {
                    Some("Current location".to_string())
                } else {
                    Some(node.path.to_string_lossy().to_string())
                },
            })
        } else {
            let mut sub_elements = Vec::with_capacity(valid_child_items.len() + 2);
            sub_elements.push(CtxMenuItem {
                id: base_action_id + node.id,
                text: format!("Drop directly into '{}'", node.name),
                shortcut: None,
                icon_name: Some("folder".to_string()),
                icon_path: None,
                kind: crate::ctx_menu::CtxMenuItemKind::Action,
                is_checked: false,
                is_enabled: !is_current_location,
                children: Vec::new(),
                menu_index: 0,
                tooltip: if is_current_location {
                    Some("Current location".to_string())
                } else {
                    Some(node.path.to_string_lossy().to_string())
                },
            });
            sub_elements.push(CtxMenuItem::new_separator());
            sub_elements.extend(valid_child_items);

            Some(CtxMenuItem {
                id: 0,
                text: display_title,
                shortcut: None,
                icon_name: Some("folder".to_string()),
                icon_path: None,
                kind: crate::ctx_menu::CtxMenuItemKind::Submenu,
                is_checked: false,
                is_enabled: true,
                children: sub_elements,
                menu_index: 0,
                tooltip: Some(node.path.to_string_lossy().to_string()),
            })
        }
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
    #[serde(default)]
    pub source: Option<String>,
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

#[derive(Deserialize)]
struct LegacyMediaRecordV4 {
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
    is_corrupted: bool,
}

/// Custom codec providing backward compatibility across all media record schema revisions.
pub struct MediaRecordCodec;

impl<'a> BytesDecode<'a> for MediaRecordCodec {
    type DItem = MediaRecord;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if let Ok(record) = bincode::deserialize::<MediaRecord>(bytes) {
            return Ok(record);
        }
        if let Ok(legacy4) = bincode::deserialize::<LegacyMediaRecordV4>(bytes) {
            return Ok(MediaRecord {
                id: legacy4.id,
                folder_id: legacy4.folder_id,
                filename: legacy4.filename,
                file_hash: legacy4.file_hash,
                file_size: legacy4.file_size,
                modification_time: legacy4.modification_time,
                media_type: legacy4.media_type,
                format: legacy4.format,
                width: legacy4.width,
                height: legacy4.height,
                rating: legacy4.rating,
                last_seen: legacy4.last_seen,
                is_deleted: legacy4.is_deleted,
                creation_time: legacy4.creation_time,
                duration_ms: legacy4.duration_ms,
                is_corrupted: legacy4.is_corrupted,
                source: None,
            });
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
                source: None,
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
                source: None,
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
            source: None,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalHashRecord {
    pub hash_id: u64,
    pub hash: String,
    pub date_time: String,
}

#[derive(Debug, Clone, Default)]
pub struct MediaSearchParams {
    pub rating: Option<u8>,
    pub tag_ids: HashSet<TagId>,
    pub match_mode: MatchMode,
    pub scope_mode: ScopeMode,
    pub folder_id: Option<FolderId>,
    pub media_type_filter: MediaTypeFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileTransferKind {
    Copy,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileConflictResolution {
    Skip,
    AutoRename,
    Overwrite,
    Cancel,
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileConflictReport {
    pub source: FileConflictItemDetails,
    pub destination: FileConflictItemDetails,
}

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
