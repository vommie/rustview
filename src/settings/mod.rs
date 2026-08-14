pub mod bridge;
pub mod registry;
pub mod schema;
pub mod store;

pub use bridge::SettingsDialogController;
pub use registry::SettingsManager;
pub use schema::{
    AppSettings, AppearanceSettings, DeletionMode, GeneralSettings, HashingMode,
    PerformanceSettings, PlayerSettings, TagSettings, WindowState,
};
pub use store::SettingsStore;
