use super::format_policy::FormatPolicy;
use super::format_table::{FORMAT_SPECS, FormatSpec};
use crate::util::cast::Az;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaType {
    Image,
    Video,
}

/// Every format `RustView` knows. The library stores the variant INDEX (bincode), so variants are only ever appended after
/// `Unknown`, never inserted or reordered; `FORMAT_SPECS` follows the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MediaFormat {
    // Image formats
    Jpeg,
    Png,
    Webp,
    Avif,
    Gif,
    Bmp,
    Tiff,
    Psd,

    // Video formats
    Mp4,
    Mkv,
    Webm,
    Mov,
    Avi,
    Mpeg,
    Flv,

    Unknown,

    // Appended image formats
    Ico,
    Tga,
    Pnm,
    Qoi,
    Hdr,
    Exr,
    Dds,
    Farbfeld,
    Jxl,
    Jp2,
    Svg,
    Dpx,
    Pcx,
    Sgi,
    SunRaster,
    Xbm,
    Xpm,
    Xwd,
    Wbmp,

    // Appended video formats
    Wmv,
    Ts,
    M2ts,
    Ogv,
    ThreeGp,
    Mxf,
    Dv,
    Rm,
    Y4m,
    Ivf,
    Nut,
    H264,
    Hevc,
    Gxf,
    Wtv,

    // Appended image format
    Heic,
}

/// The longest extension in `FORMAT_SPECS` fits comfortably; longer ones cannot match and are rejected without lookup.
const MAX_EXTENSION_LEN: usize = 8;

impl MediaFormat {
    /// Number of variants including `Unknown` (the length of `FORMAT_SPECS`).
    pub const COUNT: usize = Self::Heic as usize + 1;

    /// The table entry of this format.
    #[inline]
    pub fn spec(self) -> &'static FormatSpec {
        &FORMAT_SPECS[self as usize]
    }

    /// Every known format except `Unknown`, in enum order.
    pub fn all() -> impl Iterator<Item = MediaFormat> {
        FORMAT_SPECS.iter().map(|s| s.format).filter(MediaFormat::is_supported)
    }

    /// Looks a format up by its stable settings id.
    pub fn from_id(id: &str) -> Option<Self> {
        Self::all().find(|f| f.spec().id.eq_ignore_ascii_case(id))
    }

    /// Detects the media format based on the file extension (case-insensitive, without the dot).
    pub fn from_extension(ext: &str) -> Self {
        let bytes = ext.as_bytes();
        if bytes.is_empty() || bytes.len() > MAX_EXTENSION_LEN {
            return Self::Unknown;
        }
        let mut lower = [0u8; MAX_EXTENSION_LEN];
        for (dst, src) in lower.iter_mut().zip(bytes) {
            *dst = src.to_ascii_lowercase();
        }
        std::str::from_utf8(&lower[..bytes.len()])
            .ok()
            .and_then(|key| extension_map().get(key).copied())
            .unwrap_or(Self::Unknown)
    }

    /// Detects the media format from a file path.
    pub fn from_path(path: &Path) -> Self {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map_or(Self::Unknown, Self::from_extension)
    }

    /// Identifies the high-level media type.
    #[inline]
    pub fn media_type(&self) -> Option<MediaType> {
        self.spec().media_type
    }

    /// Returns true if this format supports multi-frame animations (e.g. animated GIFs).
    #[inline]
    pub fn is_animated(&self) -> bool {
        self.spec().animated
    }

    /// Returns the recommended default file extension for this format in lowercase.
    #[inline]
    pub fn default_extension(&self) -> &'static str {
        self.spec().default_extension
    }

    /// Returns the standardized, capitalized uppercase display name for the media format (e.g. "JPEG", "PNG", "MP4").
    #[inline]
    pub fn display_name(&self) -> &'static str {
        self.spec().name
    }

    /// Whether `RustView` knows this format at all (independent of the user's Settings; see `FormatPolicy` for that).
    #[inline]
    pub fn is_supported(&self) -> bool {
        !matches!(self, Self::Unknown)
    }

    /// Whether the user has this format switched on in Settings > File Formats.
    #[inline]
    pub fn is_enabled(&self) -> bool {
        FormatPolicy::is_enabled(*self)
    }
}

fn extension_map() -> &'static HashMap<&'static str, MediaFormat> {
    static MAP: OnceLock<HashMap<&'static str, MediaFormat>> = OnceLock::new();
    MAP.get_or_init(|| {
        FORMAT_SPECS
            .iter()
            .flat_map(|spec| spec.extensions.iter().map(move |ext| (*ext, spec.format)))
            .collect()
    })
}

/// Formats a byte size in Megabytes with 2 decimal places and no suffix (e.g. "310.01").
#[inline]
pub fn format_filesize_mb(bytes: u64) -> String {
    let mb = bytes.az::<f64>() / (1024.0 * 1024.0);
    format!("{mb:.2}")
}

/// Formats a video duration in milliseconds to "hh:mm:ss.ms" format (e.g. "00:00:05.332").
/// For static images or unspecified duration, returns "-".
pub fn format_duration_hms_ms(duration_ms: u64, is_video: bool) -> String {
    if !is_video || duration_ms == 0 {
        return "-".to_string();
    }
    let ms = duration_ms % 1000;
    let total_secs = duration_ms / 1000;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

/// Formats a video duration compactly for thumbnail badges. Hours are the largest unit (no days).
///
/// - below one second: seconds with up to two decimals, e.g. `"0.5s"` or `"0.25s"`
/// - below 60 seconds: `"5s"` (milliseconds are not shown from one second on)
/// - below one hour: `"04:30"` (mm:ss, the clock format is unambiguous without a unit)
/// - one hour or more: `"01:02:03"` (hh:mm:ss, hours grow beyond two digits if needed)
///
/// Returns an empty string for images and for videos without a known duration, so callers can
/// treat "empty" as "no badge".
pub fn format_duration_compact(duration_ms: u64, is_video: bool) -> String {
    if !is_video || duration_ms == 0 {
        return String::new();
    }
    if duration_ms < 1000 {
        return format_sub_second(duration_ms);
    }
    let total_secs = duration_ms / 1000;
    if total_secs < 60 {
        return format!("{total_secs}s");
    }
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    if h == 0 {
        format!("{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

/// Formats 1..=999 ms as fractional seconds rounded to centiseconds (at least `0.01s`).
fn format_sub_second(duration_ms: u64) -> String {
    let centis = ((duration_ms + 5) / 10).max(1);
    match centis {
        100.. => "1s".to_string(),
        c if c % 10 == 0 => format!("0.{}s", c / 10),
        c => format!("0.{c:02}s"),
    }
}

/// Approximates image/video dimensions to the closest standard Aspect Ratio bucket:
/// 1:1, 5:4, 4:3, 16:9, 4:5, 3:4, 9:16.
pub fn approximate_aspect_ratio(width: u32, height: u32) -> &'static str {
    const BUCKETS: [(&str, f64); 7] = [
        ("9:16", 9.0 / 16.0), // 0.5625
        ("3:4", 3.0 / 4.0),   // 0.7500
        ("4:5", 4.0 / 5.0),   // 0.8000
        ("1:1", 1.0),         // 1.0000
        ("5:4", 5.0 / 4.0),   // 1.2500
        ("4:3", 4.0 / 3.0),   // 1.3333
        ("16:9", 16.0 / 9.0), // 1.7778
    ];

    if width == 0 || height == 0 {
        return "-";
    }

    let q = f64::from(width) / f64::from(height);

    let mut best_label = "1:1";
    let mut min_diff = f64::MAX;

    for (label, target) in BUCKETS {
        let diff = (q - target).abs();
        if diff < min_diff {
            min_diff = diff;
            best_label = label;
        }
    }

    best_label
}

#[cfg(test)]
mod duration_compact_tests {
    use super::format_duration_compact;

    #[test]
    fn seconds_below_one_minute_use_unit() {
        assert_eq!(format_duration_compact(5_000, true), "5s");
        assert_eq!(format_duration_compact(59_999, true), "59s");
    }

    #[test]
    fn minutes_use_clock_format_without_unit() {
        assert_eq!(format_duration_compact(60_000, true), "01:00");
        assert_eq!(format_duration_compact(270_000, true), "04:30");
        assert_eq!(format_duration_compact(3_599_999, true), "59:59");
    }

    #[test]
    fn hours_use_three_segments_and_never_days() {
        assert_eq!(format_duration_compact(3_600_000, true), "01:00:00");
        assert_eq!(format_duration_compact(3_723_000, true), "01:02:03");
        assert_eq!(format_duration_compact(90_000_000, true), "25:00:00");
        assert_eq!(format_duration_compact(360_000_000, true), "100:00:00");
    }

    #[test]
    fn milliseconds_are_not_rendered_from_one_second_on() {
        assert_eq!(format_duration_compact(5_999, true), "5s");
        assert_eq!(format_duration_compact(61_999, true), "01:01");
    }

    #[test]
    fn sub_second_clips_show_fractional_seconds() {
        assert_eq!(format_duration_compact(500, true), "0.5s");
        assert_eq!(format_duration_compact(250, true), "0.25s");
        assert_eq!(format_duration_compact(40, true), "0.04s");
        assert_eq!(format_duration_compact(100, true), "0.1s");
    }

    #[test]
    fn sub_second_edge_cases_never_show_zero_or_overflow() {
        assert_eq!(format_duration_compact(1, true), "0.01s");
        assert_eq!(format_duration_compact(994, true), "0.99s");
        assert_eq!(format_duration_compact(995, true), "1s");
        assert_eq!(format_duration_compact(999, true), "1s");
        assert_eq!(format_duration_compact(1000, true), "1s");
    }

    #[test]
    fn unknown_duration_and_images_yield_empty_string() {
        assert_eq!(format_duration_compact(0, true), "");
        assert_eq!(format_duration_compact(5_000, false), "");
    }
}
