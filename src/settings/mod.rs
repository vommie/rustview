pub mod bridge;
pub mod registry;
pub mod schema;
pub mod store;

pub use bridge::SettingsDialogController;
pub use registry::SettingsManager;
pub use schema::{
    AppSettings, AppearanceSettings, DeletionMode, GeneralSettings, GridAspectRatio,
    PerformanceSettings, PlayerSettings, TagSettings, ViewSettings, WindowState,
    DEFAULT_THUMBNAIL_ASPECT_RATIO, DEFAULT_THUMBNAIL_WIDTH,
};
pub use store::SettingsStore;
