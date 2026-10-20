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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::zoom::geometry::{Frame, IRect, Limits, RectF, ZoomView};

    /// A gradient: red grows with x, green with y, blue is a checker of 2x2 blocks.
    fn gradient(w: u32, h: u32) -> SourceImage {
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
        for (i, px) in buffer.make_mut_slice().iter_mut().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            *px = Rgb8Pixel {
                r: (x * 255 / (w - 1).max(1)) as u8,
                g: (y * 255 / (h - 1).max(1)) as u8,
                b: if (x / 2 + y / 2) % 2 == 0 { 255 } else { 0 },
            };
        }
        SourceImage::new(w, h, buffer).unwrap()
    }

    fn visible(dest: (i32, i32), src: (f64, f64, f64, f64)) -> Visible {
        Visible { dest: IRect { x: 0, y: 0, w: dest.0, h: dest.1 }, src: RectF::new(src.0, src.1, src.2, src.3) }
    }

    #[test]
    fn the_settings_names_round_trip_and_unknown_names_fall_back() {
        for algorithm in Resample::ALL {
            assert_eq!(Resample::from_setting(algorithm.as_setting()), algorithm);
        }
        assert_eq!(Resample::from_setting("whatever"), Resample::Lanczos3);
        assert_eq!(Resample::from_setting("  BICUBIC "), Resample::CatmullRom);
        assert!(Resample::Nearest.is_pixelated() && !Resample::Lanczos3.is_pixelated());
        assert_eq!(Resample::default(), Resample::Lanczos3);
    }

    #[test]
    fn a_source_must_match_its_buffer() {
        assert!(SourceImage::new(0, 4, SharedPixelBuffer::new(1, 1)).is_none());
        assert!(SourceImage::new(2, 2, SharedPixelBuffer::new(3, 3)).is_none());
        assert!(SourceImage::new(3, 3, SharedPixelBuffer::new(3, 3)).is_some());
    }

    #[test]
    fn nearest_at_one_to_one_reproduces_the_pixels() {
        let source = gradient(32, 24);
        let out = render_view(&source, &visible((32, 24), (0.0, 0.0, 32.0, 24.0)), Resample::Nearest).unwrap();
        assert_eq!((out.width(), out.height()), (32, 24));
        assert_eq!(out.as_bytes(), source.pixels.as_bytes());
    }

    #[test]
    fn a_crop_takes_exactly_the_requested_part() {
        let source = gradient(100, 100);
        // The window x 40..60, y 20..40 shown 1:1 (nearest): pixel (0,0) is source (40,20).
        let out = render_view(&source, &visible((20, 20), (40.0, 20.0, 20.0, 20.0)), Resample::Nearest).unwrap();
        let first = out.as_slice()[0];
        let expected = source.pixels.as_slice()[20 * 100 + 40];
        assert_eq!((first.r, first.g, first.b), (expected.r, expected.g, expected.b));
        let last = out.as_slice()[20 * 20 - 1];
        let expected = source.pixels.as_slice()[39 * 100 + 59];
        assert_eq!((last.r, last.g, last.b), (expected.r, expected.g, expected.b));
    }

    #[test]
    fn enlarging_keeps_the_gradient_and_every_filter_works_in_both_directions() {
        let source = gradient(64, 48);
        for algorithm in Resample::ALL {
            let up = render_view(&source, &visible((256, 192), (0.0, 0.0, 64.0, 48.0)), algorithm).unwrap();
            assert_eq!((up.width(), up.height()), (256, 192), "{algorithm:?}");
            // Red grows from left to right.
            let row = &up.as_slice()[100 * 256..101 * 256];
            assert!(row[200].r > row[20].r, "{algorithm:?}");
            let down = render_view(&source, &visible((16, 12), (0.0, 0.0, 64.0, 48.0)), algorithm).unwrap();
            assert_eq!((down.width(), down.height()), (16, 12), "{algorithm:?}");
        }
    }

    #[test]
    fn the_smooth_filters_average_when_shrinking_and_nearest_does_not() {
        // A fine checker (2x2 blocks) shrunk by 4: the smooth filters give mid grey blue, nearest gives a hard value.
        let source = gradient(64, 64);
        let smooth = render_view(&source, &visible((16, 16), (0.0, 0.0, 64.0, 64.0)), Resample::Lanczos3).unwrap();
        let blues: Vec<u8> = smooth.as_slice().iter().map(|p| p.b).collect();
        let mid = blues.iter().filter(|b| (60..200).contains(*b)).count();
        assert!(mid as f64 > blues.len() as f64 * 0.6, "averaged: {mid} of {}", blues.len());
        let hard = render_view(&source, &visible((16, 16), (0.0, 0.0, 64.0, 64.0)), Resample::Nearest).unwrap();
        assert!(hard.as_slice().iter().all(|p| p.b == 0 || p.b == 255));
    }

    #[test]
    fn a_crop_that_reaches_the_edge_of_the_image_is_accepted() {
        let source = gradient(50, 40);
        let out = render_view(&source, &visible((50, 40), (0.0, 0.0, 50.0 + 1e-10, 40.0 + 1e-10)), Resample::Lanczos3).unwrap();
        assert_eq!((out.width(), out.height()), (50, 40));
        let out = render_view(&source, &visible((10, 10), (45.0, 35.0, 5.0, 5.0)), Resample::CatmullRom).unwrap();
        assert_eq!((out.width(), out.height()), (10, 10));
    }

    #[test]
    fn the_whole_pipeline_from_a_view_to_pixels() {
        // A 400x300 image in a 100x75 tile, zoomed to 400% around the middle: the tile shows the central 100x75 source pixels.
        let source = gradient(400, 300);
        let frame = Frame::new((400, 300), (100, 75)).unwrap();
        let view = frame.zoom_to(ZoomView::FIT, 4.0, None, Limits::DEFAULT);
        let vis = frame.visible(view).unwrap();
        assert_eq!(vis.dest, IRect { x: 0, y: 0, w: 100, h: 75 });
        assert!((vis.src.w - 100.0).abs() < 1e-6 && (vis.src.x - 150.0).abs() < 1e-6);
        let out = render_view(&source, &vis, Resample::Nearest).unwrap();
        // The tile's first pixel is source (150, 112.5): its left neighbour column is red 150/399.
        let first = out.as_slice()[0];
        assert!((i32::from(first.r) - (150 * 255 / 399)).abs() <= 3, "{}", first.r);
    }

    #[test]
    fn thumbnails_fit_and_never_enlarge() {
        let source = gradient(400, 200);
        let small = thumbnail(&source.pixels, 100, 100).unwrap();
        assert_eq!((small.width(), small.height()), (100, 50));
        let same = thumbnail(&gradient(40, 20).pixels, 100, 100).unwrap();
        assert_eq!((same.width(), same.height()), (40, 20));
        assert!(thumbnail(&source.pixels, 0, 10).is_err());
        let thin = thumbnail(&gradient(1000, 3).pixels, 100, 100).unwrap();
        assert_eq!((thin.width(), thin.height()), (100, 1));
    }
}
