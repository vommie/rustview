//! Dispatching the action chosen in a context menu to the feature that handles it.

use super::{BridgeContextMenu, ContextMenuRegistration};
use crate::ui::MainWindow;
use crate::ui::context_menu::browse::{BrowseActionContext, BrowseContextMenus};
pub use crate::ui::context_menu::favorite::FavoriteContextMenu;
pub use crate::ui::context_menu::folder::FolderContextMenu;
pub use crate::ui::context_menu::media::{MediaActionContext, MediaContextMenu};
pub use crate::ui::context_menu::presenter::CtxMenuPresenter;
pub use crate::ui::context_menu::tab::{TabActionContext, TabContextMenu};
pub use crate::ui::context_menu::tag::TagContextMenu;
use crate::ui::favorites::BridgeFavorites;
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::state::*;
use slint::ComponentHandle;

impl BridgeContextMenu {
    pub(super) fn register_action_dispatcher(window: &MainWindow, reg: ContextMenuRegistration) {
        let win = window.clone_strong();
        let lib = reg.library;
        let thumb = reg.thumbnails;
        let tab_mgr = reg.tab_manager;
        let favs = reg.favorites;
        let st = reg.state;
        let set = reg.settings;
        let disp = reg.dispatcher;
        let ren_dlg = reg.rename_dialog;
        let fav_ren_dlg = reg.favorite_rename_dialog;
        let missing_file_dlg = reg.missing_file_dialog;
        let delete_file_dlg = reg.delete_file_dialog;
        let remove_orphaned_dlg = reg.remove_orphaned_dialog;
        let conflict_dlg = reg.conflict_dialog;
        let tag_dlg = reg.tag_dialogs;
        let fld_dlg = reg.folder_dialogs;
        let fs_ctrl = reg.fullscreen;

        window.on_context_menu_item_clicked(move |action_id| {
            CtxMenuPresenter::dismiss(&win);
            tab_mgr.clear_menu_cutouts();
            let action_u64 = action_id as u64;

            // 1. Favorites Action Handling (5000..6000)
            if (5000..6000).contains(&action_u64) {
                let target_fav = st.write().active_favorite_target.take();
                FavoriteContextMenu::handle_action(action_u64, &win, &favs, &fav_ren_dlg, target_fav);
                return;
            }

            // 2. Folder Actions: Add to Favorites
            if action_u64 == CTX_FOLDER_ADD_TO_FAVORITES {
                let target_folder_id = st.write().active_folder_dialog_target.take();
                if let Some(fid) = target_folder_id
                    && let Ok(Some(rec)) = lib.folders().get_folder(fid)
                {
                    let path = std::path::PathBuf::from(&rec.path);
                    let _ = BridgeFavorites::add_favorite(&win, &favs, path, None);
                }
                return;
            }

            // 2b. Actions that only the menus of the free space and of a mixed selection have
            if (CTX_BROWSE_SELECTION_OPEN_IN_FILE_MANAGER..=CTX_BROWSE_TOGGLE_FOLDERS).contains(&action_u64) {
                BrowseContextMenus::handle_action(
                    action_u64,
                    &win,
                    BrowseActionContext {
                        library: &lib,
                        thumbnails: &thumb,
                        tab_manager: &tab_mgr,
                        settings: &set,
                        state: &st,
                        favorites: &favs,
                        delete_file_dialog: &delete_file_dlg,
                        remove_orphaned_file_dialog: &remove_orphaned_dlg,
                        folder_dialogs: &fld_dlg,
                        fullscreen: &fs_ctrl,
                    },
                );
                return;
            }

            // 3. Folder Actions: 3000..4000 (Open FM, Rename, Delete, Purge, Refresh) and Move (300_000..500_000)
            if (3000..4000).contains(&action_u64)
                || (CTX_FOLDER_MOVE_BASE..CTX_ACTION_COPY_TO_BASE).contains(&action_u64)
            {
                let target_folder_id = st.write().active_folder_dialog_target.take();
                let ctx = FolderDialogContext {
                    library: lib.clone(),
                    thumbnails: thumb.clone(),
                    tab_manager: tab_mgr.clone(),
                    settings: set.clone(),
                    state: st.clone(),
                };
                FolderContextMenu::handle_action(action_u64, &win, &fld_dlg, ctx, target_folder_id);
                return;
            }

            // 4. Tag Actions: 2000..3000 and Reparent (100_000..300_000)
            if (2000..3000).contains(&action_u64)
                || (CTX_TAG_MOVE_TARGET_BASE..CTX_FOLDER_MOVE_BASE).contains(&action_u64)
            {
                let bundle = tag_dlg.create_action_bundle(&win, &lib, &thumb, &set, &st);
                TagContextMenu::handle_action(action_u64, &win, &lib, &set, &st, bundle);
                return;
            }

            // 5. Tab Context Menu Invocations (4000..5000)
            let active_tab_target = st.write().active_tab_dialog_target.take();
            if let Some(target_tab_id) = active_tab_target {
                if (4000..5000).contains(&action_u64) {
                    TabContextMenu::handle_action(
                        action_u64,
                        TabActionContext {
                            window: &win,
                            tab_manager: &tab_mgr,
                            library: &lib,
                            settings: &set,
                            state: &st,
                            fullscreen: &fs_ctrl,
                        },
                        Some(target_tab_id),
                    );
                    return;
                }

                if let Some(tab) = tab_mgr.get_tab(target_tab_id) {
                    let media_action_ctx = MediaActionContext {
                        library: &lib,
                        thumbnails: &thumb,
                        tab_manager: &tab_mgr,
                        settings: &set,
                        state: &st,
                        dispatcher: &disp,
                        rename_dialog: &ren_dlg,
                        missing_file_dialog: &missing_file_dlg,
                        delete_file_dialog: &delete_file_dlg,
                        remove_orphaned_dialog: &remove_orphaned_dlg,
                        conflict_dialog: &conflict_dlg,
                        fullscreen: &fs_ctrl,
                    };

                    MediaContextMenu::handle_file_action_for_ids(action_u64, &win, &media_action_ctx, &[tab.media_id]);
                    return;
                }
            }

            // 6. Media Grid / Table Selection Actions (including Copy To / Move To)
            MediaContextMenu::handle_action(
                action_u64,
                &win,
                &MediaActionContext {
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tab_mgr,
                    settings: &set,
                    state: &st,
                    dispatcher: &disp,
                    rename_dialog: &ren_dlg,
                    missing_file_dialog: &missing_file_dlg,
                    delete_file_dialog: &delete_file_dlg,
                    remove_orphaned_dialog: &remove_orphaned_dlg,
                    conflict_dialog: &conflict_dlg,
                    fullscreen: &fs_ctrl,
                },
            );
        });
    }
}
