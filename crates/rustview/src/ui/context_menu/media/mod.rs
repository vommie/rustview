use crate::ui::folder_tree::projection::FolderProjection;
use crate::ui::file_dialogs::conflict::FileOpsConflictDialogController;
use crate::ui::file_dialogs::delete::DeleteFileDialogController;
use crate::ui::file_dialogs::missing::MissingFileDialogController;
use crate::ui::file_dialogs::remove_orphaned::RemoveOrphanedFileDialogController;
use crate::ui::selection::BridgeFileSelection;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::media::open_with::MediaOpenWith;
use crate::ui::state::*;
use crate::ui::browse_folders::BrowseFolders;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::selection::model::FileSelectionModel;
use crate::ui::multiview::{OpenSummary, eligibility as multiview_eligibility};
use crate::ui::thumbnail_regeneration::eligibility::RegenerationEligibility;
use crate::ui::thumbnail_regeneration::ThumbnailRegeneration;
use crate::ui::{FileRenameDialog, MainWindow};
use crate::ctx_menu::{
    CtxMenu, CtxMenuActionId, CtxMenuBuilder, CtxMenuDispatcher,
};
use crate::icons::Icon;
use rustview_core::library::models::{FolderHierarchySnapshot, FolderId, MediaId};
use rustview_core::library::Library;
use rustview_core::media::MediaType;
use rustview_core::settings::SettingsManager;
use crate::system::DesktopApp;
use rustview_core::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

mod open_actions;
mod copy_actions;

pub struct MediaActionContext<'a> {
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub dispatcher: &'a Arc<CtxMenuDispatcher>,
    pub rename_dialog: &'a Rc<RefCell<Option<FileRenameDialog>>>,
    pub missing_file_dialog: &'a MissingFileDialogController,
    pub delete_file_dialog: &'a DeleteFileDialogController,
    pub remove_orphaned_dialog: &'a RemoveOrphanedFileDialogController,
    pub conflict_dialog: &'a FileOpsConflictDialogController,
    pub fullscreen: &'a MultiviewHandle,
}

#[derive(Debug, Clone)]
pub struct MediaSelectionMeta {
    pub all_selected_deleted: bool,
    pub selected_count: usize,
    pub is_single_image: bool,
    /// Valid-video eligibility for "Thumbnail (Random Pos.)" (in-memory evaluation, no drive access).
    pub regeneration: RegenerationEligibility,
    /// What the Multiview would show for the current selection (in-memory evaluation, no drive access).
    pub multiview: OpenSummary,
    pub target_file_exists: bool,
    pub is_single_pending: bool,
    pub single_filename: String,
    pub current_folder_id: Option<FolderId>,
    pub associated_apps: Vec<DesktopApp>,
}

pub struct MediaContextMenu;

impl MediaContextMenu {
    pub fn prepare_selection(
        window: &MainWindow,
        library: &Library,
        state: &Arc<RwLock<UiState>>,
        media_id: MediaId,
    ) -> MediaSelectionMeta {
        let (selected_count, changed_ids, folders_cleared) = {
            let mut st = state.write();
            let mut changed = Vec::new();
            let mut folders_cleared = false;
            if !st.file_selection.is_selected(media_id) {
                let prev = st.file_selection.selected_ids().clone();
                folders_cleared = st.folder_rows.has_selection();
                st.select_single_media(media_id);
                let newly = st.file_selection.selected_ids().clone();
                changed = FileSelectionModel::compute_selection_diff(&prev, &newly);
            }
            (st.file_selection.count(), changed, folders_cleared)
        };

        if !changed_ids.is_empty() {
            BridgeFileSelection::sync_selection_diff(window, state, &changed_ids);
        }
        if folders_cleared {
            BrowseFolders::refresh_folder_tiles(window, state);
        }
        BridgeInspector::update(window, state);
        StatusBarBridge::update_metrics(window, state);

        let is_single = selected_count == 1;
        let single_id = state
            .read()
            .file_selection
            .selected_ids()
            .iter()
            .next()
            .copied()
            .unwrap_or(media_id);

        let (is_single_image, all_selected_deleted, target_path_opt, is_single_pending, single_filename, cur_folder_id) = {
            let st = state.read();
            let selected_ids = st.file_selection.selected_ids();

            let all_deleted = !selected_ids.is_empty()
                && selected_ids.iter().all(|&id| {
                    st.all_folder_media
                        .iter()
                        .find(|m| m.record.id == id)
                        .map(|m| m.record.is_deleted)
                        .unwrap_or(false)
                });

            let (is_single_img, is_pending, fname, f_id) = if let Some(item) = st.all_folder_media.find(single_id) {
                (
                    matches!(item.record.media_type, MediaType::Image),
                    item.is_pending,
                    item.record.filename.clone(),
                    Some(item.record.folder_id),
                )
            } else if let Ok(Some(m)) = library.media().get(single_id) {
                let pending = m.source.is_some() && (m.file_size == 0 || m.width == 0);
                (
                    matches!(m.media_type, MediaType::Image),
                    pending,
                    m.filename.clone(),
                    Some(m.folder_id),
                )
            } else {
                (false, false, String::new(), st.current_folder_id)
            };

            let path = if let Some(item) = st.all_folder_media.find(single_id) {
                Some(item.full_path())
            } else {
                library.resolve_media_path(single_id).ok()
            };

            (is_single_img, all_deleted, path, is_single && is_pending, fname, f_id)
        };

        let target_file_exists = if is_single {
            target_path_opt
                .as_ref()
                .map(|p| p.is_file() && p.metadata().map(|m| m.len() > 0).unwrap_or(false))
                .unwrap_or(false)
        } else {
            !all_selected_deleted
        };

        let associated_apps = match target_path_opt {
            Some(ref path) if target_file_exists => MediaOpenWith::discover_apps(state, path),
            _ => {
                state.write().active_associated_apps = Vec::new();
                Vec::new()
            }
        };

        tracing::debug!(
            selected_count,
            single_media_id = single_id,
            target_file_exists,
            apps_count = associated_apps.len(),
            "[MediaContextMenu: Prepare] Media context menu selection metadata calculated"
        );

        MediaSelectionMeta {
            all_selected_deleted,
            selected_count,
            is_single_image: is_single && is_single_image,
            regeneration: ThumbnailRegeneration::eligibility(state),
            multiview: multiview_eligibility::summarize(&state.read()),
            target_file_exists,
            is_single_pending,
            single_filename,
            current_folder_id: cur_folder_id,
            associated_apps,
        }
    }

    pub fn append_file_open_entries(
        mut builder: CtxMenuBuilder,
        is_single: bool,
        can_open: bool,
        associated_apps: &[DesktopApp],
    ) -> CtxMenuBuilder {
        builder = builder.action(
            CTX_ACTION_OPEN,
            if is_single {
                "Open".to_string()
            } else {
                "Open Selected Files".to_string()
            },
            None,
            Some(Icon::Open),
            can_open,
        );

        if can_open && !associated_apps.is_empty() {
            let apps = associated_apps.to_vec();
            builder = builder.submenu("Open With...", Some(Icon::OpenWith), true, move |sub| {
                MediaOpenWith::append_menu_items(sub, &apps)
            });
        } else {
            builder = builder.action(
                CTX_ACTION_OPEN_WITH_OTHER,
                "Open With...",
                None,
                Some(Icon::OpenWith),
                can_open,
            );
        }

        builder = builder.action(
            CTX_ACTION_OPEN_IN_FILE_MANAGER,
            if is_single {
                "Open in File Manager".to_string()
            } else {
                "Open in File Manager (Multiple Files)".to_string()
            },
            None,
            Some(Icon::FolderOpen),
            can_open,
        );

        builder
    }

    pub fn append_file_management_entries(
        mut builder: CtxMenuBuilder,
        is_single: bool,
        is_single_image: bool,
        all_selected_deleted: bool,
        target_file_exists: bool,
        is_busy: bool,
    ) -> CtxMenuBuilder {
        let can_rename = is_single && !is_busy;

        let copy_name_text = if is_single {
            "File Name".to_string()
        } else {
            "File Names".to_string()
        };

        let copy_path_text = if is_single {
            "File Path".to_string()
        } else {
            "File Paths".to_string()
        };

        let delete_action_text = if all_selected_deleted {
            if is_single {
                "Remove from Database...".to_string()
            } else {
                "Remove Files from Database...".to_string()
            }
        } else if is_single {
            "Delete File...".to_string()
        } else {
            "Delete Files...".to_string()
        };

        let (delete_action_id, delete_icon) = if all_selected_deleted {
            (CTX_ACTION_REMOVE_FROM_DB, Some(Icon::DatabaseClean))
        } else {
            (CTX_ACTION_DELETE_MEDIA, Some(Icon::Delete))
        };

        builder = builder
            .action(
                CTX_ACTION_RENAME,
                if is_single {
                    "Rename...".to_string()
                } else {
                    "Rename (Single File Only)".to_string()
                },
                Some("F2"),
                Some(Icon::Rename),
                can_rename,
            )
            .submenu("Copy...", Some(Icon::Copy), true, move |sub| {
                let mut sub_builder = sub
                    .action(CTX_ACTION_COPY_NAME, copy_name_text, None, Some(Icon::Copy), true)
                    .action(CTX_ACTION_COPY_PATH, copy_path_text, None, Some(Icon::Copy), true);

                if is_single_image {
                    sub_builder = sub_builder.action(
                        CTX_ACTION_COPY_IMAGE,
                        "Image Content",
                        None,
                        Some(Icon::Copy),
                        target_file_exists,
                    );
                }
                sub_builder
            })
            .action(
                delete_action_id,
                delete_action_text,
                Some("Del"),
                delete_icon,
                !is_busy,
            );

        builder
    }

    /// Builds the complete media context menu using SPoT in-memory hierarchy projections for "Copy to..." and "Move to..." submenus.
    pub fn build_menu(
        meta: &MediaSelectionMeta,
        is_busy: bool,
        snapshot: Option<&FolderHierarchySnapshot>,
        show_hidden: bool,
    ) -> CtxMenu {
        let is_single = meta.selected_count == 1;
        let can_open = meta.selected_count > 0 && meta.target_file_exists;

        let open_in_tab_text = if is_single {
            "Open in Tab".to_string()
        } else {
            format!("Open in Tabs ({} Files)", meta.selected_count)
        };
        let open_in_tab_shortcut = if is_single { Some("DoubleClick") } else { None };

        let open_in_bg_tab_text = if is_single {
            "Open in Background-Tab".to_string()
        } else {
            format!("Open in Background-Tabs ({} Files)", meta.selected_count)
        };
        let open_in_bg_tab_shortcut = if is_single { Some("Ctrl+MiddleClick") } else { None };

        let open_in_fullscreen_shortcut = if is_single { Some("MiddleClick") } else { None };

        let refresh_full_text = if is_single {
            "File (Full)".to_string()
        } else {
            format!("Files (Full) ({} Files)", meta.selected_count)
        };

        let refresh_meta_text = if is_single {
            "File (Metadata)".to_string()
        } else {
            format!("Files (Metadata) ({} Files)", meta.selected_count)
        };

        let regen_thumb_text = ThumbnailRegeneration::menu_label(&meta.regeneration);
        // Enabled only if the selection contains a valid video (no drive access) and nothing else is running.
        let can_regenerate_thumbs = meta.regeneration.is_actionable() && !is_busy;
        let regen_thumb_tooltip = if is_busy && meta.regeneration.is_actionable() {
            format!(
                "{}\n{}",
                ThumbnailRegeneration::hint(&meta.regeneration),
                ThumbnailRegeneration::busy_hint()
            )
        } else {
            ThumbnailRegeneration::hint(&meta.regeneration)
        };

        let can_refresh = meta.selected_count > 0 && !is_busy;

        let mut builder = CtxMenuBuilder::new();
        if is_single {
            let prefix = if meta.is_single_pending {
                "Pending File: "
            } else if meta.all_selected_deleted {
                "Missing File: "
            } else {
                "File: "
            };
            let (header_title, header_tooltip) =
                CtxMenuBuilder::format_header_title(prefix, &meta.single_filename, 24);
            builder = builder.header_with_tooltip(header_title, header_tooltip);
        } else {
            builder = builder.header(format!("Selected Files ({})", meta.selected_count));
        }

        // 1. File Open Actions
        builder = Self::append_file_open_entries(builder, is_single, can_open, &meta.associated_apps);

        // 2. Tab Navigation Actions
        builder = builder
            .action(
                CTX_ACTION_OPEN_IN_TAB,
                open_in_tab_text,
                open_in_tab_shortcut,
                Some(Icon::OpenInTab),
                can_open,
            )
            .action(
                CTX_ACTION_OPEN_IN_BG_TAB,
                open_in_bg_tab_text,
                open_in_bg_tab_shortcut,
                Some(Icon::OpenInBackgroundTab),
                can_open,
            );
        builder = builder
            .action_with_tooltip(
                CTX_ACTION_OPEN_IN_FULLSCREEN,
                meta.multiview.menu_label(),
                open_in_fullscreen_shortcut,
                Some(if is_single {
                    Icon::OpenFullscreen
                } else {
                    Icon::OpenMultiview
                }),
                meta.multiview.is_actionable(),
                meta.multiview.hint(),
            )
            .separator();

        // 3. Move To... / Copy To... Submenus Projected directly from SPoT with zero disk I/O
        if let Some(snap) = snapshot {
            let copy_menu_items = snap.project_target_folder_menu(
                CTX_ACTION_COPY_TO_BASE,
                None,
                meta.current_folder_id,
                show_hidden,
            );
            if !copy_menu_items.is_empty() {
                builder = builder.submenu("Copy to...", Some(Icon::Copy), true, move |sub| {
                    let mut s = sub;
                    for item in copy_menu_items {
                        s = s.item(item);
                    }
                    s
                });
            }

            let move_menu_items = snap.project_target_folder_menu(
                CTX_ACTION_MOVE_TO_BASE,
                None,
                meta.current_folder_id,
                show_hidden,
            );
            if !move_menu_items.is_empty() {
                builder = builder.submenu("Move to...", Some(Icon::MoveTo), !is_busy, move |sub| {
                    let mut s = sub;
                    for item in move_menu_items {
                        s = s.item(item);
                    }
                    s
                });
            }
            builder = builder.separator();
        }

        // 4. Shared File Management Actions
        builder = Self::append_file_management_entries(
            builder,
            is_single,
            meta.is_single_image,
            meta.all_selected_deleted,
            meta.target_file_exists,
            is_busy,
        );

        // 5. Ratings, Refresh & Selection Utilities
        builder = builder
            .separator()
            .submenu(
                "Rate Selection",
                Some(Icon::Rate),
                meta.selected_count > 0,
                |sub| {
                    sub.action(CTX_ACTION_RATE_5, "5 Stars", None, Some(Icon::Rating5), true)
                        .action(CTX_ACTION_RATE_4, "4 Stars", None, Some(Icon::Rating4), true)
                        .action(CTX_ACTION_RATE_3, "3 Stars", None, Some(Icon::Rating3), true)
                        .action(CTX_ACTION_RATE_2, "2 Stars", None, Some(Icon::Rating2), true)
                        .action(CTX_ACTION_RATE_1, "1 Star", None, Some(Icon::Rating1), true)
                        .separator()
                        .action(CTX_ACTION_RATE_0, "Clear Rating", None, Some(Icon::RatingClear), true)
                },
            )
            .separator()
            .submenu(
                "Refresh",
                Some(Icon::Refresh),
                can_refresh,
                |sub| {
                    sub.action(
                        CTX_ACTION_REFRESH_FILES,
                        refresh_full_text,
                        Some("F5"),
                        Some(Icon::Refresh),
                        can_refresh,
                    )
                    .action(
                        CTX_ACTION_REFRESH_METADATA,
                        refresh_meta_text,
                        None,
                        Some(Icon::Refresh),
                        can_refresh,
                    )
                    .action_with_tooltip(
                        CTX_ACTION_REGENERATE_THUMB_RANDOM,
                        regen_thumb_text,
                        None,
                        Some(Icon::RegenerateThumbnail),
                        can_regenerate_thumbs,
                        regen_thumb_tooltip,
                    )
                },
            )
            .separator()
            .action(
                CTX_ACTION_SELECT_ALL,
                "Select All",
                Some("Ctrl+A"),
                Some(Icon::SelectAll),
                true,
            )
            .action(
                CTX_ACTION_CLEAR_SELECTION,
                "Clear Selection",
                Some("Ctrl+Shift+A"),
                Some(Icon::ClearSelection),
                meta.selected_count > 0,
            );

        builder.build()
    }

    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        ctx: &MediaActionContext<'_>,
    ) {
        let selected_ids: Vec<MediaId> = ctx.state.read().get_selected_ids_ordered();
        let fallback_ids = if selected_ids.is_empty() {
            ctx.state.read().file_selection.selected_ids().iter().copied().collect()
        } else {
            selected_ids
        };

        Self::handle_file_action_for_ids(action_id, window, ctx, &fallback_ids);
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx_menu::CtxMenuItem;

    fn meta(selected_count: usize, deleted: bool) -> MediaSelectionMeta {
        MediaSelectionMeta {
            all_selected_deleted: deleted,
            selected_count,
            is_single_image: selected_count == 1,
            regeneration: RegenerationEligibility::default(),
            multiview: OpenSummary {
                selected: selected_count,
                playable: selected_count,
            },
            target_file_exists: true,
            is_single_pending: false,
            single_filename: "image.jpg".to_string(),
            current_folder_id: None,
            associated_apps: vec![DesktopApp {
                id: "viewer.desktop".to_string(),
                name: "Viewer".to_string(),
                icon_name: "viewer".to_string(),
                icon_path: None,
                exec: "viewer %f".to_string(),
            }],
        }
    }

    #[test]
    fn every_media_menu_entry_has_an_icon() {
        for (count, deleted) in [(1, false), (3, false), (1, true)] {
            let menu = MediaContextMenu::build_menu(&meta(count, deleted), false, None, false);
            assert!(
                menu.entries_without_icon().is_empty(),
                "entries without icon (count={count}, deleted={deleted}): {:?}",
                menu.entries_without_icon()
            );
        }
    }

    fn regenerate_entry(menu: &CtxMenu) -> CtxMenuItem {
        let refresh = menu.items.iter().find(|i| i.text == "Refresh").expect("refresh submenu");
        refresh
            .children
            .iter()
            .find(|i| i.id == CTX_ACTION_REGENERATE_THUMB_RANDOM)
            .expect("regenerate entry")
            .clone()
    }

    fn regeneration_menu(selected: usize, videos: usize, is_busy: bool) -> CtxMenuItem {
        let mut m = meta(selected, false);
        m.regeneration = RegenerationEligibility { selected, videos };
        regenerate_entry(&MediaContextMenu::build_menu(&m, is_busy, None, false))
    }

    #[test]
    fn regenerate_entry_is_disabled_without_a_valid_video_and_explains_why() {
        let none_selected = regeneration_menu(0, 0, false);
        let images_only = regeneration_menu(3, 0, false);
        for entry in [&none_selected, &images_only] {
            assert!(!entry.is_enabled);
            assert_eq!(entry.text, "Thumbnail (Random Pos.)");
        }
        assert!(images_only.tooltip.as_deref().unwrap_or("").contains("no available video"));
    }

    #[test]
    fn regenerate_entry_is_enabled_for_mixed_selections_and_counts_only_videos() {
        let single = regeneration_menu(1, 1, false);
        assert!(single.is_enabled);
        assert_eq!(single.text, "Thumbnail (Random Pos.)");

        let mixed = regeneration_menu(5, 2, false);
        assert!(mixed.is_enabled);
        assert_eq!(mixed.text, "Thumbnails (Random Pos.) (2 Videos)");
        assert!(mixed.tooltip.as_deref().unwrap_or("").contains("3 other selected files are skipped"));
    }

    #[test]
    fn regenerate_entry_is_disabled_while_another_operation_runs() {
        let busy = regeneration_menu(2, 2, true);
        assert!(!busy.is_enabled);
        assert!(busy.tooltip.as_deref().unwrap_or("").contains("in progress"));
    }

    #[test]
    fn open_with_entries_fall_back_to_the_generic_application_icon() {
        let menu = MediaContextMenu::build_menu(&meta(1, false), false, None, false);
        let open_with = menu.items.iter().find(|i| i.text == "Open With...").expect("submenu");
        let app_entry = open_with.children.iter().find(|i| i.text == "Viewer").expect("app entry");
        assert_eq!(app_entry.icon, Some(Icon::AppWindow));
    }

    #[test]
    fn rating_entries_use_the_colored_rating_icons() {
        let menu = MediaContextMenu::build_menu(&meta(1, false), false, None, false);
        let rate = menu.items.iter().find(|i| i.text == "Rate Selection").expect("submenu");
        let icons: Vec<Option<Icon>> = rate.children.iter().filter(|i| i.id != 0).map(|i| i.icon).collect();
        assert_eq!(
            icons,
            vec![
                Some(Icon::Rating5),
                Some(Icon::Rating4),
                Some(Icon::Rating3),
                Some(Icon::Rating2),
                Some(Icon::Rating1),
                Some(Icon::RatingClear)
            ]
        );
        assert!(rate.children.iter().all(|i| !i.text.contains('*')));
    }
}
