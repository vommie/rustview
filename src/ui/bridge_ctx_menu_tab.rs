use super::bridge_ctx_menu_media::MediaContextMenu;
use super::bridge_image_fullscreen::ImageFullscreenDialogController;
use super::bridge_state::*;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tab_grid::BridgeTabGrid;
use super::bridge_viewer_tabs::BridgeViewerTabs;
use super::MainWindow;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, SettingsManager};
use crate::system::DesktopApp;
use crate::viewer::{HighResImageLoader, MediaTabSession, TabId, TabPayload, ViewerTabManager};
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;

/// Context parameter bundle for tab context menu action processing.
pub struct TabActionContext<'a> {
    pub window: &'a MainWindow,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub library: &'a Arc<Library>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub fullscreen_controller: &'a ImageFullscreenDialogController,
}

/// Domain context menu builder and action handler for viewer tab items.
pub struct TabContextMenu;

impl TabContextMenu {
    /// Builds the declarative context menu for a viewer tab item with Move, Close, Open, Open With, Rename, Copy, and Delete options.
    /// Strictly leverages shared builder methods from `MediaContextMenu` to eliminate any logic duplication.
    /// Formats the header using sticky prefix and left-side ellipsis to eliminate menu text overflow while attaching the full path as tooltip.
    pub fn build_menu(
        tab: &MediaTabSession,
        tab_idx: usize,
        total_tabs: usize,
        all_tabs: &[MediaTabSession],
        associated_apps: &[DesktopApp],
        is_busy: bool,
    ) -> CtxMenu {
        let display_title = ViewerTabManager::compute_tab_title(all_tabs, tab.id);

        let (header_title, _) = CtxMenuBuilder::format_header_title("Tab: ", &display_title, 26);
        let header_tooltip = if !tab.file_path.as_os_str().is_empty() {
            format!("Tab: {} ({})", display_title, tab.file_path.display())
        } else {
            format!("Tab: {}", display_title)
        };

        let can_move_left = tab_idx > 0;
        let can_move_right = tab_idx + 1 < total_tabs;
        let has_multiple_tabs = total_tabs > 1;

        let has_tabs_to_left = tab_idx > 0;
        let has_tabs_to_right = tab_idx + 1 < total_tabs;
        let has_other_tabs = total_tabs > 1;

        let mut builder = CtxMenuBuilder::new()
            .header_with_tooltip(header_title, header_tooltip)
            .action(
                CTX_TAB_DUPLICATE,
                "Duplicate Tab",
                None,
                None,
                true,
            )
            .separator()
            .submenu("Move Tab...", None, has_multiple_tabs, move |sub| {
                sub.action(
                    CTX_TAB_MOVE_TO_START,
                    "To the Beginning",
                    None,
                    None,
                    can_move_left,
                )
                .action(
                    CTX_TAB_MOVE_LEFT,
                    "Move Left",
                    None,
                    None,
                    can_move_left,
                )
                .action(
                    CTX_TAB_MOVE_RIGHT,
                    "Move Right",
                    None,
                    None,
                    can_move_right,
                )
                .action(
                    CTX_TAB_MOVE_TO_END,
                    "To the End",
                    None,
                    None,
                    can_move_right,
                )
            })
            .separator()
            .submenu("Close Tabs...", None, has_other_tabs, move |sub| {
                sub.action(
                    CTX_TAB_CLOSE_TO_LEFT,
                    "Left of selected tab",
                    None,
                    None,
                    has_tabs_to_left,
                )
                .action(
                    CTX_TAB_CLOSE_TO_RIGHT,
                    "Right of selected tab",
                    None,
                    None,
                    has_tabs_to_right,
                )
                .action(
                    CTX_TAB_CLOSE_OTHERS,
                    "All other tabs",
                    None,
                    None,
                    has_other_tabs,
                )
            })
            .action(
                CTX_TAB_CLOSE,
                "Close Tab",
                Some("Ctrl+W"),
                None,
                true,
            );

        // Section 2: Shared File Open Actions (Open, Open With..., Open in File Manager)
        let target_file_exists = tab.file_path.is_file()
            && tab.file_path.metadata().map(|m| m.len() > 0).unwrap_or(false);

        builder = builder.separator();
        builder = MediaContextMenu::append_file_open_entries(
            builder,
            true, // is_single
            target_file_exists,
            associated_apps,
        );

        // Section 3: Shared File Management Actions (Rename, Copy, Delete File / Remove from DB)
        let is_image = matches!(tab.payload, TabPayload::Image(_))
            || matches!(tab.record.media_type, MediaType::Image);
        let is_deleted = tab.record.is_deleted || !tab.file_path.exists();

        builder = builder.separator();
        builder = MediaContextMenu::append_file_management_entries(
            builder,
            true,     // is_single
            is_image, // is_single_image
            is_deleted,
            target_file_exists,
            is_busy,
        );

        builder.build()
    }

    /// Handles tab-management context menu action routing (IDs 4001..4009).
    pub fn handle_action(
        action_id: CtxMenuActionId,
        ctx: TabActionContext<'_>,
        target_tab_id: Option<TabId>,
    ) {
        let Some(tab_id) = target_tab_id else {
            tracing::warn!("[TabContextMenu] No target tab ID found for action");
            return;
        };

        let tab_session_opt = ctx.tab_manager.get_tab(tab_id);

        match action_id {
            CTX_TAB_DUPLICATE => {
                let Some(session) = tab_session_opt else {
                    return;
                };

                tracing::info!(
                    tab_id = tab_id,
                    media_id = session.media_id,
                    filename = %LogObscurer::filename(&session.filename),
                    "[TabContextMenu: Duplicate] User triggered Duplicate Tab action from context menu"
                );

                match ctx.tab_manager.duplicate_tab(tab_id) {
                    Ok(new_tab_id) => {
                        BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);

                        let is_video_session = ctx.tab_manager
                            .get_tab(new_tab_id)
                            .map(|t| matches!(t.payload, TabPayload::Video(_)))
                            .unwrap_or(false);

                        if is_video_session {
                            let fs_dummy = ImageFullscreenDialogController::new();
                            BridgeViewerTabs::sync_video_embedding(ctx.window, ctx.tab_manager, &fs_dummy, ctx.settings);
                        } else if let Ok(file_path) = ctx.library.resolve_media_path(session.media_id) {
                            let tm_async = ctx.tab_manager.clone();
                            let w_weak = ctx.window.as_weak();
                            let lib_async = ctx.library.clone();
                            let st_async = ctx.state.clone();
                            let set_async = ctx.settings.clone();

                            rayon::spawn(move || {
                                let load_res = HighResImageLoader::load_full_resolution(&file_path);
                                let _ = w_weak.upgrade_in_event_loop(move |w| {
                                    match load_res {
                                        Ok((w_px, h_px, pixel_buffer)) => {
                                            tracing::info!(
                                                media_id = session.media_id,
                                                tab_id = new_tab_id,
                                                dimensions = format!("{}x{}", w_px, h_px),
                                                "[TabContextMenu: AsyncLoad] Duplicate image tab buffer loaded"
                                            );
                                            tm_async.update_tab_image_buffer(new_tab_id, w_px, h_px, pixel_buffer);
                                        }
                                        Err(err) => {
                                            tracing::error!(
                                                media_id = session.media_id,
                                                tab_id = new_tab_id,
                                                error = %err,
                                                "[TabContextMenu: AsyncLoad] Failed to load duplicate image tab buffer"
                                            );
                                            tm_async.set_tab_error(new_tab_id, err.to_string());
                                        }
                                    }
                                    BridgeViewerTabs::sync_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                                    if w.get_active_view_mode() == 2 {
                                        BridgeTabGrid::sync_tab_grid_ui(&w, &tm_async, &lib_async, &set_async, &st_async);
                                    }
                                });
                            });
                        }

                        if ctx.window.get_active_view_mode() == 2 {
                            BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                        }

                        let all_tabs = ctx.tab_manager.list_tabs();
                        let new_title = ViewerTabManager::compute_tab_title(&all_tabs, new_tab_id);
                        StatusBarBridge::success(
                            ctx.window,
                            format!("Duplicated tab '{}'", new_title),
                        );
                    }
                    Err(e) => {
                        tracing::error!(tab_id, error = %e, "[TabContextMenu: DuplicateError] Failed to duplicate tab");
                        StatusBarBridge::error(ctx.window, format!("Failed to duplicate tab: {}", e));
                    }
                }
            }
            CTX_TAB_MOVE_TO_START => {
                tracing::info!(tab_id, "[TabContextMenu: Move] Move tab to start requested");
                if ctx.tab_manager.move_tab_to_start(tab_id) {
                    BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    if ctx.window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    }
                }
            }
            CTX_TAB_MOVE_LEFT => {
                tracing::info!(tab_id, "[TabContextMenu: Move] Move tab left requested");
                if ctx.tab_manager.move_tab_left(tab_id) {
                    BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    if ctx.window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    }
                }
            }
            CTX_TAB_MOVE_RIGHT => {
                tracing::info!(tab_id, "[TabContextMenu: Move] Move tab right requested");
                if ctx.tab_manager.move_tab_right(tab_id) {
                    BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    if ctx.window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    }
                }
            }
            CTX_TAB_MOVE_TO_END => {
                tracing::info!(tab_id, "[TabContextMenu: Move] Move tab to end requested");
                if ctx.tab_manager.move_to_end(tab_id) {
                    BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    if ctx.window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                    }
                }
            }
            CTX_TAB_CLOSE_TO_LEFT => {
                let all_tabs_before = ctx.tab_manager.list_tabs();
                let tab_title = ViewerTabManager::compute_tab_title(&all_tabs_before, tab_id);
                let (closed_count, _) = ctx.tab_manager.close_tabs_to_left(tab_id);

                tracing::info!(
                    tab_id,
                    target_title = %tab_title,
                    closed_count,
                    "[TabContextMenu: CloseLeft] User closed tabs to the left"
                );

                BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                BridgeViewerTabs::sync_video_embedding(ctx.window, ctx.tab_manager, ctx.fullscreen_controller, ctx.settings);
                ctx.fullscreen_controller.sync_active_tab(ctx.tab_manager, ctx.settings);

                if ctx.window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                }

                if closed_count > 0 {
                    StatusBarBridge::success(
                        ctx.window,
                        format!("Closed {} tab(s) to the left of '{}'", closed_count, tab_title),
                    );
                }
            }
            CTX_TAB_CLOSE_TO_RIGHT => {
                let all_tabs_before = ctx.tab_manager.list_tabs();
                let tab_title = ViewerTabManager::compute_tab_title(&all_tabs_before, tab_id);
                let (closed_count, _) = ctx.tab_manager.close_tabs_to_right(tab_id);

                tracing::info!(
                    tab_id,
                    target_title = %tab_title,
                    closed_count,
                    "[TabContextMenu: CloseRight] User closed tabs to the right"
                );

                BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                BridgeViewerTabs::sync_video_embedding(ctx.window, ctx.tab_manager, ctx.fullscreen_controller, ctx.settings);
                ctx.fullscreen_controller.sync_active_tab(ctx.tab_manager, ctx.settings);

                if ctx.window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                }

                if closed_count > 0 {
                    StatusBarBridge::success(
                        ctx.window,
                        format!("Closed {} tab(s) to the right of '{}'", closed_count, tab_title),
                    );
                }
            }
            CTX_TAB_CLOSE_OTHERS => {
                let all_tabs_before = ctx.tab_manager.list_tabs();
                let tab_title = ViewerTabManager::compute_tab_title(&all_tabs_before, tab_id);
                let (closed_count, _) = ctx.tab_manager.close_other_tabs(tab_id);

                tracing::info!(
                    tab_id,
                    target_title = %tab_title,
                    closed_count,
                    "[TabContextMenu: CloseOthers] User closed all other tabs"
                );

                BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                BridgeViewerTabs::sync_video_embedding(ctx.window, ctx.tab_manager, ctx.fullscreen_controller, ctx.settings);
                ctx.fullscreen_controller.sync_active_tab(ctx.tab_manager, ctx.settings);

                if ctx.window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                }

                if closed_count > 0 {
                    StatusBarBridge::success(
                        ctx.window,
                        format!("Closed {} other tab(s), keeping '{}'", closed_count, tab_title),
                    );
                }
            }
            CTX_TAB_CLOSE => {
                tracing::info!(tab_id, "[TabContextMenu: Close] Close tab requested from context menu");
                ctx.tab_manager.close_tab(tab_id);
                BridgeViewerTabs::sync_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                BridgeViewerTabs::sync_video_embedding(ctx.window, ctx.tab_manager, ctx.fullscreen_controller, ctx.settings);
                ctx.fullscreen_controller.sync_active_tab(ctx.tab_manager, ctx.settings);

                if ctx.window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(ctx.window, ctx.tab_manager, ctx.library, ctx.settings, ctx.state);
                }
            }
            _ => {}
        }
    }
}
