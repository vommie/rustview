//! Frame scaling (swscale) and rotation of decoded video frames.

use crate::thumbnails::error::ThumbnailError;
use crate::thumbnails::types::ScalingAlgorithm;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::context::Context as SwsContext;
use ffmpeg_next::software::scaling::flag::Flags as SwsFlags;

/// Maps `RustView` `ScalingAlgorithm` domain enum to native `FFmpeg` libswscale flags with accurate rounding.
#[inline]
pub fn map_scaling_algorithm_to_sws_flags(algo: ScalingAlgorithm) -> SwsFlags {
    match algo {
        ScalingAlgorithm::Nearest => SwsFlags::POINT,
        ScalingAlgorithm::Bilinear => SwsFlags::BILINEAR | SwsFlags::ACCURATE_RND,
        ScalingAlgorithm::CatmullRom => SwsFlags::BICUBIC | SwsFlags::ACCURATE_RND,
        ScalingAlgorithm::Lanczos3 => SwsFlags::LANCZOS | SwsFlags::ACCURATE_RND,
    }
}

/// Dynamic, self-healing Swscale context manager that transparently adapts to stream pixel format changes
/// and unpacks padded pitch/stride linesizes into contiguous linear RGB8 byte buffers.
pub struct DynamicScaler {
    scaler: Option<SwsContext>,
    src_fmt: Pixel,
    src_w: u32,
    src_h: u32,
    pub dst_w: u32,
    pub dst_h: u32,
    flags: SwsFlags,
}

impl DynamicScaler {
    pub fn new(dst_w: u32, dst_h: u32, flags: SwsFlags) -> Self {
        Self {
            scaler: None,
            src_fmt: Pixel::None,
            src_w: 0,
            src_h: 0,
            dst_w,
            dst_h,
            flags,
        }
    }

    /// Converts and scales a decoded video frame to tightly packed RGB24 bytes via `FFmpeg`'s native libswscale.
    pub fn scale_frame(&mut self, decoded: &ffmpeg_next::frame::Video) -> Result<Vec<u8>, ThumbnailError> {
        let fmt = decoded.format();
        let w = decoded.width();
        let h = decoded.height();

        if self.scaler.is_none() || self.src_fmt != fmt || self.src_w != w || self.src_h != h {
            let new_scaler =
                SwsContext::get(fmt, w, h, Pixel::RGB24, self.dst_w, self.dst_h, self.flags).map_err(|e| {
                    ThumbnailError::Ffmpeg(format!(
                        "Failed to initialize swscale context ({:?} {}x{} -> RGB24 {}x{}): {}",
                        fmt, w, h, self.dst_w, self.dst_h, e
                    ))
                })?;

            self.scaler = Some(new_scaler);
            self.src_fmt = fmt;
            self.src_w = w;
            self.src_h = h;
        }

        let scaler = self.scaler.as_mut().unwrap();
        let mut rgb_frame = ffmpeg_next::frame::Video::empty();
        scaler
            .run(decoded, &mut rgb_frame)
            .map_err(|e| ThumbnailError::Ffmpeg(format!("swscale conversion failed: {e}")))?;

        let actual_w = rgb_frame.width();
        let actual_h = rgb_frame.height();
        let stride = rgb_frame.stride(0);
        let row_bytes = (actual_w * 3) as usize;
        let plane_data = rgb_frame.data(0);

        if stride == row_bytes && plane_data.len() >= row_bytes * actual_h as usize {
            return Ok(plane_data[..row_bytes * actual_h as usize].to_vec());
        }

        let mut contiguous = Vec::with_capacity(row_bytes * actual_h as usize);
        for y in 0..actual_h as usize {
            let start = y * stride;
            let end = start + row_bytes;
            if end <= plane_data.len() {
                contiguous.extend_from_slice(&plane_data[start..end]);
            } else {
                return Err(ThumbnailError::Ffmpeg(format!(
                    "Incomplete video frame buffer: row {}/{} exceeded plane length (stride: {}, len: {})",
                    y,
                    actual_h,
                    stride,
                    plane_data.len()
                )));
            }
        }

        Ok(contiguous)
    }
}

/// Applies orientation rotation if container or stream tags require 90°, 180°, or 270° rotation.
pub fn apply_rotation_if_needed(pixels: Vec<u8>, width: u32, height: u32, rotation: i32) -> (u32, u32, Vec<u8>) {
    let norm_rot = rotation.rem_euclid(360);
    if norm_rot != 90 && norm_rot != 180 && norm_rot != 270 {
        return (width, height, pixels);
    }

    if pixels.len() != (width * height * 3) as usize {
        return (width, height, pixels);
    }

    let Some(img) = image::RgbImage::from_raw(width, height, pixels) else {
        return (width, height, Vec::new());
    };

    let rotated = match norm_rot {
        90 => image::imageops::rotate90(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate270(&img),
        _ => img,
    };
    let (new_w, new_h) = rotated.dimensions();
    (new_w, new_h, rotated.into_raw())
}
