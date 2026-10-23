pub mod manager;
pub mod rename_dialog;

use crate::ui::favorites::manager::FavoriteManager;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::{FavoriteItemView, MainWindow};
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::FolderId;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    /// Persistent, surgical vector model handle for the Favorites panel list.
    static FAVORITES_MODEL: RefCell<Option<Rc<VecModel<FavoriteItemView>>>> = const { RefCell::new(None) };
}

/// Computes the subtle original folder name suffix, e.g. "(pics/)"
fn compute_original_folder_suffix(path: &Path) -> String {
    let name = path.file_name().map_or_else(
        || path.to_string_lossy().to_string(),
        |f| f.to_string_lossy().to_string(),
    );
    format!("({}/)", name.trim_end_matches(['/', '\\']))
}

/// Subsystem managing favorites panel presentation, surgical updates, and seamless navigation.
pub struct BridgeFavorites;

impl BridgeFavorites {
    /// Initializes and sets the permanent `VecModel` handle for the Favorites list.
    pub fn init(window: &MainWindow, favorites: &RwLock<FavoriteManager>) {
        let t0 = Instant::now();
        let fav_entries = favorites.read().entries().to_vec();
        let views: Vec<FavoriteItemView> = fav_entries
            .into_iter()
            .map(|entry| {
                let original_folder = compute_original_folder_suffix(&entry.path);
                let is_valid = entry.path.is_dir();
                FavoriteItemView {
                    alias: entry.alias.into(),
                    original_folder: original_folder.into(),
                    path: entry.path.to_string_lossy().to_string().into(),
                    is_valid,
                    is_selected: false,
                }
            })
            .collect();

        let model = Rc::new(VecModel::from(views));
        window.set_favorites(ModelRc::from(model.clone()));

        FAVORITES_MODEL.with(|cell| {
            *cell.borrow_mut() = Some(model);
        });

        tracing::debug!(
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeFavorites: Init] Initialized surgical VecModel for favorites"
        );
    }

    /// Full synchronized reload fallback (rebuilds the surgical `VecModel` if invalidated).
    pub fn reload(window: &MainWindow, favorites: &RwLock<FavoriteManager>) {
        Self::init(window, favorites);
    }

    /// Checks if a folder path is already in favorites without acquiring a write lock.
    pub fn is_favorite(favorites: &RwLock<FavoriteManager>, path: &Path) -> bool {
        favorites.read().has_path(path)
    }

    /// Surgically appends a new favorite item to the Slint model without destroying the list.
    pub fn add_favorite(
        window: &MainWindow,
        favorites: &Arc<RwLock<FavoriteManager>>,
        path: &Path,
        alias: Option<String>,
    ) -> Result<(), String> {
        let t0 = Instant::now();
        let new_entry = favorites.write().add(path, alias)?;

        let is_valid = new_entry.path.is_dir();
        let original_folder = compute_original_folder_suffix(&new_entry.path);

        let view_item = FavoriteItemView {
            alias: new_entry.alias.clone().into(),
            original_folder: original_folder.into(),
            path: new_entry.path.to_string_lossy().to_string().into(),
            is_valid,
            is_selected: false,
        };

        FAVORITES_MODEL.with(|cell| {
            if let Some(ref model) = *cell.borrow() {
                model.push(view_item);
                tracing::info!(
                    alias = %new_entry.alias,
                    path = %LogObscurer::path(&new_entry.path),
                    elapsed_us = t0.elapsed().as_micros(),
                    "[BridgeFavorites: SurgicalAdd] Appended favorite to model without re-render"
                );
            } else {
                Self::init(window, favorites);
            }
        });

        StatusBarBridge::success(window, format!("Added folder '{}' to favorites.", new_entry.alias));

        Ok(())
    }

    /// Surgically renames a favorite in-place without destroying the model.
    pub fn rename_favorite(
        window: &MainWindow,
        favorites: &Arc<RwLock<FavoriteManager>>,
        path: &Path,
        new_alias: &str,
    ) -> Result<(), String> {
        let t0 = Instant::now();
        favorites.write().rename(path, new_alias)?;
        let normalized = normalize_path(path);

        let mut updated = false;
        FAVORITES_MODEL.with(|cell| {
            if let Some(ref model) = *cell.borrow() {
                let total = model.row_count();
                for idx in 0..total {
                    if let Some(mut row) = model.row_data(idx)
                        && normalize_path(Path::new(row.path.as_str())) == normalized
                    {
                        row.alias = new_alias.to_string().into();
                        model.set_row_data(idx, row);
                        updated = true;
                        tracing::info!(
                            index = idx,
                            new_alias = %LogObscurer::folder(None, Path::new(new_alias)),
                            elapsed_us = t0.elapsed().as_micros(),
                            "[BridgeFavorites: SurgicalRename] Updated single row in-place"
                        );
                        break;
                    }
                }
            }
        });

        if !updated {
            Self::init(window, favorites);
        }

        Ok(())
    }

    /// Surgically removes a favorite entry from the Slint model without destroying the list.
    pub fn remove_favorite(window: &MainWindow, favorites: &Arc<RwLock<FavoriteManager>>, path: &Path) {
        let t0 = Instant::now();
        let normalized = normalize_path(path);
        let removed_res = favorites.write().remove_by_path(path);

        let mut removed_idx = None;
        FAVORITES_MODEL.with(|cell| {
            if let Some(ref model) = *cell.borrow() {
                let total = model.row_count();
                for idx in 0..total {
                    if let Some(row) = model.row_data(idx)
                        && normalize_path(Path::new(row.path.as_str())) == normalized
                    {
                        model.remove(idx);
                        removed_idx = Some(idx);
                        tracing::info!(
                            index = idx,
                            path = %LogObscurer::path(path),
                            elapsed_us = t0.elapsed().as_micros(),
                            "[BridgeFavorites: SurgicalRemove] Removed single row without list re-render"
                        );
                        break;
                    }
                }
            }
        });

        if removed_idx.is_none() {
            Self::init(window, favorites);
        }

        if let Ok(entry) = removed_res {
            StatusBarBridge::success(window, format!("Removed '{}' from favorites.", entry.alias));
        }
    }

    /// Surgically marks the favorite pointing to `target_path` as selected and deselects all others.
    pub fn set_selected_path(target_path_opt: Option<&Path>) {
        let normalized_opt = target_path_opt.map(normalize_path);

        FAVORITES_MODEL.with(|cell| {
            if let Some(ref model) = *cell.borrow() {
                let count = model.row_count();
                for idx in 0..count {
                    if let Some(mut row) = model.row_data(idx) {
                        let row_p = normalize_path(Path::new(row.path.as_str()));
                        let should_select = normalized_opt.as_ref().is_some_and(|p| *p == row_p);
                        if row.is_selected != should_select {
                            row.is_selected = should_select;
                            model.set_row_data(idx, row);
                        }
                    }
                }
            }
        });
    }

    /// Handles clicking a favorite item:
    /// - Checks target existence.
    /// - Guards against double-loading if the folder is already open.
    /// - If the target lies under the library root, auto-expands the Folder Tree ancestors and selects the folder in the tree background.
    /// - Keeps the Favorites panel strictly in the foreground (does not switch to Folders tab!).
    pub fn handle_favorite_clicked(
        path_str: &str,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t0 = Instant::now();
        let target_path = PathBuf::from(path_str);
        let normalized_target = normalize_path(&target_path);

        if !normalized_target.is_dir() {
            tracing::warn!(
                target_path = %LogObscurer::path(&normalized_target),
                "[BridgeFavorites: Click] Target directory does not exist on disk"
            );
            StatusBarBridge::warn(
                window,
                format!(
                    "Favorite folder does not exist on disk: {}",
                    normalized_target.display()
                ),
            );
            return;
        }

        // 1. Double-open prevention check: Is this folder already currently open in Browse View?
        let current_folder_path = state.read().current_folder_path.clone();
        if let Some(ref cur) = current_folder_path
            && normalize_path(cur) == normalized_target
        {
            tracing::info!(
                folder = %LogObscurer::path(&normalized_target),
                "[BridgeFavorites: Click] Folder is already currently open in Browse View. Bypassing redundant reload."
            );
            Self::set_selected_path(Some(&normalized_target));
            {
                let mut st = state.write();
                st.last_active_panel = LastActivePanel::Favorite;
                st.selected_favorite_path = Some(normalized_target);
            }
            return;
        }

        let app_settings = settings.get();
        let root_opt = app_settings.general.root_folder.as_ref().map(|p| normalize_path(p));

        // 2. Resolve or register the folder in library LMDB
        let folder_rec = match library.folders().get_folder_by_path(&normalized_target) {
            Ok(Some(rec)) => rec,
            _ => match library.folders().add_folder(&normalized_target) {
                Ok(rec) => rec,
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        folder = %LogObscurer::path(&normalized_target),
                        "[BridgeFavorites: Click] Failed to register favorite folder in database"
                    );
                    StatusBarBridge::error(window, format!("Failed to open folder: {e}"));
                    return;
                }
            },
        };

        // 3. Subtree expansion check: Does the target lie under the configured library root?
        if let Some(ref root_p) = root_opt
            && normalized_target.starts_with(root_p)
        {
            tracing::info!(
                folder = %LogObscurer::path(&normalized_target),
                root = %LogObscurer::path(root_p),
                "[BridgeFavorites: Click] Target folder is under root. Auto-expanding folder tree ancestors in background..."
            );

            let mut keep_expanded_ids: HashSet<FolderId> = HashSet::new();

            // Ensure root folder itself is expanded
            if let Ok(Some(root_rec)) = library.folders().get_folder_by_path(root_p) {
                keep_expanded_ids.insert(root_rec.id);
            }

            // Traverse ancestors upwards from target folder to root
            let mut curr_ancestor = normalized_target.parent();
            while let Some(parent_p) = curr_ancestor {
                if parent_p.starts_with(root_p) {
                    if let Ok(Some(parent_rec)) = library.folders().get_folder_by_path(parent_p) {
                        keep_expanded_ids.insert(parent_rec.id);
                    }
                    if parent_p == root_p.as_path() {
                        break;
                    }
                    curr_ancestor = parent_p.parent();
                } else {
                    break;
                }
            }

            {
                let mut st = state.write();
                for id in keep_expanded_ids {
                    st.expanded_folder_ids.insert(id);
                }
                BridgeFolders::persist_expanded_folders(settings, &st.expanded_folder_ids);
            }

            // Reload the FolderTree in the background so nodes are expanded and folder is highlighted
            BridgeFolders::reload_tree(window, library, settings, state);
        }

        // 4. Open the folder in Browse View without switching away from Favorites sidebar tab
        BridgeFolders::select_folder(
            folder_rec.id,
            &normalized_target.clone(),
            window,
            library,
            thumbnails,
            settings,
            state,
        );

        // 5. Surgically mark this favorite as selected in the favorites panel list & update state
        Self::set_selected_path(Some(&normalized_target));
        {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Favorite;
            st.selected_favorite_path = Some(normalized_target.clone());
        }

        tracing::info!(
            folder_id = folder_rec.id,
            path = %LogObscurer::path(&normalized_target),
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeFavorites: Click] Successfully navigated to favorite folder without disturbing panel dock"
        );
    }
}
