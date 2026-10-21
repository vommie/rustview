//! Producing the picture of a zoomed view (the resampling).
//!
//! `fast_image_resize` resamples a *fractional* crop of the original straight into the pixels of the tile (SIMD, no
//! intermediate copy of the crop), so the picture is made from the full-resolution original with the chosen filter whether it
//! is scaled up or down. The default is the best filter there is (Lanczos with three lobes) in both directions; the others are
//! offered for taste and speed.

use super::geometry::Visible;
use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{CropBoxError, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use slint::{Rgb8Pixel, SharedPixelBuffer};

/// The filter a zoomed picture is resampled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Resample {
    /// Lanczos, three lobes: the sharpest and most detailed result, a faint ringing at hard edges. The default.
    #[default]
    Lanczos3,
    /// Bicubic (Catmull-Rom): nearly as sharp, less ringing.
    CatmullRom,
    /// Bilinear: soft and fast.
    Bilinear,
    /// Nearest neighbour: every source pixel stays a sharp square, which is what one wants to see when pixel-peeping.
    Nearest,
}

impl Resample {
    pub const ALL: [Self; 4] = [Self::Lanczos3, Self::CatmullRom, Self::Bilinear, Self::Nearest];

    /// The name stored in the settings file (a hand-edited unknown value falls back to the default).
    pub fn from_setting(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "catmull_rom" | "bicubic" => Self::CatmullRom,
            "bilinear" => Self::Bilinear,
            "nearest" | "pixel" => Self::Nearest,
            _ => Self::Lanczos3,
        }
    }

    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Lanczos3 => "lanczos3",
            Self::CatmullRom => "catmull_rom",
            Self::Bilinear => "bilinear",
            Self::Nearest => "nearest",
        }
    }

    fn algorithm(self) -> ResizeAlg {
        match self {
            Self::Lanczos3 => ResizeAlg::Convolution(FilterType::Lanczos3),
            Self::CatmullRom => ResizeAlg::Convolution(FilterType::CatmullRom),
            Self::Bilinear => ResizeAlg::Convolution(FilterType::Bilinear),
            Self::Nearest => ResizeAlg::Nearest,
        }
    }

    /// The screen shows the not yet resampled preview with hard pixels as well (no smoothing by the renderer).
    pub fn is_pixelated(self) -> bool {
        self == Self::Nearest
    }
}

/// The original picture of an image, full resolution.
#[derive(Clone)]
pub struct SourceImage {
    pub width: u32,
    pub height: u32,
    pub pixels: SharedPixelBuffer<Rgb8Pixel>,
}

impl SourceImage {
    pub fn new(width: u32, height: u32, pixels: SharedPixelBuffer<Rgb8Pixel>) -> Option<Self> {
        (width > 0 && height > 0 && pixels.width() == width && pixels.height() == height).then_some(Self { width, height, pixels })
    }
}

fn crop_error(e: CropBoxError) -> String {
    format!("invalid crop: {e}")
}

/// Resamples the visible part of `source` into new pixels of the size of `visible.dest`.
pub fn render_view(source: &SourceImage, visible: &Visible, algorithm: Resample) -> Result<SharedPixelBuffer<Rgb8Pixel>, String> {
    let (dest_w, dest_h) = (visible.dest.w.max(1) as u32, visible.dest.h.max(1) as u32);
    let view = ImageRef::new(source.width, source.height, source.pixels.as_bytes(), PixelType::U8x3).map_err(|e| e.to_string())?;
    let mut out = SharedPixelBuffer::<Rgb8Pixel>::new(dest_w, dest_h);
    let mut target = Image::from_slice_u8(dest_w, dest_h, out.make_mut_bytes(), PixelType::U8x3).map_err(|e| e.to_string())?;
    let src = visible.src;
    // A hair of slack on the right and bottom edge (float noise must not reject a crop that ends exactly at the image).
    let width = src.w.min(f64::from(source.width) - src.x).max(1e-3);
    let height = src.h.min(f64::from(source.height) - src.y).max(1e-3);
    let options = ResizeOptions::new().resize_alg(algorithm.algorithm()).crop(src.x, src.y, width, height);
    Resizer::new().resize(&view, &mut target, &options).map_err(|e| match e {
        fast_image_resize::ResizeError::SrcCroppingError(c) => crop_error(c),
        other => other.to_string(),
    })?;
    Ok(out)
}

/// A small copy of `picture` that fits into `max_w` x `max_h` (never larger than the original): the thumbnail of the minimap.
pub fn thumbnail(picture: &SharedPixelBuffer<Rgb8Pixel>, max_w: u32, max_h: u32) -> Result<SharedPixelBuffer<Rgb8Pixel>, String> {
    let (w, h) = (picture.width(), picture.height());
    if w == 0 || h == 0 || max_w == 0 || max_h == 0 {
        return Err("empty picture".to_string());
    }
    let scale = (f64::from(max_w) / f64::from(w)).min(f64::from(max_h) / f64::from(h)).min(1.0);
    let (tw, th) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
    if (tw, th) == (w, h) {
        return Ok(picture.clone());
    }
    let view = ImageRef::new(w, h, picture.as_bytes(), PixelType::U8x3).map_err(|e| e.to_string())?;
    let mut out = SharedPixelBuffer::<Rgb8Pixel>::new(tw, th);
    let mut target = Image::from_slice_u8(tw, th, out.make_mut_bytes(), PixelType::U8x3).map_err(|e| e.to_string())?;
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    Resizer::new().resize(&view, &mut target, &options).map_err(|e| e.to_string())?;
    Ok(out)
}
