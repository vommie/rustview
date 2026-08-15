use super::favorites::FavoriteManager;
use super::{FavoriteItemView, MainWindow};
use parking_lot::RwLock;
use slint::{ModelRc, VecModel};

/// Subsystem managing favorites panel presentation in the sidebar.
pub struct BridgeFavorites;

impl BridgeFavorites {
    /// Reloads the favorites list and reflects validity of stored folder paths in the UI.
    pub fn reload(window: &MainWindow, favorites: &RwLock<FavoriteManager>) {
        let favs = favorites.read().list();
        let views: Vec<FavoriteItemView> = favs
            .into_iter()
            .map(|(alias, path, valid)| FavoriteItemView {
                alias: alias.into(),
                path: path.to_string_lossy().to_string().into(),
                is_valid: valid,
                is_selected: false,
            })
            .collect();
        window.set_favorites(ModelRc::new(VecModel::from(views)));
    }
}
