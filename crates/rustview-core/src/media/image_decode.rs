//! Shared start of every in-process image decode (thumbnails, viewer, metadata).

use image::{ImageFormat, ImageReader};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// Opens an image for the `image` crate. The extension names the format first (TGA has no signature, so content alone
/// cannot find it), then a recognized signature takes over: a PNG called `.jpg` is decoded as a PNG.
pub fn open_reader(path: &Path, buffer_bytes: usize) -> std::io::Result<ImageReader<BufReader<File>>> {
    let mut reader = ImageReader::new(BufReader::with_capacity(buffer_bytes, File::open(path)?));
    if let Ok(format) = ImageFormat::from_path(path) {
        reader.set_format(format);
    }
    reader.with_guessed_format()
}
