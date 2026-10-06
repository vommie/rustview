use super::MainWindow;
use super::bridge_favorites::BridgeFavorites;
use super::bridge_status_bar::StatusBarBridge;
use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder};
use crate::settings::LogObscurer;
use crate::system::SystemFileHandler;
use crate::ui::bridge_favorite_dialog_rename::FavoriteRenameDialogController;
use crate::ui::favorites::FavoriteManager;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;

/// Domain context menu builder and action handler for individual favorites in the favorites panel.
pub struct FavoriteContextMenu;

impl FavoriteContextMenu {
    /// Builds the dedicated context menu for a favorite item.
    pub fn build_menu(alias: &str, path: &Path, is_valid: bool) -> CtxMenu {
        let (header_title, _) = CtxMenuBuilder::format_header_title("Favorite: ", alias, 22);
        let header_tooltip = format!("Favorite: {} ({})", alias, path.display());

        let mut builder = CtxMenuBuilder::new().header_with_tooltip(header_title, header_tooltip);

        // 1. Open in File Manager (disabled if invalid on disk)
        builder = builder.action(
            crate::ui::bridge_state::CTX_FAVORITE_OPEN_IN_FILE_MANAGER,
            "Open in File Manager",
            None,
            Some("folder"),
            is_valid,
        );

        builder = builder.separator();

        // 2. Virtual Rename Favorite
        builder = builder.action(
            crate::ui::bridge_state::CTX_FAVORITE_RENAME,
            "Rename Favorite...",
            Some("F2"),
            Some("edit"),
            true,
        );

        builder = builder.separator();

        // 3. Virtual Remove from Favorites
        builder = builder.action(
            crate::ui::bridge_state::CTX_FAVORITE_REMOVE,
            "Remove from Favorites",
            Some("Del"),
            Some("trash"),
            true,
        );

        builder.build()
    }

    /// Handles favorite context menu action routing.
    pub fn handle_action(
        action_id: CtxMenuActionId,
        window: &MainWindow,
        favorites: &Arc<RwLock<FavoriteManager>>,
        rename_dialog: &FavoriteRenameDialogController,
        target_info: Option<(String, std::path::PathBuf)>,
    ) {
        let Some((alias, path)) = target_info else {
            tracing::warn!("[FavoriteContextMenu: Action] No active favorite target found for action");
            return;
        };

        match action_id {
            crate::ui::bridge_state::CTX_FAVORITE_OPEN_IN_FILE_MANAGER => {
                tracing::info!(
                    alias = %alias,
                    path = %LogObscurer::path(&path),
                    "[FavoriteContextMenu] Open Favorite in File Manager triggered"
                );
                if path.exists() {
                    if let Err(e) = SystemFileHandler::open_folder(&path) {
                        tracing::error!(error = %e, "[FavoriteContextMenu] Failed to open folder in File Manager");
                        StatusBarBridge::error(window, format!("Failed to open File Manager: {}", e));
                    } else {
                        StatusBarBridge::success(
                            window,
                            format!("Opened favorite '{}' in File Manager.", alias),
                        );
                    }
                } else {
                    StatusBarBridge::warn(
                        window,
                        format!("Favorite directory does not exist on disk: {}", path.display()),
                    );
                }
            }
            crate::ui::bridge_state::CTX_FAVORITE_RENAME => {
                tracing::info!(
                    alias = %alias,
                    path = %LogObscurer::path(&path),
                    "[FavoriteContextMenu] Opening Rename Favorite dialog"
                );
                rename_dialog.show(window, &alias, path, favorites.clone());
            }
            crate::ui::bridge_state::CTX_FAVORITE_REMOVE => {
                tracing::info!(
                    alias = %alias,
                    path = %LogObscurer::path(&path),
                    "[FavoriteContextMenu] Removing favorite"
                );
                BridgeFavorites::remove_favorite(window, favorites, &path);
            }
            _ => {}
        }
    }
}
