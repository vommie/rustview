use super::error::ThumbnailError;
use super::types::{FitMode, THUMBNAIL_SIZE, anonymize_target};
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::ImageReader;
use std::io::BufReader;
use std::path::Path;
use std::time::Instant;

pub trait ImageDecoder: Send + Sync {
    fn decode_and_resize(&self, path: &Path, fit_mode: FitMode) -> Result<Vec<u8>, ThumbnailError>;
}

#[derive(Default)]
pub struct SimdImageThumbnailer;

impl SimdImageThumbnailer {
    pub fn new() -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BlitParams {
    pub src_w: u32,
    pub src_h: u32,
    pub dst_w: u32,
    pub offset_x: u32,
    pub offset_y: u32,
}

impl ImageDecoder for SimdImageThumbnailer {
    fn decode_and_resize(&self, path: &Path, fit_mode: FitMode) -> Result<Vec<u8>, ThumbnailError> {
        let anon_target = anonymize_target(path, None);
        let t0 = Instant::now();

        // 1. Buffered I/O (128 KiB buffer) eliminates syscall bottlenecks
        let file = std::fs::File::open(path)?;
        let reader = BufReader::with_capacity(128 * 1024, file);
        let dynamic_img = ImageReader::new(reader).with_guessed_format()?.decode()?;

        let decode_elapsed = t0.elapsed();
        let orig_w = dynamic_img.width();
        let orig_h = dynamic_img.height();
        let color_type = dynamic_img.color();

        tracing::debug!(
            target = %anon_target,
            orig_w = orig_w,
            orig_h = orig_h,
            color = ?color_type,
            decode_time_ms = decode_elapsed.as_millis(),
            "[ImageDecoder: image-rs/zune-jpeg] Decoded source image frame"
        );

        let t1 = Instant::now();
        let rgb_src = dynamic_img.to_rgb8();

        let (target_w, target_h, offset_x, offset_y) =
            calculate_dimensions(orig_w, orig_h, THUMBNAIL_SIZE, THUMBNAIL_SIZE, fit_mode);

        // Fast Box/Nearest Pre-downsampling for large photos (> 1024px)
        // Reduces 12MP/24MP convolutions to a fractional 8ms operation
        let (src_w, src_h, src_raw) = if orig_w > 1024 || orig_h > 1024 {
            let scale_factor = ((orig_w / 512).max(orig_h / 512)).max(1);
            if scale_factor > 1 {
                let intermediate_w = orig_w / scale_factor;
                let intermediate_h = orig_h / scale_factor;
                let mut downsampled =
                    Vec::with_capacity((intermediate_w * intermediate_h * 3) as usize);

                for y in 0..intermediate_h {
                    let src_y = y * scale_factor;
                    for x in 0..intermediate_w {
                        let src_x = x * scale_factor;
                        let pixel = rgb_src.get_pixel(src_x, src_y);
                        downsampled.extend_from_slice(&pixel.0);
                    }
                }
                (intermediate_w, intermediate_h, downsampled)
            } else {
                (orig_w, orig_h, rgb_src.into_raw())
            }
        } else {
            (orig_w, orig_h, rgb_src.into_raw())
        };

        let src_image = Image::from_vec_u8(src_w, src_h, src_raw, PixelType::U8x3)?;
        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options =
            ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));

        resizer.resize(&src_image, &mut dst_image, &options)?;
        let resize_elapsed = t1.elapsed();

        let resized_raw = dst_image.into_vec();
        let mut final_canvas = vec![0u8; (THUMBNAIL_SIZE * THUMBNAIL_SIZE * 3) as usize];
        blit_rgb(
            &resized_raw,
            &mut final_canvas,
            BlitParams {
                src_w: target_w,
                src_h: target_h,
                dst_w: THUMBNAIL_SIZE,
                offset_x,
                offset_y,
            },
        );

        let total_elapsed = t0.elapsed();
        tracing::debug!(
            target = %anon_target,
            target_w = target_w,
            target_h = target_h,
            resize_time_ms = resize_elapsed.as_millis(),
            total_time_ms = total_elapsed.as_millis(),
            "[SIMD Resizer: fast_image_resize (CatmullRom)] Generated 256x256 RGB8 canvas"
        );

        Ok(final_canvas)
    }
}

pub fn calculate_dimensions(
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    fit_mode: FitMode,
) -> (u32, u32, u32, u32) {
    let scale_x = dst_w as f32 / src_w as f32;
    let scale_y = dst_h as f32 / src_h as f32;

    let scale = match fit_mode {
        FitMode::Contain => scale_x.min(scale_y),
        FitMode::Cover => scale_x.max(scale_y),
    };

    let target_w = ((src_w as f32 * scale).round() as u32).clamp(1, dst_w);
    let target_h = ((src_h as f32 * scale).round() as u32).clamp(1, dst_h);

    let offset_x = (dst_w.saturating_sub(target_w)) / 2;
    let offset_y = (dst_h.saturating_sub(target_h)) / 2;

    (target_w, target_h, offset_x, offset_y)
}

pub fn blit_rgb(src: &[u8], dst: &mut [u8], params: BlitParams) {
    let src_stride = (params.src_w * 3) as usize;
    let dst_stride = (params.dst_w * 3) as usize;

    for y in 0..params.src_h {
        let src_start = (y as usize) * src_stride;
        let src_end = src_start + src_stride;

        let dst_start =
            ((y + params.offset_y) as usize * dst_stride) + (params.offset_x as usize * 3);
        let dst_end = dst_start + src_stride;

        if dst_end <= dst.len() && src_end <= src.len() {
            dst[dst_start..dst_end].copy_from_slice(&src[src_start..src_end]);
        }
    }
}
