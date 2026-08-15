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
