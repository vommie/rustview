//! The options of writing a picture (pure): the presets of Settings > Image Edit (stored as strings, so a hand-edited settings
//! file never breaks parsing), how the original is replaced, and the choice of encoding for an in-place save or an export.

use super::formats::WriteFormat;
use super::geometry::{PixelRect, origin_on_grid};
use super::probe::SourceProbe;
use super::stack::EditPlan;

/// JPEG quality 1..=100.
pub const MIN_QUALITY: u8 = 1;
pub const MAX_QUALITY: u8 = 100;
/// AVIF encoder speed 1 (slowest, smallest) ..= 10 (fastest).
pub const MIN_AVIF_SPEED: u8 = 1;
pub const MAX_AVIF_SPEED: u8 = 10;

macro_rules! string_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal, $label:literal),+ $(,)? } default $default:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The name stored in the settings file.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// The text the user sees.
            pub const fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }

            /// Unknown names (a hand-edited file) give the default.
            pub fn from_str_name(name: &str) -> Self {
                match name { $($text => Self::$variant,)+ _ => Self::$default }
            }
        }

        impl Default for $name {
            fn default() -> Self { Self::$default }
        }
    };
}

string_enum!(
    /// How the original is replaced when it is overwritten. Every mode writes a temporary file first and replaces the
    /// original only after the new file was written completely and read back.
    OverwriteMode {
        Safe => "safe", "Safe (keep a backup until the new file is in place)",
        Trash => "trash", "Move the original to the trash",
        Direct => "direct", "Replace directly",
    } default Safe
);

string_enum!(
    ChromaSubsampling {
        S420 => "420", "4:2:0 (smallest)",
        S422 => "422", "4:2:2",
        S444 => "444", "4:4:4 (sharpest colors)",
    } default S420
);

string_enum!(
    PngCompression {
        Fast => "fast", "Fast",
        Balanced => "default", "Balanced",
        Best => "best", "Smallest file",
    } default Balanced
);

string_enum!(
    TiffCompression {
        None => "none", "None",
        Lzw => "lzw", "LZW",
        Deflate => "deflate", "Deflate",
    } default Lzw
);

string_enum!(
    /// When the rule-of-thirds lines are drawn inside the crop rectangle.
    CropGrid {
        Dragging => "dragging", "While dragging",
        Always => "always", "Always",
        Never => "never", "Never",
    } default Dragging
);

/// The presets of each format that has options (Settings > Image Edit > Formats, and the starting values of the export dialog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodeOptions {
    pub jpeg_quality: u8,
    pub jpeg_subsampling: ChromaSubsampling,
    pub jpeg_progressive: bool,
    pub png_compression: PngCompression,
    pub webp_lossless: bool,
    pub webp_quality: u8,
    pub avif_quality: u8,
    pub avif_speed: u8,
    pub tiff_compression: TiffCompression,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            jpeg_subsampling: ChromaSubsampling::S420,
            jpeg_progressive: false,
            png_compression: PngCompression::Balanced,
            webp_lossless: false,
            webp_quality: 90,
            avif_quality: 80,
            avif_speed: 6,
            tiff_compression: TiffCompression::Lzw,
        }
    }
}

impl EncodeOptions {
    /// Every value inside its range.
    pub fn sanitized(mut self) -> Self {
        self.jpeg_quality = self.jpeg_quality.clamp(MIN_QUALITY, MAX_QUALITY);
        self.webp_quality = self.webp_quality.clamp(MIN_QUALITY, MAX_QUALITY);
        self.avif_quality = self.avif_quality.clamp(MIN_QUALITY, MAX_QUALITY);
        self.avif_speed = self.avif_speed.clamp(MIN_AVIF_SPEED, MAX_AVIF_SPEED);
        self
    }
}

/// How the picture is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// Cut the blocks out of the JPEG without decoding them (no quality loss); `crop` lies on the block grid.
    JpegLossless {
        crop: PixelRect,
    },
    /// Decode, crop and encode again with the quantization tables and the sampling of the original (the quality it has).
    JpegSameTables,
    Jpeg {
        quality: u8,
        subsampling: ChromaSubsampling,
        progressive: bool,
    },
    /// Lossless; a picture with a palette keeps its palette and bit depth.
    Png {
        compression: PngCompression,
    },
    Webp {
        lossless: bool,
        quality: u8,
    },
    Avif {
        quality: u8,
        speed: u8,
    },
    Tiff {
        compression: TiffCompression,
    },
    Bmp,
    /// A still GIF keeps its palette.
    Gif,
    Qoi,
    Tga,
    Farbfeld,
}

impl Encoding {
    pub fn is_lossless(self) -> bool {
        !matches!(
            self,
            Self::JpegSameTables | Self::Jpeg { .. } | Self::Avif { .. } | Self::Webp { lossless: false, .. }
        )
    }

    /// A short description for the log and the status line ("lossless crop", "JPEG quality 90").
    pub fn describe(self) -> String {
        match self {
            Self::JpegLossless { .. } => "JPEG, cropped without quality loss".to_string(),
            Self::JpegSameTables => "JPEG, re-encoded at the original quality".to_string(),
            Self::Jpeg { quality, .. } => format!("JPEG, quality {quality}"),
            Self::Png { .. } => "PNG (lossless)".to_string(),
            Self::Webp { lossless: true, .. } => "WebP (lossless)".to_string(),
            Self::Webp { quality, .. } => format!("WebP, quality {quality}"),
            Self::Avif { quality, .. } => format!("AVIF, quality {quality}"),
            Self::Tiff { compression } => format!("TIFF ({})", compression.label()),
            Self::Bmp => "BMP".to_string(),
            Self::Gif => "GIF".to_string(),
            Self::Qoi => "QOI".to_string(),
            Self::Tga => "TGA".to_string(),
            Self::Farbfeld => "Farbfeld".to_string(),
        }
    }
}

/// The encoding of an in-place save: the file keeps its format, and as much of its quality as the format allows. A JPEG is
/// cropped without loss when the crop starts on its block grid and is otherwise encoded with its own tables; a lossless WebP
/// stays lossless; formats without a stored quality use the presets.
pub fn in_place_encoding(
    write: WriteFormat,
    probe: &SourceProbe,
    plan: &EditPlan,
    presets: &EncodeOptions,
) -> Encoding {
    let presets = presets.sanitized();
    match write {
        WriteFormat::Jpeg => {
            let layout = probe.jpeg;
            match (plan.crop, layout) {
                (Some(crop), Some(layout)) if origin_on_grid(crop, layout.mcu) => Encoding::JpegLossless { crop },
                _ => Encoding::JpegSameTables,
            }
        }
        WriteFormat::Png => Encoding::Png {
            compression: presets.png_compression,
        },
        WriteFormat::Webp => match probe.webp_lossless {
            Some(true) => Encoding::Webp {
                lossless: true,
                quality: presets.webp_quality,
            },
            _ => Encoding::Webp {
                lossless: false,
                quality: presets.webp_quality,
            },
        },
        _ => export_encoding(write, &presets),
    }
}

/// The encoding of a new file of `target` with the options of the export dialog (or the presets).
pub fn export_encoding(target: WriteFormat, options: &EncodeOptions) -> Encoding {
    let o = options.sanitized();
    match target {
        WriteFormat::Jpeg => Encoding::Jpeg {
            quality: o.jpeg_quality,
            subsampling: o.jpeg_subsampling,
            progressive: o.jpeg_progressive,
        },
        WriteFormat::Png => Encoding::Png {
            compression: o.png_compression,
        },
        WriteFormat::Webp => Encoding::Webp {
            lossless: o.webp_lossless,
            quality: o.webp_quality,
        },
        WriteFormat::Avif => Encoding::Avif {
            quality: o.avif_quality,
            speed: o.avif_speed,
        },
        WriteFormat::Tiff => Encoding::Tiff {
            compression: o.tiff_compression,
        },
        WriteFormat::Bmp => Encoding::Bmp,
        WriteFormat::Gif => Encoding::Gif,
        WriteFormat::Qoi => Encoding::Qoi,
        WriteFormat::Tga => Encoding::Tga,
        WriteFormat::Farbfeld => Encoding::Farbfeld,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_edit::probe::JpegLayout;

    fn jpeg_probe(mcu: (u32, u32)) -> SourceProbe {
        SourceProbe {
            jpeg: Some(JpegLayout {
                mcu,
                components: 3,
                progressive: false,
            }),
            ..SourceProbe::default()
        }
    }

    fn plan(crop: PixelRect) -> EditPlan {
        EditPlan {
            crop: Some(crop),
            output_size: crop.size(),
        }
    }

    #[test]
    fn a_jpeg_crop_on_the_block_grid_is_lossless_and_any_other_keeps_the_original_tables() {
        let presets = EncodeOptions::default();
        let on_grid = PixelRect::new(32, 16, 101, 77);
        assert_eq!(
            in_place_encoding(WriteFormat::Jpeg, &jpeg_probe((16, 16)), &plan(on_grid), &presets),
            Encoding::JpegLossless { crop: on_grid }
        );
        assert_eq!(
            in_place_encoding(
                WriteFormat::Jpeg,
                &jpeg_probe((16, 16)),
                &plan(PixelRect::new(8, 16, 50, 50)),
                &presets
            ),
            Encoding::JpegSameTables
        );
        // 4:4:4 has 8 px blocks: the same crop is lossless there.
        assert!(matches!(
            in_place_encoding(
                WriteFormat::Jpeg,
                &jpeg_probe((8, 8)),
                &plan(PixelRect::new(8, 16, 50, 50)),
                &presets
            ),
            Encoding::JpegLossless { .. }
        ));
        // Without a known layout there is no lossless crop.
        assert_eq!(
            in_place_encoding(WriteFormat::Jpeg, &SourceProbe::default(), &plan(on_grid), &presets),
            Encoding::JpegSameTables
        );
    }

    #[test]
    fn a_webp_keeps_its_kind_and_other_formats_use_the_presets() {
        let presets = EncodeOptions {
            webp_quality: 70,
            avif_quality: 55,
            tiff_compression: TiffCompression::Deflate,
            ..EncodeOptions::default()
        };
        let lossless = SourceProbe {
            webp_lossless: Some(true),
            ..SourceProbe::default()
        };
        let any = plan(PixelRect::new(0, 0, 10, 10));
        assert_eq!(
            in_place_encoding(WriteFormat::Webp, &lossless, &any, &presets),
            Encoding::Webp {
                lossless: true,
                quality: 70
            }
        );
        assert_eq!(
            in_place_encoding(WriteFormat::Webp, &SourceProbe::default(), &any, &presets),
            Encoding::Webp {
                lossless: false,
                quality: 70
            }
        );
        assert_eq!(
            in_place_encoding(WriteFormat::Avif, &SourceProbe::default(), &any, &presets),
            Encoding::Avif { quality: 55, speed: 6 }
        );
        assert_eq!(
            in_place_encoding(WriteFormat::Tiff, &SourceProbe::default(), &any, &presets),
            Encoding::Tiff {
                compression: TiffCompression::Deflate
            }
        );
        assert!(in_place_encoding(WriteFormat::Png, &SourceProbe::default(), &any, &presets).is_lossless());
    }

    #[test]
    fn out_of_range_values_are_clamped_and_unknown_names_fall_back() {
        let wild = EncodeOptions {
            jpeg_quality: 0,
            webp_quality: 200,
            avif_speed: 0,
            ..EncodeOptions::default()
        };
        assert_eq!(
            export_encoding(WriteFormat::Jpeg, &wild),
            Encoding::Jpeg {
                quality: 1,
                subsampling: ChromaSubsampling::S420,
                progressive: false
            }
        );
        assert_eq!(
            export_encoding(WriteFormat::Avif, &wild),
            Encoding::Avif { quality: 80, speed: 1 }
        );
        assert_eq!(OverwriteMode::from_str_name("bogus"), OverwriteMode::Safe);
        assert_eq!(OverwriteMode::from_str_name("trash"), OverwriteMode::Trash);
        for mode in OverwriteMode::ALL {
            assert_eq!(OverwriteMode::from_str_name(mode.as_str()), *mode);
        }
        assert_eq!(ChromaSubsampling::from_str_name("444"), ChromaSubsampling::S444);
        assert_eq!(CropGrid::from_str_name(""), CropGrid::Dragging);
    }
}
