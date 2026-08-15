slint::include_modules!();

pub mod bridge;
pub mod bridge_context_menu;
pub mod bridge_favorites;
pub mod bridge_file_selection;
pub mod bridge_folders;
pub mod bridge_inspector;
pub mod bridge_media;
pub mod bridge_player;
pub mod bridge_state;
pub mod bridge_tags;
pub mod bridge_thumbnails;
pub mod favorites;
pub mod file_selection;
pub mod models;

pub use bridge::UiBridge;
pub use bridge_state::*;
pub use favorites::FavoriteManager;
pub use file_selection::{FileSelectionModel, FileSelectionSummary};
