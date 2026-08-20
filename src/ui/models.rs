pub use crate::media::formats::{approximate_aspect_ratio, format_duration_hms_ms, format_filesize_mb};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};

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

/// Formats a UNIX epoch timestamp into human-readable `YYYY-MM-DD HH:MM:SS` string.
pub fn format_timestamp(timestamp_secs: i64) -> String {
    if timestamp_secs <= 0 {
        return "-".to_string();
    }
    let secs = timestamp_secs as u64;
    let sec = secs % 60;
    let mins = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;
    let days = (secs / 86400) as i64;

    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, hours, mins, sec)
}

/// Creates a thread-safe `SharedPixelBuffer` that implements `Send` + `Sync`.
pub fn rgb8_to_shared_buffer(
    raw_rgb: &[u8],
    width: u32,
    height: u32,
) -> SharedPixelBuffer<Rgb8Pixel> {
    let mut pixel_buffer = SharedPixelBuffer::<Rgb8Pixel>::new(width, height);
    pixel_buffer.make_mut_bytes().copy_from_slice(raw_rgb);
    pixel_buffer
}

/// Converts raw RGB8 slice directly into a Slint UI `Image`.
pub fn rgb8_to_slint_image(raw_rgb: &[u8], width: u32, height: u32) -> Image {
    let pixel_buffer = rgb8_to_shared_buffer(raw_rgb, width, height);
    Image::from_rgb8(pixel_buffer)
}
