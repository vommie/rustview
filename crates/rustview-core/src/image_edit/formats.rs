//! Which pictures can be edited and in which formats they can be written (pure).
//!
//! A picture can be edited when it is a still raster picture `RustView` can decode. Its file is overwritten in place only when
//! `RustView` can write that format faithfully (`WriteFormat::in_place`); every other picture is saved through the export
//! dialog as a new file. Animated and multi-page files are never edited: the editor sees only their first frame, and writing
//! it back would throw the rest away.

use super::probe::SourceProbe;
use crate::media::MediaFormat;

/// A format `RustView` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WriteFormat {
    Jpeg,
    Png,
    Webp,
    Avif,
    Tiff,
    Bmp,
    Gif,
    Qoi,
    Tga,
    Farbfeld,
}

impl WriteFormat {
    /// What the export dialog offers, in its order.
    pub const EXPORT_TARGETS: [Self; 6] = [Self::Jpeg, Self::Png, Self::Webp, Self::Avif, Self::Tiff, Self::Bmp];

    /// The format a file of `format` is overwritten in, if `RustView` writes it faithfully.
    pub fn in_place(format: MediaFormat) -> Option<Self> {
        Some(match format {
            MediaFormat::Jpeg => Self::Jpeg,
            MediaFormat::Png => Self::Png,
            MediaFormat::Webp => Self::Webp,
            MediaFormat::Avif => Self::Avif,
            MediaFormat::Tiff => Self::Tiff,
            MediaFormat::Bmp => Self::Bmp,
            MediaFormat::Gif => Self::Gif,
            MediaFormat::Qoi => Self::Qoi,
            MediaFormat::Tga => Self::Tga,
            MediaFormat::Farbfeld => Self::Farbfeld,
            _ => return None,
        })
    }

    /// Stable key (settings file, remembered export choice); never rename one.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Jpeg => "jpeg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Tiff => "tiff",
            Self::Bmp => "bmp",
            Self::Gif => "gif",
            Self::Qoi => "qoi",
            Self::Tga => "tga",
            Self::Farbfeld => "farbfeld",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [
            Self::Jpeg,
            Self::Png,
            Self::Webp,
            Self::Avif,
            Self::Tiff,
            Self::Bmp,
            Self::Gif,
            Self::Qoi,
            Self::Tga,
            Self::Farbfeld,
        ]
        .into_iter()
        .find(|f| f.id() == id)
    }

    pub fn media_format(self) -> MediaFormat {
        match self {
            Self::Jpeg => MediaFormat::Jpeg,
            Self::Png => MediaFormat::Png,
            Self::Webp => MediaFormat::Webp,
            Self::Avif => MediaFormat::Avif,
            Self::Tiff => MediaFormat::Tiff,
            Self::Bmp => MediaFormat::Bmp,
            Self::Gif => MediaFormat::Gif,
            Self::Qoi => MediaFormat::Qoi,
            Self::Tga => MediaFormat::Tga,
            Self::Farbfeld => MediaFormat::Farbfeld,
        }
    }

    pub fn label(self) -> &'static str {
        self.media_format().display_name()
    }

    /// The extension a new file of this format gets.
    pub fn extension(self) -> &'static str {
        self.media_format().default_extension()
    }

    /// Transparency survives.
    pub const fn keeps_alpha(self) -> bool {
        !matches!(self, Self::Jpeg)
    }

    /// 16 bits per channel survive.
    pub const fn keeps_16bit(self) -> bool {
        matches!(self, Self::Png | Self::Tiff | Self::Farbfeld)
    }

    /// EXIF, the ICC profile and XMP survive.
    pub const fn keeps_metadata(self) -> bool {
        matches!(self, Self::Jpeg | Self::Png | Self::Webp | Self::Tiff)
    }

    /// Writing never loses picture information (the pixels read back are the pixels written).
    pub const fn is_lossless(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Avif)
    }
}

/// How an editable picture is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSupport {
    /// The original is overwritten in this format (the format the file really has, whatever its extension says).
    InPlace(WriteFormat),
    /// The original cannot be written: saving goes through the export dialog (a new file).
    ExportOnly(ExportReason),
}

/// Why a picture is saved as a new file instead of overwriting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportReason {
    /// `RustView` reads the format but cannot write it.
    FormatNotWritable(MediaFormat),
    /// The file has transparency or more than 8 bits per channel that `RustView` would lose writing it back.
    WouldLoseDetail(MediaFormat),
}

impl ExportReason {
    pub fn text(self) -> String {
        match self {
            Self::FormatNotWritable(format) => format!(
                "RustView can show {} files but cannot write them, so the edited picture is saved as a new file.",
                format.display_name()
            ),
            Self::WouldLoseDetail(format) => format!(
                "This {} file has transparency or more than 8 bits per channel that RustView cannot write back, so the \
                 edited picture is saved as a new file.",
                format.display_name()
            ),
        }
    }
}

/// Why a picture cannot be edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRefusal {
    Animated,
    MultiPage,
    Video,
    Unknown,
    /// The file could not be examined (it is missing, unreadable or damaged).
    Unreadable,
}

impl EditRefusal {
    /// The tooltip of the disabled crop button.
    pub fn text(self) -> &'static str {
        match self {
            Self::Animated => "Crop (disabled: animated pictures cannot be edited)",
            Self::MultiPage => "Crop (disabled: pictures with several pages cannot be edited)",
            Self::Video => "Crop (disabled: videos cannot be edited)",
            Self::Unknown => "Crop (disabled: the format of this file is not known)",
            Self::Unreadable => "Crop (disabled: the file could not be examined)",
        }
    }
}

/// Whether and how a picture of `format` (the format of its extension) with the probe `probe` is saved.
pub fn save_support(format: MediaFormat, probe: &SourceProbe) -> Result<SaveSupport, EditRefusal> {
    if probe.animated {
        return Err(EditRefusal::Animated);
    }
    if probe.multi_page {
        return Err(EditRefusal::MultiPage);
    }
    // A PNG called "x.jpg" stays a PNG: the content decides, the extension is only the fallback.
    let actual = probe.signature.unwrap_or(format);
    match actual {
        MediaFormat::Unknown => return Err(EditRefusal::Unknown),
        f if f.media_type() == Some(crate::media::MediaType::Video) => return Err(EditRefusal::Video),
        _ => {}
    }
    let Some(write) = WriteFormat::in_place(actual) else {
        return Ok(SaveSupport::ExportOnly(ExportReason::FormatNotWritable(actual)));
    };
    // AVIF is decoded by FFmpeg to 8-bit RGB: transparency or a deeper picture would be lost when writing it back.
    if write == WriteFormat::Avif && (probe.has_alpha != Some(false) || probe.bit_depth.is_none_or(|d| d > 8)) {
        return Ok(SaveSupport::ExportOnly(ExportReason::WouldLoseDetail(actual)));
    }
    Ok(SaveSupport::InPlace(write))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn still(signature: Option<MediaFormat>) -> SourceProbe {
        SourceProbe {
            signature,
            ..SourceProbe::default()
        }
    }

    #[test]
    fn writable_formats_are_overwritten_in_place() {
        for (format, write) in [
            (MediaFormat::Jpeg, WriteFormat::Jpeg),
            (MediaFormat::Png, WriteFormat::Png),
            (MediaFormat::Webp, WriteFormat::Webp),
            (MediaFormat::Tiff, WriteFormat::Tiff),
            (MediaFormat::Bmp, WriteFormat::Bmp),
            (MediaFormat::Gif, WriteFormat::Gif),
            (MediaFormat::Qoi, WriteFormat::Qoi),
            (MediaFormat::Tga, WriteFormat::Tga),
            (MediaFormat::Farbfeld, WriteFormat::Farbfeld),
        ] {
            assert_eq!(
                save_support(format, &still(Some(format))),
                Ok(SaveSupport::InPlace(write))
            );
        }
    }

    #[test]
    fn formats_rustview_only_reads_are_exported() {
        for format in [
            MediaFormat::Heic,
            MediaFormat::Jxl,
            MediaFormat::Jp2,
            MediaFormat::Psd,
            MediaFormat::Svg,
            MediaFormat::Dds,
            MediaFormat::Ico,
            MediaFormat::Exr,
            MediaFormat::Hdr,
            MediaFormat::Pnm,
        ] {
            assert_eq!(
                save_support(format, &still(None)),
                Ok(SaveSupport::ExportOnly(ExportReason::FormatNotWritable(format)))
            );
        }
    }

    #[test]
    fn the_content_decides_over_the_extension() {
        assert_eq!(
            save_support(MediaFormat::Jpeg, &still(Some(MediaFormat::Png))),
            Ok(SaveSupport::InPlace(WriteFormat::Png))
        );
        assert_eq!(
            save_support(MediaFormat::Png, &still(Some(MediaFormat::Heic))),
            Ok(SaveSupport::ExportOnly(ExportReason::FormatNotWritable(
                MediaFormat::Heic
            )))
        );
    }

    #[test]
    fn animations_pages_and_videos_cannot_be_edited() {
        let animated = SourceProbe {
            animated: true,
            ..still(Some(MediaFormat::Gif))
        };
        assert_eq!(save_support(MediaFormat::Gif, &animated), Err(EditRefusal::Animated));
        let pages = SourceProbe {
            multi_page: true,
            ..still(Some(MediaFormat::Tiff))
        };
        assert_eq!(save_support(MediaFormat::Tiff, &pages), Err(EditRefusal::MultiPage));
        assert_eq!(save_support(MediaFormat::Mp4, &still(None)), Err(EditRefusal::Video));
        assert_eq!(
            save_support(MediaFormat::Unknown, &still(None)),
            Err(EditRefusal::Unknown)
        );
    }

    #[test]
    fn only_an_opaque_8_bit_avif_is_written_back() {
        let avif = |alpha: Option<bool>, depth: Option<u8>| SourceProbe {
            has_alpha: alpha,
            bit_depth: depth,
            ..still(Some(MediaFormat::Avif))
        };
        assert_eq!(
            save_support(MediaFormat::Avif, &avif(Some(false), Some(8))),
            Ok(SaveSupport::InPlace(WriteFormat::Avif))
        );
        for (alpha, depth) in [
            (Some(true), Some(8)),
            (Some(false), Some(10)),
            (None, Some(8)),
            (Some(false), None),
        ] {
            assert!(matches!(
                save_support(MediaFormat::Avif, &avif(alpha, depth)),
                Ok(SaveSupport::ExportOnly(ExportReason::WouldLoseDetail(_)))
            ));
        }
    }

    #[test]
    fn write_format_ids_round_trip() {
        for format in WriteFormat::EXPORT_TARGETS {
            assert_eq!(WriteFormat::from_id(format.id()), Some(format));
            assert_ne!(format.extension(), "");
        }
        assert_eq!(WriteFormat::from_id("nope"), None);
        assert_eq!(WriteFormat::Tiff.extension(), "tiff");
    }
}
