//! The one table of every media format `RustView` knows: stable id, names, extensions, kind, decoder and the order of the
//! Settings page. Everything else (scanner, tree walker, detector, Settings page) reads this table.

use super::formats::{MediaFormat, MediaType};

/// How a format is decoded; shown on the Settings page so users know what a format depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatDecoder {
    /// Decoded in-process by the `image` crate (thumbnails and viewer); `FFmpeg` is the fallback for damaged variants.
    Native,
    /// Decoded through `FFmpeg`'s libavcodec (thumbnails and viewer).
    FFmpeg,
    /// Thumbnails and metadata through `FFmpeg` (or mpv), playback through mpv.
    Player,
}

impl FormatDecoder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Built-in decoder",
            Self::FFmpeg => "FFmpeg",
            Self::Player => "FFmpeg + mpv",
        }
    }
}

/// How common a format is; only used to order and group the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormatGroup {
    Common,
    Specialized,
    Legacy,
}

impl FormatGroup {
    pub fn title(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Specialized => "Professional and specialized",
            Self::Legacy => "Legacy and rare",
        }
    }
}

#[derive(Debug)]
pub struct FormatSpec {
    pub format: MediaFormat,
    /// Stable key stored in the settings file (`formats.disabled_formats`); never rename one.
    pub id: &'static str,
    /// Short uppercase name for columns and badges.
    pub name: &'static str,
    /// Full name for the Settings page.
    pub long_name: &'static str,
    pub media_type: Option<MediaType>,
    /// Lower-case extensions that select this format.
    pub extensions: &'static [&'static str],
    /// Extension appended to extensionless files of this format and shown in the Format column.
    pub default_extension: &'static str,
    pub group: FormatGroup,
    pub decoder: FormatDecoder,
    pub animated: bool,
    /// The extension is also used by unrelated files, so a file is only taken when its header matches
    /// (`MediaDetector::confirms`).
    pub needs_header_check: bool,
    /// Short hint for the Settings page; empty when there is nothing to add.
    pub note: &'static str,
    /// The MIME type desktop applications know the format by ("Open With", clipboard).
    pub mime: &'static str,
}

// A table constructor: every column of a row is one argument.
#[allow(clippy::too_many_arguments)]
const fn spec(
    format: MediaFormat,
    id: &'static str,
    name: &'static str,
    long_name: &'static str,
    media_type: Option<MediaType>,
    extensions: &'static [&'static str],
    default_extension: &'static str,
    group: FormatGroup,
    decoder: FormatDecoder,
) -> FormatSpec {
    FormatSpec {
        format,
        id,
        name,
        long_name,
        media_type,
        extensions,
        default_extension,
        group,
        decoder,
        animated: false,
        needs_header_check: false,
        note: "",
        mime: "application/octet-stream",
    }
}

const fn animated(mut s: FormatSpec) -> FormatSpec {
    s.animated = true;
    s
}

const fn header_checked(mut s: FormatSpec) -> FormatSpec {
    s.needs_header_check = true;
    s
}

const fn mime(mut s: FormatSpec, mime: &'static str) -> FormatSpec {
    s.mime = mime;
    s
}

const fn noted(mut s: FormatSpec, note: &'static str) -> FormatSpec {
    s.note = note;
    s
}

mod data;
pub use data::FORMAT_SPECS;

impl FormatSpec {
    /// The `(media type, group, short name)` order of the Settings page, the name compared without case.
    pub fn ui_sort_key(&self) -> (u8, FormatGroup, String) {
        let kind = match self.media_type {
            Some(MediaType::Image) => 0,
            Some(MediaType::Video) => 1,
            None => 2,
        };
        (kind, self.group, self.name.to_lowercase())
    }
}
