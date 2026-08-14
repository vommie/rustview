use super::error::ThumbnailError;
use super::types::{FitMode, THUMBNAIL_SIZE};
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::ImageReader;
use std::path::Path;

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
        let dynamic_img = ImageReader::open(path)?.with_guessed_format()?.decode()?;

        let orig_w = dynamic_img.width();
        let orig_h = dynamic_img.height();
        let rgb_src = dynamic_img.to_rgb8();

        let (target_w, target_h, offset_x, offset_y) =
            calculate_dimensions(orig_w, orig_h, THUMBNAIL_SIZE, THUMBNAIL_SIZE, fit_mode);

        let src_image = Image::from_vec_u8(orig_w, orig_h, rgb_src.into_raw(), PixelType::U8x3)?;

        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options =
            ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));

        resizer.resize(&src_image, &mut dst_image, &options)?;

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
