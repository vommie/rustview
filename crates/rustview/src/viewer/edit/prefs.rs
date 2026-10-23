//! The user's Image Edit settings (Settings > Image Edit) as a snapshot the editor reads cheaply. The settings dialog is the
//! only writer (`apply`, at startup and after every change).

use parking_lot::RwLock;
use rustview_core::image_edit::options::{CropGrid, EncodeOptions, OverwriteMode};
use rustview_core::image_edit::ratio::RatioSet;
use rustview_core::settings::schema::ImageEditSettings;
use std::sync::{Arc, LazyLock};

#[derive(Debug, Clone, PartialEq)]
pub struct EditPrefs {
    pub ratios: RatioSet,
    pub grid: CropGrid,
    /// Every JPEG crop keeps its top left corner on the block grid (always lossless).
    pub align_jpeg: bool,
    pub overwrite: OverwriteMode,
    pub presets: EncodeOptions,
}

impl EditPrefs {
    pub fn from_settings(settings: &ImageEditSettings) -> Self {
        Self {
            ratios: settings.crop.ratio_set(),
            grid: settings.crop.grid_mode(),
            align_jpeg: settings.crop.align_jpeg_to_lossless_grid,
            overwrite: settings.save.mode(),
            presets: settings.presets.encode_options(),
        }
    }
}

static PREFS: LazyLock<RwLock<Arc<EditPrefs>>> =
    LazyLock::new(|| RwLock::new(Arc::new(EditPrefs::from_settings(&ImageEditSettings::default()))));

pub fn current() -> Arc<EditPrefs> {
    PREFS.read().clone()
}

pub fn apply(settings: &ImageEditSettings) {
    let prefs = EditPrefs::from_settings(settings);
    tracing::info!(
        ratios = prefs.ratios.presets.len(),
        grid = prefs.grid.as_str(),
        align_jpeg = prefs.align_jpeg,
        overwrite = prefs.overwrite.as_str(),
        "[ImageEdit: Prefs] Applied"
    );
    *PREFS.write() = Arc::new(prefs);
}
