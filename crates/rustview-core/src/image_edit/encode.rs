//! Encoding an edited picture (no drive access: pixels in, bytes out). Each format keeps what it can of the picture: bit depth,
//! transparency, gray. A transparent picture written as JPEG is put on white first (JPEG has no transparency).

use super::metadata::{SourceMetadata, patched_exif};
use super::options::{ChromaSubsampling, Encoding, PngCompression, TiffCompression};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageEncoder, ImageFormat, Rgb, RgbImage};
use std::io::Cursor;

pub fn encode_image(image: &DynamicImage, encoding: Encoding, meta: &SourceMetadata) -> Result<Vec<u8>, String> {
    match encoding {
        Encoding::Jpeg {
            quality,
            subsampling,
            progressive,
        } => encode_jpeg(image, quality, subsampling, progressive),
        Encoding::Png { compression } => encode_png(image, compression),
        Encoding::Webp { lossless, quality } => Ok(encode_webp(image, lossless, quality)),
        Encoding::Avif { quality, speed } => encode_avif(image, quality, speed, meta),
        Encoding::Tiff { compression } => encode_tiff(image, compression, meta),
        Encoding::Bmp => write_with(&bmp_compatible(image), ImageFormat::Bmp),
        Encoding::Gif => write_with(&DynamicImage::ImageRgba8(image.to_rgba8()), ImageFormat::Gif),
        Encoding::Qoi => write_with(&eight_bit(image), ImageFormat::Qoi),
        Encoding::Tga => write_with(&eight_bit(image), ImageFormat::Tga),
        Encoding::Farbfeld => write_with(&DynamicImage::ImageRgba16(image.to_rgba16()), ImageFormat::Farbfeld),
        Encoding::JpegLossless { .. } | Encoding::JpegSameTables => {
            Err("a JPEG crop of the original is encoded from the original file".to_string())
        }
    }
}

fn write_with(image: &DynamicImage, format: ImageFormat) -> Result<Vec<u8>, String> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn has_alpha(image: &DynamicImage) -> bool {
    image.color().has_alpha()
}

fn is_gray(image: &DynamicImage) -> bool {
    !image.color().has_color()
}

/// 8 bits per channel, keeping gray, color and transparency apart.
fn eight_bit(image: &DynamicImage) -> DynamicImage {
    match (has_alpha(image), is_gray(image)) {
        (false, true) => DynamicImage::ImageLuma8(image.to_luma8()),
        (true, true) => DynamicImage::ImageLumaA8(image.to_luma_alpha8()),
        (false, false) => DynamicImage::ImageRgb8(image.to_rgb8()),
        (true, false) => DynamicImage::ImageRgba8(image.to_rgba8()),
    }
}

fn bmp_compatible(image: &DynamicImage) -> DynamicImage {
    if has_alpha(image) {
        DynamicImage::ImageRgba8(image.to_rgba8())
    } else if is_gray(image) {
        DynamicImage::ImageLuma8(image.to_luma8())
    } else {
        DynamicImage::ImageRgb8(image.to_rgb8())
    }
}

/// The picture on a white background (what a JPEG of a transparent picture shows).
pub fn flatten_on_white(image: &DynamicImage) -> RgbImage {
    if !has_alpha(image) {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let a = u32::from(p[3]);
        let mix = |c: u8| u8::try_from((u32::from(c) * a + 255 * (255 - a) + 127) / 255).unwrap_or(u8::MAX);
        Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    })
}

pub fn jpeg_subsampling(subsampling: ChromaSubsampling) -> libjpeg_turbo_rs::Subsampling {
    match subsampling {
        ChromaSubsampling::S420 => libjpeg_turbo_rs::Subsampling::S420,
        ChromaSubsampling::S422 => libjpeg_turbo_rs::Subsampling::S422,
        ChromaSubsampling::S444 => libjpeg_turbo_rs::Subsampling::S444,
    }
}

fn encode_jpeg(
    image: &DynamicImage,
    quality: u8,
    subsampling: ChromaSubsampling,
    progressive: bool,
) -> Result<Vec<u8>, String> {
    let (w, h) = (
        usize::try_from(image.width()).map_err(|e| e.to_string())?,
        usize::try_from(image.height()).map_err(|e| e.to_string())?,
    );
    let gray;
    let rgb;
    let (pixels, format) = if is_gray(image) && !has_alpha(image) {
        gray = image.to_luma8();
        (gray.as_raw().as_slice(), libjpeg_turbo_rs::PixelFormat::Grayscale)
    } else {
        rgb = flatten_on_white(image);
        (rgb.as_raw().as_slice(), libjpeg_turbo_rs::PixelFormat::Rgb)
    };
    let mut encoder = libjpeg_turbo_rs::Encoder::new(pixels, w, h, format)
        .quality(quality)
        .progressive(progressive)
        .optimize_huffman(true);
    if format == libjpeg_turbo_rs::PixelFormat::Rgb {
        encoder = encoder.subsampling(jpeg_subsampling(subsampling));
    }
    encoder.encode().map_err(|e| e.to_string())
}

fn encode_png(image: &DynamicImage, compression: PngCompression) -> Result<Vec<u8>, String> {
    let image = match image {
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => DynamicImage::ImageRgba16(image.to_rgba16()),
        other => other.clone(),
    };
    let compression = match compression {
        PngCompression::Fast => CompressionType::Fast,
        PngCompression::Balanced => CompressionType::Default,
        PngCompression::Best => CompressionType::Best,
    };
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, compression, FilterType::Adaptive)
        .write_image(image.as_bytes(), image.width(), image.height(), image.color().into())
        .map_err(|e| e.to_string())?;
    Ok(out)
}

fn encode_webp(image: &DynamicImage, lossless: bool, quality: u8) -> Vec<u8> {
    let (w, h) = (image.width(), image.height());
    let rgba;
    let rgb;
    let encoder = if has_alpha(image) {
        rgba = image.to_rgba8();
        webp::Encoder::from_rgba(rgba.as_raw(), w, h)
    } else {
        rgb = image.to_rgb8();
        webp::Encoder::from_rgb(rgb.as_raw(), w, h)
    };
    let memory = if lossless {
        encoder.encode_lossless()
    } else {
        encoder.encode(f32::from(quality))
    };
    memory.to_vec()
}

fn encode_avif(image: &DynamicImage, quality: u8, speed: u8, meta: &SourceMetadata) -> Result<Vec<u8>, String> {
    let (w, h) = (
        usize::try_from(image.width()).map_err(|e| e.to_string())?,
        usize::try_from(image.height()).map_err(|e| e.to_string())?,
    );
    let mut encoder = ravif::Encoder::new()
        .with_quality(f32::from(quality))
        .with_alpha_quality(f32::from(quality))
        .with_speed(speed);
    let exif = patched_exif(meta, (image.width(), image.height()));
    if let Some(exif) = exif {
        encoder = encoder.with_exif(exif);
    }
    let encoded = if has_alpha(image) {
        let pixels: Vec<ravif::RGBA8> = image
            .to_rgba8()
            .pixels()
            .map(|p| ravif::RGBA8::new(p[0], p[1], p[2], p[3]))
            .collect();
        encoder.encode_rgba(ravif::Img::new(pixels.as_slice(), w, h))
    } else {
        let pixels: Vec<ravif::RGB8> = image
            .to_rgb8()
            .pixels()
            .map(|p| ravif::RGB8::new(p[0], p[1], p[2]))
            .collect();
        encoder.encode_rgb(ravif::Img::new(pixels.as_slice(), w, h))
    };
    encoded.map(|e| e.avif_file).map_err(|e| e.to_string())
}

fn encode_tiff(image: &DynamicImage, compression: TiffCompression, meta: &SourceMetadata) -> Result<Vec<u8>, String> {
    use tiff::encoder::{Compression, TiffEncoder, colortype};
    use tiff::tags::Tag;

    let mut out = Cursor::new(Vec::new());
    let mut encoder = TiffEncoder::new(&mut out)
        .map_err(|e| e.to_string())?
        .with_compression(match compression {
            TiffCompression::None => Compression::Uncompressed,
            TiffCompression::Lzw => Compression::Lzw,
            TiffCompression::Deflate => Compression::Deflate(tiff::encoder::DeflateLevel::Balanced),
        });
    let (w, h) = (image.width(), image.height());
    macro_rules! write_tiff {
        ($color:ty, $data:expr) => {{
            let mut img = encoder.new_image::<$color>(w, h).map_err(|e| e.to_string())?;
            if let Some(icc) = &meta.icc {
                img.encoder()
                    .write_tag(Tag::IccProfile, icc.as_slice())
                    .map_err(|e| e.to_string())?;
            }
            if let Some(xmp) = &meta.xmp {
                img.encoder()
                    .write_tag(Tag::Unknown(700), xmp.as_slice())
                    .map_err(|e| e.to_string())?;
            }
            img.write_data($data).map_err(|e| e.to_string())?;
        }};
    }
    let sixteen = matches!(
        image,
        DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageLumaA16(_)
            | DynamicImage::ImageRgb16(_)
            | DynamicImage::ImageRgba16(_)
            | DynamicImage::ImageRgb32F(_)
            | DynamicImage::ImageRgba32F(_)
    );
    match (sixteen, is_gray(image), has_alpha(image)) {
        (false, true, false) => write_tiff!(colortype::Gray8, image.to_luma8().as_raw()),
        (false, false, false) => write_tiff!(colortype::RGB8, image.to_rgb8().as_raw()),
        (false, _, true) => write_tiff!(colortype::RGBA8, image.to_rgba8().as_raw()),
        (true, true, false) => write_tiff!(colortype::Gray16, image.to_luma16().as_raw()),
        (true, false, false) => write_tiff!(colortype::RGB16, image.to_rgb16().as_raw()),
        (true, _, true) => write_tiff!(colortype::RGBA16, image.to_rgba16().as_raw()),
    }
    Ok(out.into_inner())
}
