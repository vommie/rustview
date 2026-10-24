//! What the export dialog suggests and warns about (pure): the name of the new file, the format it starts with, what the chosen
//! format loses of the picture, and whether a name can be used.

use rustview_core::image_edit::formats::WriteFormat;
use rustview_core::image_edit::options::{ChromaSubsampling, PngCompression, TiffCompression};
use rustview_core::library::file_operations::FilenameValidator;
use rustview_core::media::MediaFormat;
use std::path::{Path, PathBuf};

/// The name the dialog starts with (without extension): the original's own, so saving without renaming replaces it; the user
/// types another name for a copy.
pub fn suggested_stem(filename: &str) -> String {
    Path::new(filename)
        .file_stem()
        .map_or_else(|| filename.to_string(), |s| s.to_string_lossy().into_owned())
}

/// The format the dialog starts with: the original's own when it can be written (the unchanged dialog then replaces the
/// original), else the one used last time, else PNG for a picture with transparency and JPEG for any other.
pub fn initial_format(last: Option<WriteFormat>, source: MediaFormat, has_alpha: Option<bool>) -> WriteFormat {
    if let Some(own) = WriteFormat::in_place(source).filter(|f| WriteFormat::EXPORT_TARGETS.contains(f)) {
        return own;
    }
    if let Some(last) = last.filter(|f| WriteFormat::EXPORT_TARGETS.contains(f)) {
        return last;
    }
    if has_alpha == Some(true) {
        WriteFormat::Png
    } else {
        WriteFormat::Jpeg
    }
}

/// What `target` loses of a picture with this transparency and bit depth.
pub fn warnings(target: WriteFormat, has_alpha: Option<bool>, bit_depth: Option<u8>) -> Vec<String> {
    let mut lines = Vec::new();
    if has_alpha == Some(true) && !target.keeps_alpha() {
        lines.push(format!(
            "{} has no transparency: transparent parts of the picture become white.",
            target.label()
        ));
    }
    if bit_depth.is_some_and(|d| d > 8) && !target.keeps_16bit() {
        lines.push(format!(
            "The picture has more than 8 bits per channel; {} stores 8.",
            target.label()
        ));
    }
    match target {
        WriteFormat::Avif => lines.push("AVIF keeps the EXIF data, but not the color profile and XMP.".to_string()),
        WriteFormat::Bmp => lines.push("BMP keeps no metadata (EXIF, color profile, XMP).".to_string()),
        _ => {}
    }
    lines
}

/// Whether `stem` can be the name of the new file (without its extension).
pub fn check_stem(stem: &str) -> Result<(), String> {
    if stem.trim().is_empty() {
        return Err("Enter a file name.".to_string());
    }
    FilenameValidator::validate(stem)
        .map(|_| ())
        .map_err(|e| format!("This name cannot be used: {e}"))
}

pub fn target_path(folder: &Path, stem: &str, extension: &str) -> PathBuf {
    folder.join(format!("{}.{extension}", stem.trim()))
}

/// The options of the dialog's controls (indexes of its combo boxes) and back.
pub fn subsampling_index(s: ChromaSubsampling) -> i32 {
    match s {
        ChromaSubsampling::S420 => 0,
        ChromaSubsampling::S422 => 1,
        ChromaSubsampling::S444 => 2,
    }
}

pub fn subsampling_of(index: i32) -> ChromaSubsampling {
    match index {
        1 => ChromaSubsampling::S422,
        2 => ChromaSubsampling::S444,
        _ => ChromaSubsampling::S420,
    }
}

pub fn png_index(c: PngCompression) -> i32 {
    match c {
        PngCompression::Fast => 0,
        PngCompression::Balanced => 1,
        PngCompression::Best => 2,
    }
}

pub fn png_of(index: i32) -> PngCompression {
    match index {
        0 => PngCompression::Fast,
        2 => PngCompression::Best,
        _ => PngCompression::Balanced,
    }
}

pub fn tiff_index(c: TiffCompression) -> i32 {
    match c {
        TiffCompression::None => 0,
        TiffCompression::Lzw => 1,
        TiffCompression::Deflate => 2,
    }
}

pub fn tiff_of(index: i32) -> TiffCompression {
    match index {
        0 => TiffCompression::None,
        2 => TiffCompression::Deflate,
        _ => TiffCompression::Lzw,
    }
}

pub fn format_index(format: WriteFormat) -> i32 {
    WriteFormat::EXPORT_TARGETS
        .iter()
        .position(|f| *f == format)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(0)
}

pub fn format_of(index: i32) -> WriteFormat {
    usize::try_from(index)
        .ok()
        .and_then(|i| WriteFormat::EXPORT_TARGETS.get(i).copied())
        .unwrap_or(WriteFormat::Jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_new_name_and_format_are_suggested_sensibly() {
        assert_eq!(suggested_stem("beach.jpg"), "beach");
        assert_eq!(suggested_stem("archive.tar.gz"), "archive.tar");
        // The original's own format first (the unchanged dialog replaces the original), the last one for the others.
        assert_eq!(
            initial_format(Some(WriteFormat::Webp), MediaFormat::Png, None),
            WriteFormat::Png
        );
        assert_eq!(
            initial_format(Some(WriteFormat::Webp), MediaFormat::Heic, None),
            WriteFormat::Webp
        );
        assert_eq!(initial_format(None, MediaFormat::Tiff, None), WriteFormat::Tiff);
        assert_eq!(initial_format(None, MediaFormat::Heic, Some(false)), WriteFormat::Jpeg);
        assert_eq!(initial_format(None, MediaFormat::Svg, Some(true)), WriteFormat::Png);
        // GIF can be overwritten but is no export target.
        assert_eq!(initial_format(None, MediaFormat::Gif, None), WriteFormat::Jpeg);
    }

    #[test]
    fn the_warnings_name_what_the_format_loses() {
        assert!(warnings(WriteFormat::Jpeg, Some(true), Some(8))[0].contains("become white"));
        assert_eq!(warnings(WriteFormat::Png, Some(true), Some(16)), Vec::<String>::new());
        assert_eq!(warnings(WriteFormat::Webp, None, Some(16)).len(), 1);
        assert_eq!(warnings(WriteFormat::Bmp, Some(false), Some(8)).len(), 1);
        assert_eq!(warnings(WriteFormat::Tiff, Some(true), Some(16)), Vec::<String>::new());
    }

    #[test]
    fn names_are_checked_and_controls_map_both_ways() {
        assert!(check_stem("  ").is_err());
        assert!(check_stem("a/b").is_err());
        assert!(check_stem("holiday 2024").is_ok());
        assert_eq!(target_path(Path::new("/p"), " x ", "tiff"), PathBuf::from("/p/x.tiff"));
        for format in WriteFormat::EXPORT_TARGETS {
            assert_eq!(format_of(format_index(format)), format);
        }
        for s in [
            ChromaSubsampling::S420,
            ChromaSubsampling::S422,
            ChromaSubsampling::S444,
        ] {
            assert_eq!(subsampling_of(subsampling_index(s)), s);
        }
        for c in [PngCompression::Fast, PngCompression::Balanced, PngCompression::Best] {
            assert_eq!(png_of(png_index(c)), c);
        }
        for c in [TiffCompression::None, TiffCompression::Lzw, TiffCompression::Deflate] {
            assert_eq!(tiff_of(tiff_index(c)), c);
        }
    }
}
