use num_format::{Locale, SystemLocale, ToFormattedString};
pub use rustview_core::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};
use std::sync::LazyLock;

static SYSTEM_LOCALE: LazyLock<Option<SystemLocale>> = LazyLock::new(|| SystemLocale::default().ok());

pub fn format_filesize(size_bytes: u64) -> String {
    if size_bytes == 0 {
        return "0 B".to_string();
    }
    let units = ["B", "KB", "MB", "GB", "TB"];
    let i = (size_bytes as f64).log(1024.0).floor() as usize;
    let p = 1024_f64.powi(i as i32);
    let s = (size_bytes as f64) / p;
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

/// Formats a UNIX epoch timestamp (UTC) into a human-readable `YYYY-MM-DD HH:MM:SS` string; `-` for unset or invalid timestamps.
pub fn format_timestamp(timestamp_secs: i64) -> String {
    if timestamp_secs <= 0 {
        return "-".to_string();
    }
    chrono::DateTime::from_timestamp(timestamp_secs, 0)
        .map_or_else(|| "-".to_string(), |time| time.format("%Y-%m-%d %H:%M:%S").to_string())
}

/// Creates a thread-safe `SharedPixelBuffer` that implements `Send` + `Sync`.
pub fn rgb8_to_shared_buffer(raw_rgb: &[u8], width: u32, height: u32) -> SharedPixelBuffer<Rgb8Pixel> {
    let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
    pixel_buffer.make_mut_bytes().copy_from_slice(raw_rgb);
    pixel_buffer
}

/// Converts raw RGB8 slice directly into a Slint UI `Image`.
pub fn rgb8_to_slint_image(raw_rgb: &[u8], width: u32, height: u32) -> Image {
    let pixel_buffer = rgb8_to_shared_buffer(raw_rgb, width, height);
    Image::from_rgb8(pixel_buffer)
}

#[cfg(test)]
mod tests {
    use super::format_timestamp;

    #[test]
    fn unset_timestamps_show_a_dash() {
        assert_eq!(format_timestamp(0), "-");
        assert_eq!(format_timestamp(-5), "-");
    }

    #[test]
    fn timestamps_are_formatted_in_utc() {
        assert_eq!(format_timestamp(86_400 * 365), "1971-01-01 00:00:00");
        assert_eq!(format_timestamp(1_700_000_000), "2023-11-14 22:13:20");
        // A leap day and the end of a month.
        assert_eq!(format_timestamp(1_709_164_800), "2024-02-29 00:00:00");
    }
}
