slint::include_modules!();

pub mod bridge;
pub mod favorites;
pub mod file_selection;
pub mod models;

pub use bridge::UiBridge;
pub use favorites::FavoriteManager;
pub use file_selection::{FileSelectionModel, FileSelectionSummary};
