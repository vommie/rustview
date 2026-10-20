//! Hover previews of animated pictures (GIF, APNG, animated WebP, AVIF sequences).
//!
//! Like the previews of videos, a few frames are sampled evenly once, while the thumbnail pipeline reads the file anyway, scaled
//! to thumbnail size, compressed and stored in the pack store (`VideoPreviewRecord`). The hover then reads only the pack store:
//! it never touches the picture file, and no animation is kept whole in the database. A record with no frames marks a picture
//! that was checked and is not animated, so it is not opened again.

use super::error::ThumbnailError;
use super::image::DecodedThumbnailFrame;
use super::types::{calculate_aspect_dimensions, ScalingAlgorithm};
use super::video::{apply_rotation_if_needed, map_scaling_algorithm_to_sws_flags, DynamicScaler};
use crate::media::metadata::ensure_ffmpeg_initialized;
use crate::media::{MediaDetector, MediaFormat};
use fast_image_resize::images::Image as FirImage;
use fast_image_resize::{PixelType, ResizeOptions, Resizer};
use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, Frame};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// More frames than this are not read: a long animation is sampled from its first part, and memory stays bounded.
pub const MAX_SOURCE_FRAMES: usize = 400;

/// Formats whose files can be animated (a PNG or WebP usually is not; the file tells).
pub fn can_animate(format: MediaFormat) -> bool {
    matches!(format, MediaFormat::Gif | MediaFormat::Png | MediaFormat::Webp | MediaFormat::Avif)
}

/// `count` indexes out of `len`, evenly spread and including the first and the last (fewer when `len` is smaller).
pub fn pick_evenly(len: usize, count: usize) -> Vec<usize> {
    let count = count.clamp(1, len.max(1));
    if len == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![0];
    }
    (0..count).map(|k| (k * (len - 1) + (count - 1) / 2) / (count - 1)).collect()
}

/// Samples up to `count` evenly spread frames of an animated picture, scaled to `max_bound`.
/// `Ok(None)` means the picture is not animated (or has a single frame): the caller stores a "checked, static" marker.
pub fn sample(
    path: &Path,
    max_bound: u32,
    scaling_algo: ScalingAlgorithm,
    count: usize,
) -> Result<Option<Vec<DecodedThumbnailFrame>>, ThumbnailError> {
    let header = MediaDetector::read_header(path).unwrap_or_default();
    let Some((format, _)) = MediaDetector::detect_from_bytes(&header) else { return Ok(None) };

    let frames = match format {
        MediaFormat::Gif => {
            let decoder = GifDecoder::new(BufReader::with_capacity(128 * 1024, File::open(path)?))?;
            scaled_frames(decoder.into_frames(), max_bound, scaling_algo)?
        }
        MediaFormat::Png => {
            let decoder = PngDecoder::new(BufReader::with_capacity(128 * 1024, File::open(path)?))?;
            if !decoder.is_apng()? {
                return Ok(None);
            }
            scaled_frames(decoder.apng()?.into_frames(), max_bound, scaling_algo)?
        }
        MediaFormat::Webp => {
            let decoder = WebPDecoder::new(BufReader::with_capacity(128 * 1024, File::open(path)?))?;
            if !decoder.has_animation() {
                return Ok(None);
            }
            scaled_frames(decoder.into_frames(), max_bound, scaling_algo)?
        }
        MediaFormat::Avif if has_sequence_brand(&header) => ffmpeg_sequence(path, max_bound, scaling_algo)?,
        _ => return Ok(None),
    };

    if frames.len() < 2 {
        return Ok(None);
    }
    let picked = pick_evenly(frames.len(), count);
    let mut frames: Vec<Option<DecodedThumbnailFrame>> = frames.into_iter().map(Some).collect();
    Ok(Some(picked.into_iter().filter_map(|i| frames[i].take()).collect()))
}

/// An AVIF image sequence has the `avis` brand; a still AVIF is not opened for animation at all.
fn has_sequence_brand(header: &[u8]) -> bool {
    header.get(4..8) == Some(b"ftyp") && header.windows(4).take(64).any(|w| w == b"avis")
}

/// Scales every frame the decoder yields (up to `MAX_SOURCE_FRAMES`) as it comes, so only thumbnail-sized pictures are held.
fn scaled_frames(
    frames: image::Frames<'_>,
    max_bound: u32,
    scaling_algo: ScalingAlgorithm,
) -> Result<Vec<DecodedThumbnailFrame>, ThumbnailError> {
    let mut resizer = Resizer::new();
    let options = ResizeOptions::default().resize_alg(scaling_algo.to_resize_alg());
    let mut out = Vec::new();
    for frame in frames.take(MAX_SOURCE_FRAMES) {
        match frame {
            Ok(frame) => out.push(scale_frame(frame, max_bound, &mut resizer, &options)?),
            // A damaged tail still leaves a usable preview of what was read
            Err(e) if out.len() >= 2 => {
                tracing::debug!(error = %e, frames = out.len(), "[AnimatedFrames] Stopped at a damaged frame");
                break;
            }
            Err(e) => return Err(ThumbnailError::Image(e)),
        }
    }
    Ok(out)
}

fn scale_frame(
    frame: Frame,
    max_bound: u32,
    resizer: &mut Resizer,
    options: &ResizeOptions,
) -> Result<DecodedThumbnailFrame, ThumbnailError> {
    let rgba = frame.into_buffer();
    let (src_w, src_h) = (rgba.width(), rgba.height());
    let (target_w, target_h) = calculate_aspect_dimensions(src_w, src_h, max_bound);
    let src = FirImage::from_vec_u8(src_w, src_h, blend_on_black(&rgba), PixelType::U8x3)?;
    let mut dst = FirImage::new(target_w, target_h, PixelType::U8x3);
    resizer.resize(&src, &mut dst, options)?;
    Ok(DecodedThumbnailFrame {
        width: target_w,
        height: target_h,
        rgb_pixels: dst.into_vec(),
        orig_width: src_w,
        orig_height: src_h,
    })
}

/// Opaque RGB with transparency blended against black, like the thumbnails of GIFs.
fn blend_on_black(rgba: &image::RgbaImage) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(rgba.width() as usize * rgba.height() as usize * 3);
    for pixel in rgba.pixels() {
        let [r, g, b, a] = pixel.0;
        if a == 255 {
            rgb.extend_from_slice(&[r, g, b]);
        } else {
            let alpha = u16::from(a);
            rgb.extend([r, g, b].map(|c| ((u16::from(c) * alpha + 127) / 255) as u8));
        }
    }
    rgb
}

/// The frames of the longest video stream of an AVIF sequence (the still image of the file is a stream of its own).
fn ffmpeg_sequence(
    path: &Path,
    max_bound: u32,
    scaling_algo: ScalingAlgorithm,
) -> Result<Vec<DecodedThumbnailFrame>, ThumbnailError> {
    let ffmpeg_err = |e: ffmpeg_next::Error| ThumbnailError::Ffmpeg(e.to_string());
    ensure_ffmpeg_initialized();
    let mut ictx = ffmpeg_next::format::input(path).map_err(ffmpeg_err)?;
    let (index, rotation) = {
        let stream = ictx
            .streams()
            .filter(|s| s.parameters().medium() == ffmpeg_next::media::Type::Video)
            .max_by_key(|s| s.frames())
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video stream found in container".into()))?;
        (stream.index(), crate::media::orientation::stream_rotation(&stream, &ictx.metadata()))
    };
    let context = ffmpeg_next::codec::context::Context::from_parameters(ictx.stream(index).map(|s| s.parameters()).ok_or_else(|| ThumbnailError::Ffmpeg("stream vanished".into()))?)
        .map_err(ffmpeg_err)?;
    let mut decoder = context.decoder().video().map_err(ffmpeg_err)?;
    let (w, h) = if crate::media::orientation::swaps_axes(rotation) { (decoder.height(), decoder.width()) } else { (decoder.width(), decoder.height()) };
    if w == 0 || h == 0 {
        return Err(ThumbnailError::Ffmpeg("Invalid picture dimensions (0x0)".into()));
    }
    let (target_w, target_h) = calculate_aspect_dimensions(w, h, max_bound);
    let (scaler_w, scaler_h) = if crate::media::orientation::swaps_axes(rotation) { (target_h, target_w) } else { (target_w, target_h) };
    let mut scaler = DynamicScaler::new(scaler_w, scaler_h, map_scaling_algorithm_to_sws_flags(scaling_algo));

    let mut out = Vec::new();
    let mut decoded = ffmpeg_next::frame::Video::empty();
    let mut take = |decoder: &mut ffmpeg_next::decoder::Video, out: &mut Vec<DecodedThumbnailFrame>| {
        while out.len() < MAX_SOURCE_FRAMES && decoder.receive_frame(&mut decoded).is_ok() {
            if let Ok(rgb) = scaler.scale_frame(&decoded) {
                let (fw, fh, pixels) = apply_rotation_if_needed(rgb, scaler_w, scaler_h, rotation);
                out.push(DecodedThumbnailFrame { width: fw, height: fh, rgb_pixels: pixels, orig_width: w, orig_height: h });
            }
        }
    };
    for (stream, packet) in ictx.packets() {
        if stream.index() != index {
            continue;
        }
        if decoder.send_packet(&packet).is_ok() {
            take(&mut decoder, &mut out);
        }
        if out.len() >= MAX_SOURCE_FRAMES {
            break;
        }
    }
    let _ = decoder.send_eof();
    take(&mut decoder, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, RgbaImage};

    #[test]
    fn frames_are_picked_evenly_including_first_and_last() {
        assert_eq!(pick_evenly(10, 5), vec![0, 2, 5, 7, 9]);
        assert_eq!(pick_evenly(3, 5), vec![0, 1, 2], "never more than there are");
        assert_eq!(pick_evenly(7, 1), vec![0]);
        assert_eq!(pick_evenly(2, 2), vec![0, 1]);
        assert!(pick_evenly(0, 5).is_empty());
    }

    #[test]
    fn only_animatable_formats_are_candidates() {
        assert!(can_animate(MediaFormat::Gif) && can_animate(MediaFormat::Webp) && can_animate(MediaFormat::Png) && can_animate(MediaFormat::Avif));
        assert!(!can_animate(MediaFormat::Jpeg) && !can_animate(MediaFormat::Mp4) && !can_animate(MediaFormat::Heic));
    }

    fn write_gif(path: &Path, frames: u8) {
        let file = File::create(path).unwrap();
        let mut encoder = GifEncoder::new(file);
        encoder.set_repeat(Repeat::Infinite).unwrap();
        for n in 0..frames {
            let img = RgbaImage::from_pixel(40, 30, image::Rgba([n * 20, 100, 200, 255]));
            encoder.encode_frame(Frame::from_parts(img, 0, 0, Delay::from_numer_denom_ms(100, 1))).unwrap();
        }
    }

    #[test]
    fn a_gif_is_sampled_down_to_the_wanted_count_at_thumbnail_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.gif");
        write_gif(&path, 12);
        let frames = sample(&path, 20, ScalingAlgorithm::Bilinear, 5).unwrap().expect("animated");
        assert_eq!(frames.len(), 5);
        assert_eq!((frames[0].width, frames[0].height), (20, 15));
        assert_eq!((frames[0].orig_width, frames[0].orig_height), (40, 30));
        let first_red = frames[0].rgb_pixels[0];
        let last_red = frames[4].rgb_pixels[0];
        assert!(last_red > first_red, "the last sampled frame is a later one");
    }

    #[test]
    fn a_single_frame_gif_and_a_png_are_static() {
        let dir = tempfile::tempdir().unwrap();
        let gif = dir.path().join("one.gif");
        write_gif(&gif, 1);
        assert!(sample(&gif, 20, ScalingAlgorithm::Bilinear, 5).unwrap().is_none());
        let png = dir.path().join("a.png");
        image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(&png).unwrap();
        assert!(sample(&png, 20, ScalingAlgorithm::Bilinear, 5).unwrap().is_none());
        let jpg = dir.path().join("a.jpg");
        image::RgbImage::from_pixel(8, 8, image::Rgb([1, 2, 3])).save(&jpg).unwrap();
        assert!(sample(&jpg, 20, ScalingAlgorithm::Bilinear, 5).unwrap().is_none());
    }

    #[test]
    fn transparency_is_blended_against_black() {
        let img = RgbaImage::from_pixel(1, 1, image::Rgba([200, 100, 50, 128]));
        assert_eq!(blend_on_black(&img), vec![100, 50, 25]);
    }
}
