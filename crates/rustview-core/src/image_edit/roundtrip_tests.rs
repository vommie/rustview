//! Saving real files of every writable format and reading them back: pixels, sizes, metadata, and that a failed save never
//! changes the original.

use super::conflict::{DiskState, FileIdentity};
use super::exif_patch::{read_sizes, tests::sample_exif, thumbnail_link};
use super::formats::WriteFormat;
use super::geometry::PixelRect;
use super::metadata::read_metadata;
use super::options::{EncodeOptions, Encoding, OverwriteMode};
use super::probe::{SourceProbe, probe};
use super::save::{EditedPicture, ExportTarget, SaveError, export, save_in_place};
use super::stack::EditPlan;
use super::writer::{Expect, WriteError, backup_path, overwrite_using};
use crate::media::MediaFormat;
use crate::util::cast::FloatToInt;
use image::{DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage};
use std::path::{Path, PathBuf};

const W: u32 = 97;
const H: u32 = 61;

/// A picture with detail everywhere (so a shifted crop would be seen) and varying transparency.
fn pattern() -> RgbaImage {
    RgbaImage::from_fn(W, H, |x, y| {
        let v = |k: u32| u8::try_from((x * k + y * (k + 3) + (x * y) % 17) % 256).unwrap_or(0);
        Rgba([v(7), v(13), v(29), u8::try_from(128 + (x + y) % 128).unwrap_or(255)])
    })
}

fn opaque() -> DynamicImage {
    let mut p = pattern();
    for px in p.pixels_mut() {
        px.0[3] = 255;
    }
    DynamicImage::ImageRgba8(p).to_rgb8().into()
}

/// A photo-like picture (smooth gradients and a few edges) for the lossy formats.
fn smooth() -> DynamicImage {
    let f = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round().to_int::<u8>();
    DynamicImage::ImageRgb8(image::RgbImage::from_fn(W, H, |x, y| {
        let (fx, fy) = (f64::from(x) / f64::from(W), f64::from(y) / f64::from(H));
        let edge = if (x / 24 + y / 20) % 2 == 0 { 0.15 } else { 0.0 };
        image::Rgb([f(fx * 0.8 + edge), f(fy * 0.7 + 0.1), f((fx + fy) * 0.4 + edge)])
    }))
}

fn decode_by_content(path: &Path) -> DynamicImage {
    image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .expect("open")
        .decode()
        .expect("decode")
}

fn write_source(dir: &Path, name: &str, image: &DynamicImage, format: ImageFormat) -> PathBuf {
    let path = dir.join(name);
    image.save_with_format(&path, format).expect("write the source");
    path
}

fn plan(crop: PixelRect) -> EditPlan {
    EditPlan {
        crop: Some(crop),
        output_size: crop.size(),
    }
}

fn picture<'a>(path: &'a Path, format: MediaFormat, probe: &'a SourceProbe, crop: PixelRect) -> EditedPicture<'a> {
    let size = image::ImageReader::open(path)
        .and_then(image::ImageReader::with_guessed_format)
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .unwrap_or((0, 0));
    EditedPicture {
        path,
        format,
        probe,
        plan: plan(crop),
        original_size: size,
        identity: FileIdentity::read(path).ok(),
    }
}

fn save(path: &Path, format: MediaFormat, crop: PixelRect) -> Result<super::save::SaveOutcome, SaveError> {
    let probe = probe(path, format).expect("probe");
    save_in_place(
        &picture(path, format, &probe, crop),
        &EncodeOptions::default(),
        OverwriteMode::Safe,
        false,
    )
}

fn psnr(a: &DynamicImage, b: &DynamicImage) -> f64 {
    let (a, b) = (a.to_rgb8(), b.to_rgb8());
    let mse = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| (f64::from(*x) - f64::from(*y)).powi(2))
        .sum::<f64>()
        / f64::from(u32::try_from(a.as_raw().len()).unwrap_or(1));
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64.powi(2) / mse).log10()
    }
}

#[test]
fn lossless_formats_keep_every_pixel_of_the_crop() {
    let dir = tempfile::tempdir().expect("temp dir");
    let crop = PixelRect::new(13, 7, 51, 33);
    let rgba = DynamicImage::ImageRgba8(pattern());
    let cases: Vec<(&str, DynamicImage, ImageFormat, MediaFormat)> = vec![
        ("rgba.png", rgba.clone(), ImageFormat::Png, MediaFormat::Png),
        ("rgb.png", opaque(), ImageFormat::Png, MediaFormat::Png),
        (
            "gray16.png",
            DynamicImage::ImageLuma16(rgba.to_luma16()),
            ImageFormat::Png,
            MediaFormat::Png,
        ),
        (
            "rgba16.png",
            DynamicImage::ImageRgba16(rgba.to_rgba16()),
            ImageFormat::Png,
            MediaFormat::Png,
        ),
        ("rgb.bmp", opaque(), ImageFormat::Bmp, MediaFormat::Bmp),
        ("rgba.bmp", rgba.clone(), ImageFormat::Bmp, MediaFormat::Bmp),
        ("rgba.tga", rgba.clone(), ImageFormat::Tga, MediaFormat::Tga),
        ("rgba.qoi", rgba.clone(), ImageFormat::Qoi, MediaFormat::Qoi),
        (
            "rgba.ff",
            DynamicImage::ImageRgba16(rgba.to_rgba16()),
            ImageFormat::Farbfeld,
            MediaFormat::Farbfeld,
        ),
        ("rgb.tif", opaque(), ImageFormat::Tiff, MediaFormat::Tiff),
        (
            "rgba16.tif",
            DynamicImage::ImageRgba16(rgba.to_rgba16()),
            ImageFormat::Tiff,
            MediaFormat::Tiff,
        ),
        (
            "gray.tif",
            DynamicImage::ImageLuma8(rgba.to_luma8()),
            ImageFormat::Tiff,
            MediaFormat::Tiff,
        ),
        ("lossless.webp", rgba.clone(), ImageFormat::WebP, MediaFormat::Webp),
    ];
    for (name, source, format, media) in cases {
        let path = write_source(dir.path(), name, &source, format);
        let before = image::open(&path).expect("decode the source");
        let outcome = save(&path, media, crop).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(outcome.encoding.is_lossless(), "{name}: {:?}", outcome.encoding);
        let after = image::ImageReader::open(&path)
            .and_then(image::ImageReader::with_guessed_format)
            .expect("open")
            .decode()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(after.dimensions(), (crop.width, crop.height), "{name}");
        let expected = before.crop_imm(crop.x, crop.y, crop.width, crop.height);
        assert_eq!(after.color(), expected.color(), "{name}: the color type changed");
        assert_eq!(after.as_bytes(), expected.as_bytes(), "{name}: the pixels changed");
        assert!(!backup_path(&path).exists(), "{name}: the backup was left behind");
    }
}

/// A PNG with a palette (`bits` per pixel) and a transparency table.
fn write_indexed_png(path: &Path, bits: png::BitDepth) {
    let file = std::fs::File::create(path).expect("create");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    encoder.set_color(png::ColorType::Indexed);
    encoder.set_depth(bits);
    let colors = 1usize << (bits as u8);
    encoder.set_palette(
        (0..colors * 3)
            .map(|i| u8::try_from((i * 37) % 256).unwrap_or(0))
            .collect::<Vec<u8>>(),
    );
    encoder.set_trns(vec![0u8, 128]);
    let per_byte = 8 / usize::from(bits as u8);
    let row_bytes = (W as usize).div_ceil(per_byte);
    let mut data = vec![0u8; row_bytes * H as usize];
    for y in 0..H as usize {
        for x in 0..W as usize {
            let index = u8::try_from((x * 3 + y * 5) % colors).unwrap_or(0);
            let shift = 8 - usize::from(bits as u8) * (x % per_byte + 1);
            data[y * row_bytes + x / per_byte] |= index << shift;
        }
    }
    let mut writer = encoder.write_header().expect("header");
    writer.write_image_data(&data).expect("data");
}

#[test]
fn a_palette_png_keeps_its_palette_bit_depth_and_transparency() {
    let dir = tempfile::tempdir().expect("temp dir");
    for bits in [
        png::BitDepth::One,
        png::BitDepth::Two,
        png::BitDepth::Four,
        png::BitDepth::Eight,
    ] {
        let path = dir.path().join(format!("palette{}.png", bits as u8));
        write_indexed_png(&path, bits);
        let before = image::open(&path).expect("decode");
        let crop = PixelRect::new(3, 5, 41, 29);
        save(&path, MediaFormat::Png, crop).unwrap_or_else(|e| panic!("{bits:?}: {e}"));
        let reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).expect("open")))
            .read_info()
            .expect("read");
        let info = reader.info();
        assert_eq!(info.color_type, png::ColorType::Indexed);
        assert_eq!(info.bit_depth, bits);
        assert_eq!(info.trns.as_deref(), Some(&[0u8, 128][..]));
        let after = image::open(&path).expect("decode");
        assert_eq!(
            after.to_rgba8().as_raw(),
            before.crop_imm(3, 5, 41, 29).to_rgba8().as_raw(),
            "{bits:?}"
        );
    }
}

#[test]
fn a_still_gif_keeps_its_colors_and_an_animated_one_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let palette: Vec<u8> = (0..256 * 3).map(|i| u8::try_from((i * 7) % 256).unwrap_or(0)).collect();
    let write_gif = |name: &str, frames: usize| {
        let path = dir.path().join(name);
        let mut out = Vec::new();
        {
            let mut encoder = gif::Encoder::new(&mut out, 40, 30, &palette).expect("encoder");
            for f in 0..frames {
                let buffer: Vec<u8> = (0..40 * 30)
                    .map(|i| u8::try_from((i + f * 3) % 256).unwrap_or(0))
                    .collect();
                let frame = gif::Frame {
                    width: 40,
                    height: 30,
                    buffer: std::borrow::Cow::Owned(buffer),
                    ..gif::Frame::default()
                };
                encoder.write_frame(&frame).expect("frame");
            }
        }
        std::fs::write(&path, out).expect("write");
        path
    };
    let still = write_gif("still.gif", 1);
    let before = image::open(&still).expect("decode");
    save(&still, MediaFormat::Gif, PixelRect::new(5, 4, 20, 10)).expect("save");
    let after = image::open(&still).expect("decode");
    assert_eq!(
        after.to_rgba8().as_raw(),
        before.crop_imm(5, 4, 20, 10).to_rgba8().as_raw()
    );

    let animated = write_gif("animated.gif", 3);
    let bytes = std::fs::read(&animated).expect("read");
    assert!(matches!(
        save(&animated, MediaFormat::Gif, PixelRect::new(0, 0, 10, 10)),
        Err(SaveError::Refused(super::formats::EditRefusal::Animated))
    ));
    assert_eq!(std::fs::read(&animated).expect("read"), bytes);
}

/// A JPEG written by libjpeg-turbo-rs with 4:2:0 sampling, carrying EXIF (with a thumbnail link), an ICC profile and XMP.
fn write_jpeg(path: &Path, progressive: bool) {
    let rgb = smooth().to_rgb8();
    let jpeg = libjpeg_turbo_rs::Encoder::new(rgb.as_raw(), W as usize, H as usize, libjpeg_turbo_rs::PixelFormat::Rgb)
        .quality(85)
        .subsampling(libjpeg_turbo_rs::Subsampling::S420)
        .progressive(progressive)
        .icc_profile(b"fake icc profile bytes")
        .xmp_data(b"<x:xmpmeta>rating</x:xmpmeta>")
        .encode()
        .expect("encode");
    let mut parts = img_parts::jpeg::Jpeg::from_bytes(bytes::Bytes::from(jpeg)).expect("parts");
    let exif = sample_exif(false);
    img_parts::ImageEXIF::set_exif(&mut parts, Some(bytes::Bytes::from(exif[6..].to_vec())));
    std::fs::write(path, parts.encoder().bytes()).expect("write");
}

fn check_jpeg_metadata(path: &Path, size: (u32, u32)) {
    let data = std::fs::read(path).expect("read");
    let meta = read_metadata(&data, MediaFormat::Jpeg);
    assert_eq!(meta.icc.as_deref(), Some(&b"fake icc profile bytes"[..]));
    assert_eq!(meta.xmp.as_deref(), Some(&b"<x:xmpmeta>rating</x:xmpmeta>"[..]));
    let exif = meta.exif.expect("EXIF kept");
    assert_eq!(
        read_sizes(&exif),
        Some([Some(size.0), Some(size.1), Some(size.0), Some(size.1)])
    );
    assert_eq!(thumbnail_link(&exif), Some(0));
}

#[test]
fn a_jpeg_crop_on_the_block_grid_is_lossless_and_keeps_the_metadata() {
    let dir = tempfile::tempdir().expect("temp dir");
    for progressive in [false, true] {
        let path = dir.path().join(format!("photo{progressive}.jpg"));
        write_jpeg(&path, progressive);
        let original = std::fs::read(&path).expect("read");
        let crop = PixelRect::new(16, 32, 61, 27);
        let outcome = save(&path, MediaFormat::Jpeg, crop).expect("save");
        assert_eq!(outcome.encoding, Encoding::JpegLossless { crop });
        // The blocks inside the crop are the original's blocks.
        let before = libjpeg_turbo_rs::read_coefficients(&original).expect("coefficients");
        let after = libjpeg_turbo_rs::read_coefficients(&std::fs::read(&path).expect("read")).expect("coefficients");
        assert_eq!(after.quant_tables, before.quant_tables);
        let (y_before, y_after) = (&before.components[0], &after.components[0]);
        for by in 0..y_after.blocks_y.min(3) {
            for bx in 0..y_after.blocks_x.min(7) {
                assert_eq!(
                    y_after.blocks[by * y_after.blocks_x + bx],
                    y_before.blocks[(by + 4) * y_before.blocks_x + bx + 2]
                );
            }
        }
        check_jpeg_metadata(&path, (61, 27));
    }
}

#[test]
fn a_jpeg_crop_off_the_grid_is_encoded_with_the_original_tables_and_keeps_the_metadata() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("photo.jpg");
    write_jpeg(&path, false);
    let original = std::fs::read(&path).expect("read");
    let before = image::load_from_memory(&original).expect("decode");
    let crop = PixelRect::new(13, 7, 50, 40);
    let outcome = save(&path, MediaFormat::Jpeg, crop).expect("save");
    assert_eq!(outcome.encoding, Encoding::JpegSameTables);
    let data = std::fs::read(&path).expect("read");
    let tables = |d: &[u8]| {
        libjpeg_turbo_rs::read_coefficients(d)
            .expect("coefficients")
            .quant_tables
    };
    assert_eq!(tables(&data), tables(&original));
    let after = image::load_from_memory(&data).expect("decode");
    assert_eq!(after.dimensions(), (50, 40));
    // Close to the picture it was: the loss of one more generation with the same tables is small.
    let quality = psnr(&after, &before.crop_imm(13, 7, 50, 40));
    assert!(quality > 34.0, "{quality}");
    check_jpeg_metadata(&path, (50, 40));
}

#[test]
fn a_lossy_webp_stays_webp_and_keeps_icc_exif_and_xmp() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("photo.webp");
    let rgb = smooth().to_rgb8();
    let lossy = webp::Encoder::from_rgb(rgb.as_raw(), W, H).encode(80.0).to_vec();
    let mut parts = img_parts::webp::WebP::from_bytes(bytes::Bytes::from(lossy)).expect("parts");
    img_parts::ImageICC::set_icc_profile(&mut parts, Some(bytes::Bytes::from_static(b"icc")));
    img_parts::ImageEXIF::set_exif(&mut parts, Some(bytes::Bytes::from(sample_exif(true)[6..].to_vec())));
    std::fs::write(&path, parts.encoder().bytes()).expect("write");
    let before = image::open(&path).expect("decode");
    let crop = PixelRect::new(10, 10, 60, 40);
    let outcome = save(&path, MediaFormat::Webp, crop).expect("save");
    assert!(matches!(outcome.encoding, Encoding::Webp { lossless: false, .. }));
    let after = image::open(&path).expect("decode");
    assert_eq!(after.dimensions(), (60, 40));
    let quality = psnr(&after, &before.crop_imm(10, 10, 60, 40));
    assert!(quality > 32.0, "{quality}");
    let meta = read_metadata(&std::fs::read(&path).expect("read"), MediaFormat::Webp);
    assert_eq!(meta.icc.as_deref(), Some(&b"icc"[..]));
    let exif = meta.exif.expect("EXIF kept");
    assert_eq!(read_sizes(&exif).map(|s| s[2]), Some(Some(60)));
}

#[test]
fn a_png_keeps_its_text_icc_and_exif_chunks() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(dir.path(), "meta.png", &opaque(), ImageFormat::Png);
    let mut parts =
        img_parts::png::Png::from_bytes(bytes::Bytes::from(std::fs::read(&path).expect("read"))).expect("parts");
    img_parts::ImageICC::set_icc_profile(&mut parts, Some(bytes::Bytes::from_static(b"profile")));
    img_parts::ImageEXIF::set_exif(&mut parts, Some(bytes::Bytes::from(sample_exif(false)[6..].to_vec())));
    let at = parts.chunks().len() - 1;
    parts.chunks_mut().insert(
        at,
        img_parts::png::PngChunk::new(*b"tEXt", bytes::Bytes::from_static(b"Comment\0kept")),
    );
    std::fs::write(&path, parts.encoder().bytes()).expect("write");
    save(&path, MediaFormat::Png, PixelRect::new(0, 0, 40, 30)).expect("save");
    let data = std::fs::read(&path).expect("read");
    let meta = read_metadata(&data, MediaFormat::Png);
    assert_eq!(meta.icc.as_deref(), Some(&b"profile"[..]));
    assert_eq!(read_sizes(&meta.exif.expect("EXIF")).map(|s| s[3]), Some(Some(30)));
    let png = img_parts::png::Png::from_bytes(bytes::Bytes::from(data)).expect("parts");
    assert!(
        png.chunks_by_type(*b"tEXt")
            .any(|c| c.contents().as_ref() == b"Comment\0kept")
    );
    // The chunks that must come before the image data do.
    let kinds: Vec<[u8; 4]> = png.chunks().iter().map(img_parts::png::PngChunk::kind).collect();
    let idat = kinds.iter().position(|k| k == b"IDAT").expect("IDAT");
    assert!(kinds.iter().position(|k| k == b"iCCP").expect("iCCP") < idat);
}

#[test]
fn a_file_changed_on_disk_is_never_overwritten_without_the_users_word() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(dir.path(), "a.png", &opaque(), ImageFormat::Png);
    let probe = probe(&path, MediaFormat::Png).expect("probe");
    let edited = picture(&path, MediaFormat::Png, &probe, PixelRect::new(0, 0, 10, 10));
    // Somebody else writes the file after it was opened.
    write_source(
        dir.path(),
        "a.png",
        &DynamicImage::ImageRgba8(pattern()),
        ImageFormat::Png,
    );
    let changed = std::fs::read(&path).expect("read");
    let result = save_in_place(&edited, &EncodeOptions::default(), OverwriteMode::Safe, false);
    assert!(matches!(result, Err(SaveError::Conflict(DiskState::Modified))));
    assert_eq!(std::fs::read(&path).expect("read"), changed);
    // The user overwrites anyway.
    save_in_place(&edited, &EncodeOptions::default(), OverwriteMode::Safe, true).expect("save");
    assert_eq!(image::image_dimensions(&path).expect("size"), (10, 10));
    // Deleted in the meantime: missing, and "save here anyway" creates it again.
    std::fs::remove_file(&path).expect("remove");
    assert!(matches!(
        save_in_place(&edited, &EncodeOptions::default(), OverwriteMode::Safe, false),
        Err(SaveError::Conflict(DiskState::Missing) | SaveError::Read(_))
    ));
}

#[test]
fn read_only_files_and_folders_are_never_touched() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(dir.path(), "ro.png", &opaque(), ImageFormat::Png);
    let bytes = std::fs::read(&path).expect("read");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("chmod");
    assert!(matches!(
        save(&path, MediaFormat::Png, PixelRect::new(0, 0, 5, 5)),
        Err(SaveError::Write(WriteError::ReadOnly))
    ));
    assert_eq!(std::fs::read(&path).expect("read"), bytes);

    let folder = dir.path().join("locked");
    std::fs::create_dir(&folder).expect("mkdir");
    let path = write_source(&folder, "a.png", &opaque(), ImageFormat::Png);
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let result = save(&path, MediaFormat::Png, PixelRect::new(0, 0, 5, 5));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    assert!(result.is_err());
    assert_eq!(image::image_dimensions(&path).expect("size"), (W, H));
    assert_eq!(
        std::fs::read_dir(&folder).expect("list").count(),
        1,
        "a temporary file was left behind"
    );
}

#[test]
fn permissions_are_kept_and_a_symbolic_link_stays_a_link() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("temp dir");
    let target = write_source(dir.path(), "real.png", &opaque(), ImageFormat::Png);
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o640)).expect("chmod");
    let link = dir.path().join("link.png");
    std::os::unix::fs::symlink("real.png", &link).expect("symlink");
    let outcome = save(&link, MediaFormat::Png, PixelRect::new(1, 1, 20, 20)).expect("save");
    assert_eq!(outcome.written, std::fs::canonicalize(&target).expect("canonical"));
    assert!(std::fs::symlink_metadata(&link).expect("meta").file_type().is_symlink());
    assert_eq!(image::image_dimensions(&target).expect("size"), (20, 20));
    let mode = std::fs::metadata(&target).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

#[test]
fn every_overwrite_mode_ends_with_the_new_file_and_no_leftovers() {
    let dir = tempfile::tempdir().expect("temp dir");
    let trash = dir.path().join("trash");
    std::fs::create_dir(&trash).expect("mkdir");
    let to_trash = |p: &Path| -> Result<(), String> {
        std::fs::rename(p, trash.join(p.file_name().unwrap_or_default())).map_err(|e| e.to_string())
    };
    for mode in OverwriteMode::ALL {
        let name = format!("{}.bin", mode.as_str());
        let path = dir.path().join(&name);
        std::fs::write(&path, b"old").expect("write");
        let identity = FileIdentity::read(&path).ok();
        overwrite_using(&path, b"new content", *mode, Expect::Unchanged(identity), &to_trash).expect("overwrite");
        assert_eq!(std::fs::read(&path).expect("read"), b"new content");
        assert!(!backup_path(&path).exists());
        assert_eq!(trash.join(&name).exists(), *mode == OverwriteMode::Trash);
    }
    // A trash that refuses leaves the original where it was.
    let path = dir.path().join("kept.bin");
    std::fs::write(&path, b"old").expect("write");
    let refused = overwrite_using(&path, b"new", OverwriteMode::Trash, Expect::Anything, &|_| {
        Err("no trash".to_string())
    });
    assert!(matches!(refused, Err(WriteError::Trash(_))));
    assert_eq!(std::fs::read(&path).expect("read"), b"old");
    assert_eq!(
        std::fs::read_dir(dir.path()).expect("list").count(),
        2 + OverwriteMode::ALL.len()
    );
}

#[test]
fn the_content_decides_the_format_of_a_misnamed_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(
        dir.path(),
        "really_a_png.jpg",
        &DynamicImage::ImageRgba8(pattern()),
        ImageFormat::Png,
    );
    let before = decode_by_content(&path);
    let outcome = save(&path, MediaFormat::Jpeg, PixelRect::new(2, 2, 30, 30)).expect("save");
    assert!(matches!(outcome.encoding, Encoding::Png { .. }));
    let data = std::fs::read(&path).expect("read");
    assert!(data.starts_with(b"\x89PNG"));
    let after = image::load_from_memory(&data).expect("decode");
    assert_eq!(
        after.to_rgba8().as_raw(),
        before.crop_imm(2, 2, 30, 30).to_rgba8().as_raw()
    );
}

#[test]
fn a_picture_that_changed_size_is_not_written() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(dir.path(), "a.png", &opaque(), ImageFormat::Png);
    let bytes = std::fs::read(&path).expect("read");
    let probe = probe(&path, MediaFormat::Png).expect("probe");
    let mut edited = picture(&path, MediaFormat::Png, &probe, PixelRect::new(0, 0, 10, 10));
    edited.original_size = (W + 1, H);
    assert!(matches!(
        save_in_place(&edited, &EncodeOptions::default(), OverwriteMode::Safe, false),
        Err(SaveError::Render(_))
    ));
    assert_eq!(std::fs::read(&path).expect("read"), bytes);
}

#[test]
fn exports_write_a_new_file_and_never_replace_one_unasked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = write_source(
        dir.path(),
        "source.png",
        &DynamicImage::ImageRgba8(pattern()),
        ImageFormat::Png,
    );
    let original = std::fs::read(&path).expect("read");
    let probe = probe(&path, MediaFormat::Png).expect("probe");
    let edited = picture(&path, MediaFormat::Png, &probe, PixelRect::new(4, 4, 50, 30));
    for format in WriteFormat::EXPORT_TARGETS {
        let target = dir.path().join(format!("out.{}", format.extension()));
        let request = ExportTarget {
            path: &target,
            format,
            options: EncodeOptions {
                avif_speed: 10,
                ..EncodeOptions::default()
            },
            replace: false,
        };
        let outcome = export(&edited, &request).unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!(outcome.size, (50, 30));
        assert!(target.exists());
        assert!(matches!(
            export(&edited, &request),
            Err(SaveError::Write(WriteError::Exists))
        ));
        let replace = ExportTarget {
            replace: true,
            ..request
        };
        export(&edited, &replace).unwrap_or_else(|e| panic!("{format:?} replace: {e}"));
    }
    assert_eq!(std::fs::read(&path).expect("read"), original, "the source was changed");
}
