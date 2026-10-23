//! Decoding of image tiles: the regular full-resolution decoder, then a downscale to at most the screen size so that up
//! to four tiles never hold four full-resolution buffers.

use crate::viewer::HighResImageLoader;
use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::Path;

/// Pixel size after fitting `width x height` into `max_w x max_h` without ever scaling up.
pub fn downscaled_size(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if width == 0 || height == 0 || (width <= max_w && height <= max_h) {
        return (width, height);
    }
    let scale = (f64::from(max_w) / f64::from(width)).min(f64::from(max_h) / f64::from(height));
    (
        ((f64::from(width) * scale).round() as u32).max(1),
        ((f64::from(height) * scale).round() as u32).max(1),
    )
}

/// Returns the original pixel size (for the aspect ratio) and the possibly downscaled pixels.
pub fn load_scaled(
    path: &Path,
    max_w: u32,
    max_h: u32,
) -> Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), String> {
    let (width, height, pixels) =
        HighResImageLoader::load_full_resolution(path).map_err(|e| e.to_string())?;
    let (target_w, target_h) = downscaled_size(width, height, max_w, max_h);
    if (target_w, target_h) == (width, height) {
        return Ok((width, height, pixels));
    }

    let source = Image::from_vec_u8(width, height, pixels.as_bytes().to_vec(), PixelType::U8x3)
        .map_err(|e| e.to_string())?;
    let mut target = Image::new(target_w, target_h, PixelType::U8x3);
    Resizer::new()
        .resize(&source, &mut target, &ResizeOptions::default())
        .map_err(|e| e.to_string())?;

    let mut scaled = SharedPixelBuffer::<Rgb8Pixel>::new(target_w, target_h);
    scaled.make_mut_bytes().copy_from_slice(target.buffer());
    Ok((width, height, scaled))
}
