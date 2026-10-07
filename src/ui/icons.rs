use std::path::Path;

/// Strongly typed identifier for all vector icon assets used in RustView.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconAsset {
    Settings,
    Refresh,
    Loader,
    FolderTreeNodesFold,
    HiddenFoldersInvisible,
    HiddenFoldersVisible,
    GridView,
    ListView,
    Plus,
    Minus,
    CaretUp,
    CaretDown,
    ChevronDown,
    ChevronRight,
    Regex,
    QueryFolder,
    QueryFolderAndSubfolders,
    QueryGlobal,
    Tag,
    Rating(u8),
    MediaFileCorrupted,
    MediaFileMissing,
    MediaFilterVideos,
    MediaFilterImages,
    MediaFilterVideosImages,
    VirtualFolderEnable,
    VirtualFolderDisable,
    Image,
    Video,
}

/// Metadata describing visual capabilities, path locations, and theme-tinting permissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconMetadata {
    pub asset: IconAsset,
    pub name: &'static str,
    pub file_name: &'static str,
    pub relative_path: &'static str,
    pub allow_tint: bool,
    pub description: &'static str,
}

impl IconAsset {
    /// Returns the descriptive metadata for the icon asset.
    pub fn metadata(&self) -> IconMetadata {
        match self {
            Self::Settings => IconMetadata {
                asset: *self,
                name: "settings",
                file_name: "settings.svg",
                relative_path: "ui/icons/settings.svg",
                allow_tint: true,
                description: "Application configuration and settings gear icon",
            },
            Self::Refresh => IconMetadata {
                asset: *self,
                name: "refresh",
                file_name: "refresh.svg",
                relative_path: "ui/icons/refresh.svg",
                allow_tint: true,
                description: "Synchronize and refresh file or directory contents",
            },
            Self::Loader => IconMetadata {
                asset: *self,
                name: "loader",
                file_name: "loader.svg",
                relative_path: "ui/icons/loader.svg",
                allow_tint: true,
                description: "Animated background progress throbber indicator",
            },
            Self::FolderTreeNodesFold => IconMetadata {
                asset: *self,
                name: "folder_tree_nodes_fold",
                file_name: "folder_tree_nodes_fold.svg",
                relative_path: "ui/icons/folder_tree_nodes_fold.svg",
                allow_tint: true,
                description: "Collapse all folder tree nodes except active path",
            },
            Self::HiddenFoldersInvisible => IconMetadata {
                asset: *self,
                name: "hidden-folders-invisible",
                file_name: "hidden-folders-invisible.svg",
                relative_path: "ui/icons/hidden-folders-invisible.svg",
                allow_tint: true,
                description: "Hidden folders are currently hidden/invisible",
            },
            Self::HiddenFoldersVisible => IconMetadata {
                asset: *self,
                name: "hidden-folders-visible",
                file_name: "hidden-folders-visible.svg",
                relative_path: "ui/icons/hidden-folders-visible.svg",
                allow_tint: true,
                description: "Hidden folders are currently shown/visible",
            },
            Self::GridView => IconMetadata {
                asset: *self,
                name: "grid_view",
                file_name: "grid_view.svg",
                relative_path: "ui/icons/grid_view.svg",
                allow_tint: true,
                description: "Switch to thumbnail grid layout mode",
            },
            Self::ListView => IconMetadata {
                asset: *self,
                name: "list_view",
                file_name: "list_view.svg",
                relative_path: "ui/icons/list_view.svg",
                allow_tint: true,
                description: "Switch to detailed media table layout mode",
            },
            Self::Plus => IconMetadata {
                asset: *self,
                name: "plus",
                file_name: "plus.svg",
                relative_path: "ui/icons/plus.svg",
                allow_tint: true,
                description: "Add new node, tag, or directory item",
            },
            Self::Minus => IconMetadata {
                asset: *self,
                name: "minus",
                file_name: "minus.svg",
                relative_path: "ui/icons/minus.svg",
                allow_tint: true,
                description: "Collapse or remove item",
            },
            Self::CaretUp => IconMetadata {
                asset: *self,
                name: "caret-up",
                file_name: "caret-up.svg",
                relative_path: "ui/icons/caret-up.svg",
                allow_tint: true,
                description: "Ascending sort direction caret",
            },
            Self::CaretDown => IconMetadata {
                asset: *self,
                name: "caret-down",
                file_name: "caret-down.svg",
                relative_path: "ui/icons/caret-down.svg",
                allow_tint: true,
                description: "Descending sort direction caret",
            },
            Self::ChevronDown => IconMetadata {
                asset: *self,
                name: "arrow-down-s-line",
                file_name: "arrow-down-s-line.svg",
                relative_path: "ui/icons/arrow-down-s-line.svg",
                allow_tint: true,
                description: "Collapsible panel expanded state chevron indicator pointing down",
            },
            Self::ChevronRight => IconMetadata {
                asset: *self,
                name: "arrow-right-s-line",
                file_name: "arrow-right-s-line.svg",
                relative_path: "ui/icons/arrow-right-s-line.svg",
                allow_tint: true,
                description: "Collapsible panel collapsed state chevron indicator pointing right",
            },
            Self::Regex => IconMetadata {
                asset: *self,
                name: "regex",
                file_name: "regex.svg",
                relative_path: "ui/icons/regex.svg",
                allow_tint: true,
                description: "Toggle regular expression query matching mode",
            },
            Self::QueryFolder => IconMetadata {
                asset: *self,
                name: "query_mode_folder",
                file_name: "query_mode_folder.svg",
                relative_path: "ui/icons/query_mode_folder.svg",
                allow_tint: true,
                description: "Query search scope limited to the active folder only",
            },
            Self::QueryFolderAndSubfolders => IconMetadata {
                asset: *self,
                name: "query_mode_folder_and_subfolders",
                file_name: "query_mode_folder_and_subfolders.svg",
                relative_path: "ui/icons/query_mode_folder_and_subfolders.svg",
                allow_tint: true,
                description: "Query search scope recursive across subfolders",
            },
            Self::QueryGlobal => IconMetadata {
                asset: *self,
                name: "query_mode_gobal",
                file_name: "query_mode_gobal.svg",
                relative_path: "ui/icons/query_mode_gobal.svg",
                allow_tint: true,
                description: "Global query search scope across all registered folders",
            },
            Self::Tag => IconMetadata {
                asset: *self,
                name: "tag",
                file_name: "tag.svg",
                relative_path: "ui/icons/tag.svg",
                allow_tint: false,
                description: "Multi-color orange tag marker indicator (never tint)",
            },
            Self::Rating(0) => IconMetadata {
                asset: *self,
                name: "rating_0",
                file_name: "rating_0.svg",
                relative_path: "ui/icons/rating_0.svg",
                allow_tint: false,
                description: "Neutral unrated circle badge (never tint)",
            },
            Self::Rating(1) => IconMetadata {
                asset: *self,
                name: "rating_1",
                file_name: "rating_1.svg",
                relative_path: "ui/icons/rating_1.svg",
                allow_tint: false,
                description: "1-Star rating colored badge (never tint)",
            },
            Self::Rating(2) => IconMetadata {
                asset: *self,
                name: "rating_2",
                file_name: "rating_2.svg",
                relative_path: "ui/icons/rating_2.svg",
                allow_tint: false,
                description: "2-Star rating colored badge (never tint)",
            },
            Self::Rating(3) => IconMetadata {
                asset: *self,
                name: "rating_3",
                file_name: "rating_3.svg",
                relative_path: "ui/icons/rating_3.svg",
                allow_tint: false,
                description: "3-Star rating colored badge (never tint)",
            },
            Self::Rating(4) => IconMetadata {
                asset: *self,
                name: "rating_4",
                file_name: "rating_4.svg",
                relative_path: "ui/icons/rating_4.svg",
                allow_tint: false,
                description: "4-Star rating colored badge (never tint)",
            },
            Self::Rating(5) => IconMetadata {
                asset: *self,
                name: "rating_5",
                file_name: "rating_5.svg",
                relative_path: "ui/icons/rating_5.svg",
                allow_tint: false,
                description: "5-Star rating colored badge (never tint)",
            },
            Self::Rating(_) => IconMetadata {
                asset: *self,
                name: "rating_0",
                file_name: "rating_0.svg",
                relative_path: "ui/icons/rating_0.svg",
                allow_tint: false,
                description: "Fallback rating badge (never tint)",
            },
            Self::MediaFileCorrupted => IconMetadata {
                asset: *self,
                name: "media_file_corrupted",
                file_name: "media_file_corrupted.svg",
                relative_path: "ui/icons/media_file_corrupted.svg",
                allow_tint: false,
                description: "Corrupted or unreadable media file warning icon",
            },
            Self::MediaFileMissing => IconMetadata {
                asset: *self,
                name: "media_file_missing",
                file_name: "media_file_missing.svg",
                relative_path: "ui/icons/media_file_missing.svg",
                allow_tint: false,
                description: "Missing or deleted media file placeholder icon",
            },
            Self::MediaFilterVideos => IconMetadata {
                asset: *self,
                name: "media_filter_videos",
                file_name: "media_filter_videos.svg",
                relative_path: "ui/icons/media_filter_videos.svg",
                allow_tint: true,
                description: "Filter display to show only video media items",
            },
            Self::MediaFilterImages => IconMetadata {
                asset: *self,
                name: "media_filter_images",
                file_name: "media_filter_images.svg",
                relative_path: "ui/icons/media_filter_images.svg",
                allow_tint: true,
                description: "Filter display to show only image media items",
            },
            Self::MediaFilterVideosImages => IconMetadata {
                asset: *self,
                name: "media_filter_videos_images",
                file_name: "media_filter_videos_images.svg",
                relative_path: "ui/icons/media_filter_videos_images.svg",
                allow_tint: true,
                description: "Filter display to show both video and image media items",
            },
            Self::VirtualFolderEnable => IconMetadata {
                asset: *self,
                name: "virtual_folder_enable",
                file_name: "virtual_folder_enable.svg",
                relative_path: "ui/icons/virtual_folder_enable.svg",
                allow_tint: true,
                description: "Enable recursive virtual folder view (include all subfolders)",
            },
            Self::VirtualFolderDisable => IconMetadata {
                asset: *self,
                name: "virtual_folder_disable",
                file_name: "virtual_folder_disable.svg",
                relative_path: "ui/icons/virtual_folder_disable.svg",
                allow_tint: true,
                description: "Disable recursive virtual folder view (show current folder only)",
            },
            Self::Image => IconMetadata {
                asset: *self,
                name: "image",
                file_name: "image.svg",
                relative_path: "ui/icons/image.svg",
                allow_tint: true,
                description: "Generic media image file icon",
            },
            Self::Video => IconMetadata {
                asset: *self,
                name: "video",
                file_name: "video.svg",
                relative_path: "ui/icons/video.svg",
                allow_tint: true,
                description: "Generic media video file icon",
            },
        }
    }

    /// Single Point of Truth helper querying whether an icon should be colorized for dark/light themes.
    #[inline]
    pub fn is_tintable(&self) -> bool {
        self.metadata().allow_tint
    }

    /// Returns the relative filesystem path to the icon SVG asset.
    #[inline]
    pub fn relative_path(&self) -> &'static str {
        self.metadata().relative_path
    }

    /// Resolves an `IconAsset` by generic identifier string.
    pub fn from_name(name: &str) -> Option<Self> {
        let trimmed = name.trim().to_lowercase();
        match trimmed.as_str() {
            "settings" => Some(Self::Settings),
            "refresh" | "reload" => Some(Self::Refresh),
            "loader" | "throbber" | "spinner" => Some(Self::Loader),
            "folder_tree_nodes_fold"
            | "folder_tree_fold"
            | "fold_nodes"
            | "collapse_all_folders" => Some(Self::FolderTreeNodesFold),
            "hidden-folders-invisible" | "hidden_folders_invisible" | "hidden_invisible" => {
                Some(Self::HiddenFoldersInvisible)
            }
            "hidden-folders-visible" | "hidden_folders_visible" | "hidden_visible" => {
                Some(Self::HiddenFoldersVisible)
            }
            "grid" | "grid_view" => Some(Self::GridView),
            "list" | "list_view" | "table" => Some(Self::ListView),
            "plus" | "add" => Some(Self::Plus),
            "minus" | "collapse" => Some(Self::Minus),
            "caret-up" | "sort_asc" => Some(Self::CaretUp),
            "caret-down" | "sort_desc" => Some(Self::CaretDown),
            "arrow-down-s-line" | "chevron_down" | "collapse_panel" => Some(Self::ChevronDown),
            "arrow-right-s-line" | "chevron_right" | "expand_panel" => Some(Self::ChevronRight),
            "regex" => Some(Self::Regex),
            "folder" | "query_folder" => Some(Self::QueryFolder),
            "folders" | "query_subfolders" | "subfolders" => Some(Self::QueryFolderAndSubfolders),
            "world" | "global" | "query_global" | "gobal" => Some(Self::QueryGlobal),
            "tag" => Some(Self::Tag),
            "rating_0" | "rate_0" => Some(Self::Rating(0)),
            "rating_1" | "rate_1" => Some(Self::Rating(1)),
            "rating_2" | "rate_2" => Some(Self::Rating(2)),
            "rating_3" | "rate_3" => Some(Self::Rating(3)),
            "rating_4" | "rate_4" => Some(Self::Rating(4)),
            "rating_5" | "rate_5" => Some(Self::Rating(5)),
            "media_file_corrupted" | "corrupt" | "corrupted" => Some(Self::MediaFileCorrupted),
            "media_file_missing" | "missing" | "deleted" => Some(Self::MediaFileMissing),
            "media_filter_videos" | "filter_videos" | "videos" => Some(Self::MediaFilterVideos),
            "media_filter_images" | "filter_images" | "images" => Some(Self::MediaFilterImages),
            "media_filter_videos_images" | "filter_videos_images" | "videos_images" | "both" => {
                Some(Self::MediaFilterVideosImages)
            }
            "virtual_folder_enable" | "virtual_folder" | "virtual_enable" | "recursive_enable" => {
                Some(Self::VirtualFolderEnable)
            }
            "virtual_folder_disable" | "virtual_disable" | "recursive_disable" => {
                Some(Self::VirtualFolderDisable)
            }
            "image" | "picture" | "photo" => Some(Self::Image),
            "video" | "movie" | "film" | "clip" => Some(Self::Video),
            _ => None,
        }
    }

    /// Returns a static slice of all registered application icons.
    pub fn all() -> &'static [Self] {
        &[
            Self::Settings,
            Self::Refresh,
            Self::Loader,
            Self::FolderTreeNodesFold,
            Self::HiddenFoldersInvisible,
            Self::HiddenFoldersVisible,
            Self::GridView,
            Self::ListView,
            Self::Plus,
            Self::Minus,
            Self::CaretUp,
            Self::CaretDown,
            Self::ChevronDown,
            Self::ChevronRight,
            Self::Regex,
            Self::QueryFolder,
            Self::QueryFolderAndSubfolders,
            Self::QueryGlobal,
            Self::Tag,
            Self::Rating(0),
            Self::Rating(1),
            Self::Rating(2),
            Self::Rating(3),
            Self::Rating(4),
            Self::Rating(5),
            Self::MediaFileCorrupted,
            Self::MediaFileMissing,
            Self::MediaFilterVideos,
            Self::MediaFilterImages,
            Self::MediaFilterVideosImages,
            Self::VirtualFolderEnable,
            Self::VirtualFolderDisable,
            Self::Image,
            Self::Video,
        ]
    }
}

/// Centralized thread-safe Icon Subsystem Manager.
pub struct IconManager;

impl IconManager {
    /// Resolves icon metadata by asset identifier string.
    pub fn get_metadata(name: &str) -> Option<IconMetadata> {
        let asset = IconAsset::from_name(name)?;
        let meta = asset.metadata();
        tracing::trace!(
            icon = meta.name,
            allow_tint = meta.allow_tint,
            path = meta.relative_path,
            "[IconManager] Resolved icon asset metadata"
        );
        Some(meta)
    }

    /// Single Point of Truth helper querying whether an icon should be colorized for dark/light themes.
    pub fn is_tintable(name: &str) -> bool {
        IconAsset::from_name(name)
            .map(|a| a.is_tintable())
            .unwrap_or(false)
    }

    /// Validates that all declared vector assets exist on physical disk.
    pub fn verify_assets(workspace_root: &Path) -> Result<(), Vec<String>> {
        let mut missing = Vec::new();
        for asset in IconAsset::all() {
            let meta = asset.metadata();
            let full_path = workspace_root.join(meta.relative_path);
            if !full_path.exists() {
                missing.push(format!(
                    "Missing icon asset '{}' at {:?}",
                    meta.name, full_path
                ));
            }
        }

        if missing.is_empty() {
            tracing::info!(
                total_icons = IconAsset::all().len(),
                "[IconManager] All vector icon assets verified on disk."
            );
            Ok(())
        } else {
            tracing::error!(
                missing_count = missing.len(),
                "[IconManager] Icon asset verification failed with missing files."
            );
            Err(missing)
        }
    }
}
