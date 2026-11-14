use num_format::{Locale, SystemLocale, ToFormattedString};
pub use rustview_core::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};

use rustview_core::thumbnails::{CachedThumbnail, ThumbnailPixels};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};
use std::sync::LazyLock;

static SYSTEM_LOCALE: LazyLock<Option<SystemLocale>> = LazyLock::new(|| SystemLocale::default().ok());

pub fn format_filesize(size_bytes: u64) -> String {
    if size_bytes == 0 {
        return "0 B".to_string();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let i = (size_bytes.az::<f64>().log(1024.0).floor()).to_int::<usize>();
    let p = 1024_f64.powi(i.saturating_as::<i32>());
    let s = size_bytes.az::<f64>() / p;
    format!("{:.2} {}", s, units[i.min(units.len() - 1)])
}

/// Formats a count with the thousands grouping of the system locale.
pub fn format_count_with_locale(count: u64) -> String {
    match *SYSTEM_LOCALE {
        Some(ref locale) => count.to_formatted_string(locale),
        None => count.to_formatted_string(&Locale::en),
    }
}

/// Formats exact byte count with locale-sensitive thousands grouping separators using the `num-format` crate.
pub fn format_bytes_with_locale(bytes: u64) -> String {
    let formatted_number = if let Some(ref locale) = *SYSTEM_LOCALE {
        bytes.to_formatted_string(locale)
    } else {
        bytes.to_formatted_string(&Locale::en)
    };

    if bytes == 1 {
        format!("{formatted_number} Byte")
    } else {
        format!("{formatted_number} Bytes")
    }
}

/// Formats a UNIX epoch timestamp in the local time zone as `YYYY-MM-DD HH:MM:SS` (like the file manager shows it); `-` for unset or
/// invalid timestamps.
pub fn format_timestamp(timestamp_secs: i64) -> String {
    format_timestamp_in(timestamp_secs, &chrono::Local)
}

fn format_timestamp_in<Tz: chrono::TimeZone>(timestamp_secs: i64, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    if timestamp_secs <= 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp(timestamp_secs, 0).map_or_else(
        || "-".to_string(),
        |time| time.with_timezone(zone).format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// The picture of a decoded thumbnail. The pixels are opaque RGBA, which the renderer draws as they are (an RGB buffer would
/// be converted pixel by pixel on the UI thread when it is first drawn).
pub fn thumbnail_image(pixels: ThumbnailPixels) -> Image {
    Image::from_rgba8_premultiplied(pixels)
}

/// Converts raw RGB8 into a Slint UI `Image`, expanded to opaque RGBA here (see `thumbnail_image`).
pub fn rgb8_to_slint_image(raw_rgb: &[u8], width: u32, height: u32) -> Image {
    thumbnail_image(CachedThumbnail::from_rgb8(width, height, raw_rgb).pixels)
}

/// The pixels of a thumbnail as RGB, for code that works on RGB pictures (a Multiview tile's placeholder).
pub fn thumbnail_rgb(pixels: &ThumbnailPixels) -> SharedPixelBuffer<Rgb8Pixel> {
    let mut rgb = SharedPixelBuffer::<Rgb8Pixel>::new(pixels.width(), pixels.height());
    let (target, _) = rgb.make_mut_bytes().as_chunks_mut::<3>();
    let (source, _) = pixels.as_bytes().as_chunks::<4>();
    for (target, source) in target.iter_mut().zip(source) {
        *target = [source[0], source[1], source[2]];
    }
    rgb
}

#[cfg(test)]
mod tests {
    use super::{format_timestamp, format_timestamp_in};
    use chrono::{FixedOffset, Utc};

    #[test]
    fn unset_timestamps_show_a_dash() {
        assert_eq!(format_timestamp(0), "-");
        assert_eq!(format_timestamp(-5), "-");
    }

    #[test]
    fn timestamps_are_formatted_in_the_given_zone() {
        assert_eq!(format_timestamp_in(86_400 * 365, &Utc), "1971-01-01 00:00:00");
        assert_eq!(format_timestamp_in(1_700_000_000, &Utc), "2023-11-14 22:13:20");
        // A leap day and the end of a month.
        assert_eq!(format_timestamp_in(1_709_164_800, &Utc), "2024-02-29 00:00:00");
        // The same instant in UTC+2 (Central European Summer Time) and UTC-5 crosses the date line differently.
        let plus_two = FixedOffset::east_opt(2 * 3600).expect("valid offset");
        let minus_five = FixedOffset::west_opt(5 * 3600).expect("valid offset");
        assert_eq!(format_timestamp_in(1_709_164_800, &plus_two), "2024-02-29 02:00:00");
        assert_eq!(format_timestamp_in(1_709_164_800, &minus_five), "2024-02-28 19:00:00");
    }

    #[test]
    fn the_local_zone_is_applied() {
        // Whatever the zone of the machine is: the local rendering parses back to the same instant.
        let text = format_timestamp(1_700_000_000);
        let parsed = chrono::NaiveDateTime::parse_from_str(&text, "%Y-%m-%d %H:%M:%S").expect("a formatted date");
        let local = chrono::TimeZone::from_local_datetime(&chrono::Local, &parsed)
            .earliest()
            .expect("a valid local time");
        assert_eq!(local.timestamp(), 1_700_000_000);
    }
}
