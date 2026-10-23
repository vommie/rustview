//! Settings > Image Edit: the crop tool, how the original is replaced, and the presets of the formats that are written.
//! Every choice is stored as a string or a number, so a hand-edited file never breaks parsing (`sanitize` repairs it).

use super::{
    DEFAULT_CROP_ALIGN_JPEG_TO_LOSSLESS_GRID, DEFAULT_CROP_ASPECT_RATIOS, DEFAULT_CROP_GRID,
    DEFAULT_PRESET_AVIF_QUALITY, DEFAULT_PRESET_AVIF_SPEED, DEFAULT_PRESET_JPEG_PROGRESSIVE,
    DEFAULT_PRESET_JPEG_QUALITY, DEFAULT_PRESET_JPEG_SUBSAMPLING, DEFAULT_PRESET_PNG_COMPRESSION,
    DEFAULT_PRESET_TIFF_COMPRESSION, DEFAULT_PRESET_WEBP_LOSSLESS, DEFAULT_PRESET_WEBP_QUALITY,
    DEFAULT_SAVE_OVERWRITE_MODE, MAX_CROP_ASPECT_RATIOS,
};
use crate::image_edit::formats::WriteFormat;
use crate::image_edit::options::{
    ChromaSubsampling, CropGrid, EncodeOptions, OverwriteMode, PngCompression, TiffCompression,
};
use crate::image_edit::ratio::RatioSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CropSettings {
    /// "W:H" entries in the user's order.
    pub aspect_ratios: Vec<String>,
    /// "dragging", "always" or "never".
    pub grid: String,
    pub align_jpeg_to_lossless_grid: bool,
}

impl Default for CropSettings {
    fn default() -> Self {
        Self {
            aspect_ratios: DEFAULT_CROP_ASPECT_RATIOS.iter().map(ToString::to_string).collect(),
            grid: DEFAULT_CROP_GRID.to_string(),
            align_jpeg_to_lossless_grid: DEFAULT_CROP_ALIGN_JPEG_TO_LOSSLESS_GRID,
        }
    }
}

impl CropSettings {
    pub fn ratio_set(&self) -> RatioSet {
        RatioSet::parse(&self.aspect_ratios).0
    }

    pub fn grid_mode(&self) -> CropGrid {
        CropGrid::from_str_name(&self.grid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveSettings {
    /// "safe", "trash" or "direct".
    pub overwrite_mode: String,
}

impl Default for SaveSettings {
    fn default() -> Self {
        Self {
            overwrite_mode: DEFAULT_SAVE_OVERWRITE_MODE.to_string(),
        }
    }
}

impl SaveSettings {
    pub fn mode(&self) -> OverwriteMode {
        OverwriteMode::from_str_name(&self.overwrite_mode)
    }
}

/// The options of each format that has some.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FormatPresets {
    pub jpeg_quality: u8,
    pub jpeg_subsampling: String,
    pub jpeg_progressive: bool,
    pub png_compression: String,
    pub webp_lossless: bool,
    pub webp_quality: u8,
    pub avif_quality: u8,
    pub avif_speed: u8,
    pub tiff_compression: String,
}

impl Default for FormatPresets {
    fn default() -> Self {
        Self {
            jpeg_quality: DEFAULT_PRESET_JPEG_QUALITY,
            jpeg_subsampling: DEFAULT_PRESET_JPEG_SUBSAMPLING.to_string(),
            jpeg_progressive: DEFAULT_PRESET_JPEG_PROGRESSIVE,
            png_compression: DEFAULT_PRESET_PNG_COMPRESSION.to_string(),
            webp_lossless: DEFAULT_PRESET_WEBP_LOSSLESS,
            webp_quality: DEFAULT_PRESET_WEBP_QUALITY,
            avif_quality: DEFAULT_PRESET_AVIF_QUALITY,
            avif_speed: DEFAULT_PRESET_AVIF_SPEED,
            tiff_compression: DEFAULT_PRESET_TIFF_COMPRESSION.to_string(),
        }
    }
}

impl FormatPresets {
    pub fn encode_options(&self) -> EncodeOptions {
        EncodeOptions {
            jpeg_quality: self.jpeg_quality,
            jpeg_subsampling: ChromaSubsampling::from_str_name(&self.jpeg_subsampling),
            jpeg_progressive: self.jpeg_progressive,
            png_compression: PngCompression::from_str_name(&self.png_compression),
            webp_lossless: self.webp_lossless,
            webp_quality: self.webp_quality,
            avif_quality: self.avif_quality,
            avif_speed: self.avif_speed,
            tiff_compression: TiffCompression::from_str_name(&self.tiff_compression),
        }
        .sanitized()
    }

    pub fn from_options(options: &EncodeOptions) -> Self {
        let o = options.sanitized();
        Self {
            jpeg_quality: o.jpeg_quality,
            jpeg_subsampling: o.jpeg_subsampling.as_str().to_string(),
            jpeg_progressive: o.jpeg_progressive,
            png_compression: o.png_compression.as_str().to_string(),
            webp_lossless: o.webp_lossless,
            webp_quality: o.webp_quality,
            avif_quality: o.avif_quality,
            avif_speed: o.avif_speed,
            tiff_compression: o.tiff_compression.as_str().to_string(),
        }
    }
}

/// What the export dialog was last used with (remembered across restarts; the presets are the starting values until then).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastExport {
    /// The id of the format (`WriteFormat::id`); empty = never used.
    pub format: String,
    pub options: Option<FormatPresets>,
}

impl LastExport {
    pub fn write_format(&self) -> Option<WriteFormat> {
        WriteFormat::from_id(&self.format).filter(|f| WriteFormat::EXPORT_TARGETS.contains(f))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ImageEditSettings {
    pub crop: CropSettings,
    pub save: SaveSettings,
    pub presets: FormatPresets,
    pub last_export: LastExport,
}

impl ImageEditSettings {
    /// Repairs what a hand-edited file may contain: unknown names fall back to the defaults, numbers are clamped, invalid and
    /// repeated ratios are dropped.
    pub fn sanitize(&mut self) {
        let (set, _) = RatioSet::parse(&self.crop.aspect_ratios);
        self.crop.aspect_ratios = set
            .presets
            .iter()
            .take(MAX_CROP_ASPECT_RATIOS)
            .map(|p| p.label.clone())
            .collect();
        self.crop.grid = self.crop.grid_mode().as_str().to_string();
        self.save.overwrite_mode = self.save.mode().as_str().to_string();
        self.presets = FormatPresets::from_options(&self.presets.encode_options());
        if let Some(options) = &self.last_export.options {
            self.last_export.options = Some(FormatPresets::from_options(&options.encode_options()));
        }
        if self.last_export.write_format().is_none() {
            self.last_export.format.clear();
        }
    }
}

impl ImageEditSettings {
    /// One line per changed group for the settings change log.
    pub fn describe_changes(&self, old: &Self) -> Vec<String> {
        let mut lines = Vec::new();
        if self.crop != old.crop {
            lines.push(format!(
                "Image edit cropping: ratios: [{}], grid: '{}', align JPEG to the lossless grid: {}",
                self.crop.aspect_ratios.join(", "),
                self.crop.grid,
                self.crop.align_jpeg_to_lossless_grid
            ));
        }
        if self.save != old.save {
            lines.push(format!(
                "Image edit saving: overwrite mode: '{}'",
                self.save.overwrite_mode
            ));
        }
        if self.presets != old.presets {
            let p = &self.presets;
            lines.push(format!(
                "Image edit presets: JPEG {} ({}, progressive: {}), PNG '{}', WebP {} (lossless: {}), AVIF {} (speed {}), TIFF '{}'",
                p.jpeg_quality,
                p.jpeg_subsampling,
                p.jpeg_progressive,
                p.png_compression,
                p.webp_quality,
                p.webp_lossless,
                p.avif_quality,
                p.avif_speed,
                p.tiff_compression
            ));
        }
        if self.last_export != old.last_export {
            lines.push(format!("Image edit: last export format '{}'", self.last_export.format));
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_settings_file_without_the_section_gets_the_defaults() {
        let settings: ImageEditSettings = serde_json::from_str("{}").expect("parse");
        assert_eq!(settings, ImageEditSettings::default());
        assert_eq!(settings.crop.aspect_ratios.len(), DEFAULT_CROP_ASPECT_RATIOS.len());
        assert_eq!(settings.save.mode(), OverwriteMode::Safe);
    }

    #[test]
    fn hand_edited_values_are_repaired() {
        let mut settings: ImageEditSettings = serde_json::from_str(
            r#"{"crop": {"aspect_ratios": ["16:9", "nonsense", "32:18", "4:5"], "grid": "sometimes"},
                "save": {"overwrite_mode": "shred"},
                "presets": {"jpeg_quality": 0, "avif_speed": 99, "tiff_compression": "zip", "jpeg_subsampling": "444"},
                "last_export": {"format": "heic"}}"#,
        )
        .expect("parse");
        settings.sanitize();
        assert_eq!(settings.crop.aspect_ratios, ["16:9", "4:5"]);
        assert_eq!(settings.crop.grid, "dragging");
        assert_eq!(settings.save.overwrite_mode, "safe");
        let options = settings.presets.encode_options();
        assert_eq!(options.jpeg_quality, 1);
        assert_eq!(options.avif_speed, 10);
        assert_eq!(options.tiff_compression, TiffCompression::Lzw);
        assert_eq!(options.jpeg_subsampling, ChromaSubsampling::S444);
        assert_eq!(settings.last_export.format, "");
    }

    #[test]
    fn presets_survive_a_round_trip_through_the_file() {
        let mut settings = ImageEditSettings::default();
        settings.presets.jpeg_quality = 77;
        settings.presets.png_compression = "best".to_string();
        settings.last_export = LastExport {
            format: "webp".to_string(),
            options: Some(settings.presets.clone()),
        };
        let text = serde_json::to_string(&settings).expect("write");
        let back: ImageEditSettings = serde_json::from_str(&text).expect("read");
        assert_eq!(back, settings);
        assert_eq!(back.last_export.write_format(), Some(WriteFormat::Webp));
        assert!(text.contains("\"png_compression\":\"best\""));
    }
}
