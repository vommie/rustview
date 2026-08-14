use super::error::ThumbnailError;
use super::image::{BlitParams, blit_rgb, calculate_dimensions};
use super::types::{FitMode, THUMBNAIL_SIZE, anonymize_target};
use fast_image_resize::images::Image;
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};
use std::path::Path;
use std::time::Instant;

pub trait VideoFrameDecoder: Send + Sync {
    fn extract_and_resize(&self, path: &Path, fit_mode: FitMode)
    -> Result<Vec<u8>, ThumbnailError>;
}

#[derive(Default)]
pub struct FfmpegVideoThumbnailer;

impl FfmpegVideoThumbnailer {
    pub fn new() -> Self {
        Self
    }
}

impl VideoFrameDecoder for FfmpegVideoThumbnailer {
    fn extract_and_resize(
        &self,
        path: &Path,
        fit_mode: FitMode,
    ) -> Result<Vec<u8>, ThumbnailError> {
        let anon_target = anonymize_target(path, None);
        let t0 = Instant::now();

        ffmpeg_next::init().map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let mut ictx =
            ffmpeg_next::format::input(path).map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let stream = ictx
            .streams()
            .best(ffmpeg_next::media::Type::Video)
            .ok_or_else(|| ThumbnailError::Ffmpeg("No video stream found".into()))?;

        let video_stream_index = stream.index();
        let duration = stream.duration();

        let context_decoder =
            ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())
                .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;
        let mut decoder = context_decoder
            .decoder()
            .video()
            .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let width = decoder.width();
        let height = decoder.height();

        if width == 0 || height == 0 {
            return Err(ThumbnailError::Ffmpeg(
                "Invalid video dimensions (0x0)".into(),
            ));
        }

        let seek_target = if duration > 0 { duration / 10 } else { 0 };
        let _ = ictx.seek(seek_target, ..seek_target);

        let mut scaler = ffmpeg_next::software::scaling::context::Context::get(
            decoder.format(),
            width,
            height,
            ffmpeg_next::format::Pixel::RGB24,
            width,
            height,
            ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
        )
        .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

        let mut decoded_rgb: Option<Vec<u8>> = None;
        let mut decoded_frame = ffmpeg_next::frame::Video::empty();

        for (stream, packet) in ictx.packets() {
            if stream.index() == video_stream_index {
                decoder
                    .send_packet(&packet)
                    .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

                while decoder.receive_frame(&mut decoded_frame).is_ok() {
                    let mut rgb_frame = ffmpeg_next::frame::Video::empty();
                    scaler
                        .run(&decoded_frame, &mut rgb_frame)
                        .map_err(|e| ThumbnailError::Ffmpeg(e.to_string()))?;

                    let rgb_bytes = rgb_frame.data(0);
                    let required_len = (width * height * 3) as usize;
                    if rgb_bytes.len() >= required_len {
                        decoded_rgb = Some(rgb_bytes[..required_len].to_vec());
                        break;
                    }
                }

                if decoded_rgb.is_some() {
                    break;
                }
            }
        }

        let rgb_raw = decoded_rgb
            .ok_or_else(|| ThumbnailError::Ffmpeg("Could not decode any video frames".into()))?;

        let (target_w, target_h, offset_x, offset_y) =
            calculate_dimensions(width, height, THUMBNAIL_SIZE, THUMBNAIL_SIZE, fit_mode);

        let src_image = Image::from_vec_u8(width, height, rgb_raw, PixelType::U8x3)?;
        let mut dst_image = Image::new(target_w, target_h, PixelType::U8x3);
        let mut resizer = Resizer::new();
        let options =
            ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::CatmullRom));

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

        let total_elapsed = t0.elapsed();
        tracing::debug!(
            target = %anon_target,
            orig_w = width,
            orig_h = height,
            total_time_ms = total_elapsed.as_millis(),
            "[VideoDecoder: ffmpeg-next] Extracted and resized video frame to 256x256"
        );

        Ok(final_canvas)
    }
}
