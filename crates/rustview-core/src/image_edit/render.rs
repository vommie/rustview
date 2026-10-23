//! Turns the original file and the plan of the edits into the bytes of the new file (no drive access except the decoders that
//! read a path: SVG, `FFmpeg`). The picture is decoded again from the original at full fidelity (16 bits, transparency, gray);
//! the downscaled copy the viewer shows is never written.

use super::encode::encode_image;
use super::formats::WriteFormat;
use super::geometry::PixelRect;
use super::indexed::{crop_gif, crop_indexed_png, is_indexed_png};
use super::metadata::{ContainerParts, SourceMetadata, embed_metadata, patched_exif};
use super::options::Encoding;
use super::stack::EditPlan;
use crate::media::{MediaFormat, svg, tile_grid};
use bytes::Bytes;
use image::{DynamicImage, ImageFormat, ImageReader, RgbImage};
use img_parts::ImageEXIF;
use img_parts::jpeg::Jpeg;
use std::io::Cursor;
use std::path::Path;

/// Pictures larger than this many bytes of pixels are not decoded for editing (a protection against damaged headers).
const MAX_DECODE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub struct RenderInput<'a> {
    /// The original file.
    pub data: &'a [u8],
    /// Where it is (decoders that read a path: SVG, `FFmpeg`).
    pub path: &'a Path,
    /// The format its content has.
    pub format: MediaFormat,
    pub plan: EditPlan,
    /// The size of the picture the edits were made on.
    pub original_size: (u32, u32),
    pub metadata: &'a SourceMetadata,
}

#[derive(Debug)]
pub struct Rendered {
    pub bytes: Vec<u8>,
    pub size: (u32, u32),
}

/// The new file: `target` encoded with `encoding`.
pub fn render(input: &RenderInput<'_>, target: WriteFormat, encoding: Encoding) -> Result<Rendered, String> {
    let crop = input.plan.crop.unwrap_or_else(|| PixelRect::full(input.original_size));
    let size = crop.size();
    let in_place = target.media_format() == input.format;
    let bytes = match encoding {
        Encoding::JpegLossless { crop } => jpeg_lossless(input, crop)?,
        Encoding::JpegSameTables => jpeg_same_tables(input, crop)?,
        Encoding::Png { compression } if in_place && is_indexed_png(input.data) => {
            let png = crop_indexed_png(input.data, crop, compression)?;
            embed_metadata(png, target, input.metadata, size)?
        }
        Encoding::Gif if in_place => crop_gif(input.data, crop)?,
        _ => {
            let picture = decode_full(input)?;
            let cropped = picture.crop_imm(crop.x, crop.y, crop.width, crop.height);
            let metadata = if in_place {
                input.metadata.clone()
            } else {
                // Another format cannot take the original's container parts; the metadata itself goes over.
                SourceMetadata {
                    parts: ContainerParts::None,
                    ..input.metadata.clone()
                }
            };
            let encoded = encode_image(&cropped, encoding, &metadata)?;
            embed_metadata(encoded, target, &metadata, size)?
        }
    };
    Ok(Rendered { bytes, size })
}

/// Decodes the original at full fidelity and checks it is the picture that was edited (same size).
pub fn decode_full(input: &RenderInput<'_>) -> Result<DynamicImage, String> {
    let picture = decode_any(input.data, input.path, input.format)?;
    if (picture.width(), picture.height()) != input.original_size {
        return Err(format!(
            "the picture on disk is {} x {} px, but the edited one was {} x {} px",
            picture.width(),
            picture.height(),
            input.original_size.0,
            input.original_size.1
        ));
    }
    Ok(picture)
}

/// The same decoders as the viewer, in the same order, so the picture has the size the edits were made on: SVG through
/// resvg at the viewer's size, the `image` crate, composed HEIC/AVIF tile grids, then `FFmpeg`.
pub fn decode_any(data: &[u8], path: &Path, format: MediaFormat) -> Result<DynamicImage, String> {
    if format == MediaFormat::Svg {
        let (w, h) = svg::intrinsic_size(path).map_err(|e| e.to_string())?;
        let raster = svg::rasterize(path, svg::viewer_edge(w, h)).map_err(|e| e.to_string())?;
        return rgb_image(raster.width, raster.height, raster.rgb);
    }
    let mut reader = ImageReader::new(Cursor::new(data));
    if let Ok(by_extension) = ImageFormat::from_path(path) {
        reader.set_format(by_extension);
    }
    if let Ok(mut reader) = reader.with_guessed_format() {
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_DECODE_BYTES);
        reader.limits(limits);
        if let Ok(picture) = reader.decode() {
            return Ok(picture);
        }
    }
    if let Ok(Some(grid)) = tile_grid::decode(path)
        && !grid.rgb.is_empty()
    {
        return rgb_image(grid.width, grid.height, grid.rgb);
    }
    let frame = crate::thumbnails::video::FfmpegVideoThumbnailer::new()
        .extract_native_frame(path)
        .map_err(|e| e.to_string())?;
    rgb_image(frame.width, frame.height, frame.rgb_pixels)
}

fn rgb_image(width: u32, height: u32, rgb: Vec<u8>) -> Result<DynamicImage, String> {
    RgbImage::from_raw(width, height, rgb)
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| "the decoder returned a picture of the wrong size".to_string())
}

/// Cuts the blocks of `crop` out of the JPEG without decoding them; the markers of the original are copied by the transform
/// and its EXIF block is patched to the new size afterwards.
fn jpeg_lossless(input: &RenderInput<'_>, crop: PixelRect) -> Result<Vec<u8>, String> {
    let to_usize = |v: u32| usize::try_from(v).map_err(|e| e.to_string());
    let options = libjpeg_turbo_rs::TransformOptions {
        op: libjpeg_turbo_rs::TransformOp::None,
        crop: Some(libjpeg_turbo_rs::CropRegion {
            x: to_usize(crop.x)?,
            y: to_usize(crop.y)?,
            width: to_usize(crop.width)?,
            height: to_usize(crop.height)?,
        }),
        copy_markers: libjpeg_turbo_rs::MarkerCopyMode::All,
        ..Default::default()
    };
    let out = libjpeg_turbo_rs::transform_jpeg_with_options(input.data, &options).map_err(|e| e.to_string())?;
    let info = libjpeg_turbo_rs::probe(&out).map_err(|e| e.to_string())?;
    if (info.width, info.height) != (to_usize(crop.width)?, to_usize(crop.height)?) {
        return Err(format!(
            "the lossless crop gave {} x {} px instead of {} x {} px",
            info.width, info.height, crop.width, crop.height
        ));
    }
    let mut jpeg = Jpeg::from_bytes(Bytes::from(out)).map_err(|e| e.to_string())?;
    // The Multi-Picture index points at pictures that followed the old image data; the transform does not copy them.
    jpeg.segments_mut()
        .retain(|s| !(s.marker() == 0xE2 && s.contents().starts_with(b"MPF\0")));
    if jpeg.exif().is_some() {
        jpeg.set_exif(patched_exif(input.metadata, crop.size()).map(Bytes::from));
    }
    Ok(jpeg.encoder().bytes().to_vec())
}

/// Decodes the JPEG in its own color space, crops it and encodes it again with the original's quantization tables, sampling,
/// coding and restart interval: the quality the file had, with as little new loss as possible.
fn jpeg_same_tables(input: &RenderInput<'_>, crop: PixelRect) -> Result<Vec<u8>, String> {
    use libjpeg_turbo_rs::{Encoder, PixelFormat};

    let coefficients = libjpeg_turbo_rs::read_coefficients(input.data).map_err(|e| e.to_string())?;
    let header = libjpeg_turbo_rs::Decoder::new(input.data).map_err(|e| e.to_string())?;
    let progressive = header.header().is_progressive;
    let format = match coefficients.components.len() {
        1 => PixelFormat::Grayscale,
        4 => PixelFormat::Cmyk,
        _ => PixelFormat::Rgb,
    };
    let picture = libjpeg_turbo_rs::decompress_to(input.data, format).map_err(|e| e.to_string())?;
    let size = (
        u32::try_from(picture.width).map_err(|e| e.to_string())?,
        u32::try_from(picture.height).map_err(|e| e.to_string())?,
    );
    if size != input.original_size {
        return Err("the picture on disk does not have the size of the edited one".to_string());
    }
    let bpp = format.bytes_per_pixel();
    let to_usize = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
    let (x, y, w, h) = (
        to_usize(crop.x),
        to_usize(crop.y),
        to_usize(crop.width),
        to_usize(crop.height),
    );
    let mut pixels = Vec::with_capacity(w * h * bpp);
    for row in y..y + h {
        let start = (row * picture.width + x) * bpp;
        pixels.extend_from_slice(
            picture
                .data
                .get(start..start + w * bpp)
                .ok_or("crop outside the picture")?,
        );
    }
    let mut encoder = Encoder::new(&pixels, w, h, format)
        .progressive(progressive)
        .optimize_huffman(true)
        .density(
            coefficients.density_unit,
            coefficients.x_density,
            coefficients.y_density,
        );
    for (index, table) in coefficients.quant_tables.iter().enumerate().take(4) {
        encoder = encoder.quant_table(index, *table);
    }
    if format == PixelFormat::Rgb {
        encoder = encoder.sampling_factors(
            coefficients
                .components
                .iter()
                .map(|c| (c.h_sampling, c.v_sampling))
                .collect(),
        );
    }
    if coefficients.restart_interval > 0 {
        encoder = encoder.restart_blocks(coefficients.restart_interval);
    }
    let encoded = encoder.encode().map_err(|e| e.to_string())?;
    // The new stream writes its own Adobe marker (color transform) where it needs one.
    let metadata = match &input.metadata.parts {
        ContainerParts::Jpeg(parts) => SourceMetadata {
            parts: ContainerParts::Jpeg(parts.iter().filter(|(m, _)| *m != 0xEE).cloned().collect()),
            ..input.metadata.clone()
        },
        _ => input.metadata.clone(),
    };
    embed_metadata(encoded, WriteFormat::Jpeg, &metadata, crop.size())
}
