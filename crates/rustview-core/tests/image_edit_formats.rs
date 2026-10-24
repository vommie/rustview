//! Every picture of a media tree, through the editor: probe, the decision how it is saved, then a crop saved over a copy of it
//! or exported as a new file, or the refusal of an animated or multi-page file. Every written file is read back and its pixels
//! are compared with the same crop of the decoded original: equal for lossless encodings, close (PSNR) for lossy ones. The
//! originals are only read. The tree is the one of the test environment (`RUSTVIEW_EDIT_SAMPLES=<folder>`); without it the test
//! says so and passes.

use image::{DynamicImage, GenericImageView};
use rustview_core::image_edit::conflict::FileIdentity;
use rustview_core::image_edit::formats::{EditRefusal, SaveSupport, WriteFormat, save_support};
use rustview_core::image_edit::geometry::PixelRect;
use rustview_core::image_edit::options::Encoding;
use rustview_core::image_edit::options::{EncodeOptions, OverwriteMode};
use rustview_core::image_edit::probe::probe;
use rustview_core::image_edit::render::decode_any;
use rustview_core::image_edit::save::{EditedPicture, EncodingChoice, ExportTarget, export, save_in_place};
use rustview_core::image_edit::stack::EditPlan;
use rustview_core::media::{MediaFormat, MediaType};
use rustview_core::util::cast::Az;
use std::path::{Path, PathBuf};

/// The smallest PSNR (dB) a lossy save or a lossless JPEG crop (its edge blocks are decoded with other neighbors) must reach.
const MIN_PSNR_LOSSY: f64 = 30.0;
const MIN_PSNR_JPEG_LOSSLESS: f64 = 38.0;

/// PSNR of two pictures of the same size over their RGB channels (8 bit); infinite when they are equal.
fn psnr(a: &DynamicImage, b: &DynamicImage) -> f64 {
    let (a, b) = (a.to_rgb8(), b.to_rgb8());
    let mut sum = 0.0;
    for (x, y) in a.as_raw().iter().zip(b.as_raw()) {
        let d = f64::from(*x) - f64::from(*y);
        sum += d * d;
    }
    let mse = sum / a.as_raw().len().max(1).az::<f64>();
    if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    }
}

/// Compares the written picture with the crop of the original: `Ok(text)` describes the match, `Err` the mismatch.
fn compare(expected: &DynamicImage, written: &DynamicImage, encoding: Option<Encoding>) -> Result<String, String> {
    if expected.dimensions() != written.dimensions() {
        return Err(format!(
            "size {:?} instead of {:?}",
            written.dimensions(),
            expected.dimensions()
        ));
    }
    let exact = expected.to_rgba16() == written.to_rgba16();
    let quality = psnr(expected, written);
    let needed = match encoding {
        Some(Encoding::JpegLossless { .. }) => Some(MIN_PSNR_JPEG_LOSSLESS),
        Some(e) if e.is_lossless() => None,
        _ => Some(MIN_PSNR_LOSSY),
    };
    match needed {
        None if exact => Ok("pixels equal".to_string()),
        None => Err(format!("a lossless save changed pixels (PSNR {quality:.1} dB)")),
        Some(min) if exact || quality >= min => Ok(format!("PSNR {quality:.1} dB")),
        Some(min) => Err(format!("PSNR {quality:.1} dB below {min} dB")),
    }
}

fn pictures(root: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(walkdir::DirEntry::into_path)
        .filter(|p| MediaFormat::from_path(p).media_type() == Some(MediaType::Image))
        .collect();
    found.sort();
    found
}

/// The middle half of a picture of `size`, starting off the JPEG block grid.
fn crop_of((w, h): (u32, u32)) -> PixelRect {
    let (cw, ch) = ((w / 2).max(1), (h / 2).max(1));
    PixelRect::new(((w - cw) / 2).saturating_sub(3) | 1, ((h - ch) / 2) | 1, cw, ch).clamped_into((w, h))
}

#[test]
fn every_sample_picture_is_saved_exported_or_refused_correctly() {
    let Ok(root) = std::env::var("RUSTVIEW_EDIT_SAMPLES") else {
        eprintln!("RUSTVIEW_EDIT_SAMPLES is not set: the sample pictures are not checked");
        return;
    };
    let work = tempfile::tempdir().expect("temp dir");
    let mut report = Vec::new();
    let mut failures = Vec::new();
    for source in pictures(Path::new(&root)) {
        let name = source.strip_prefix(&root).unwrap_or(&source).display().to_string();
        let format = MediaFormat::from_path(&source);
        let Ok(probed) = probe(&source, format) else {
            report.push(format!("{name}: cannot be examined (not editable)"));
            continue;
        };
        let original = std::fs::read(&source).expect("read the sample");
        let Ok(decoded) = decode_any(&original, &source, probed.signature.unwrap_or(format)) else {
            report.push(format!("{name}: not decodable (not editable)"));
            continue;
        };
        let size = (decoded.width(), decoded.height());
        let crop = crop_of(size);
        let expected = decoded.crop_imm(crop.x, crop.y, crop.width, crop.height);
        let plan = EditPlan {
            crop: Some(crop),
            output_size: crop.size(),
        };
        match save_support(format, &probed) {
            Err(refusal) => {
                let ok = matches!(refusal, EditRefusal::Animated | EditRefusal::MultiPage);
                report.push(format!("{name}: refused ({refusal:?})"));
                if !ok {
                    failures.push(format!("{name}: unexpected refusal {refusal:?}"));
                }
            }
            Ok(SaveSupport::InPlace(write)) => {
                let copy = work.path().join(source.file_name().unwrap_or_default());
                std::fs::copy(&source, &copy).expect("copy");
                let picture = EditedPicture {
                    path: &copy,
                    format,
                    probe: &probed,
                    plan,
                    original_size: size,
                    identity: FileIdentity::read(&copy).ok(),
                };
                match save_in_place(&picture, &EncodeOptions::default(), OverwriteMode::Safe, false) {
                    Ok(outcome) => {
                        let written = std::fs::read(&copy).expect("read back");
                        let back = decode_any(&written, &copy, write.media_format());
                        match back.map(|back| compare(&expected, &back, Some(outcome.encoding))) {
                            Ok(Ok(how)) => {
                                report.push(format!(
                                    "{name}: saved in place ({}), {how}",
                                    outcome.encoding.describe()
                                ));
                            }
                            Ok(Err(why)) => failures.push(format!("{name}: saved in place, but {why}")),
                            Err(e) => failures.push(format!("{name}: the saved file does not decode: {e}")),
                        }
                    }
                    Err(e) => failures.push(format!("{name}: saving failed: {e}")),
                }
                if let Some(layout) = probed.jpeg {
                    // The same picture once more with a crop on the block grid: cut out without decoding it.
                    let (mw, mh) = layout.mcu;
                    let aligned = PixelRect::new(mw, mh, (size.0 / 2).max(1), (size.1 / 2).max(1)).clamped_into(size);
                    let aligned = PixelRect::new(
                        aligned.x - aligned.x % mw,
                        aligned.y - aligned.y % mh,
                        aligned.width,
                        aligned.height,
                    );
                    std::fs::copy(&source, &copy).expect("copy");
                    let picture = EditedPicture {
                        path: &copy,
                        format,
                        probe: &probed,
                        plan: EditPlan {
                            crop: Some(aligned),
                            output_size: aligned.size(),
                        },
                        original_size: size,
                        identity: FileIdentity::read(&copy).ok(),
                    };
                    let expected = decoded.crop_imm(aligned.x, aligned.y, aligned.width, aligned.height);
                    match save_in_place(&picture, &EncodeOptions::default(), OverwriteMode::Safe, false) {
                        Ok(outcome) if matches!(outcome.encoding, Encoding::JpegLossless { .. }) => {
                            let written = std::fs::read(&copy).expect("read back");
                            match decode_any(&written, &copy, MediaFormat::Jpeg)
                                .map(|b| compare(&expected, &b, Some(outcome.encoding)))
                            {
                                Ok(Ok(how)) => {
                                    report.push(format!("{name}: block-grid crop saved without loss, {how}"));
                                }
                                Ok(Err(why)) => failures.push(format!("{name}: block-grid crop, but {why}")),
                                Err(e) => failures.push(format!("{name}: the block-grid crop does not decode: {e}")),
                            }
                        }
                        Ok(outcome) => failures.push(format!(
                            "{name}: a block-grid crop was saved as {}",
                            outcome.encoding.describe()
                        )),
                        Err(e) => failures.push(format!("{name}: saving the block-grid crop failed: {e}")),
                    }
                }
            }
            Ok(SaveSupport::ExportOnly(reason)) => {
                let target = work.path().join(format!("{}.png", name.replace('/', "_")));
                let picture = EditedPicture {
                    path: &source,
                    format,
                    probe: &probed,
                    plan,
                    original_size: size,
                    identity: FileIdentity::read(&source).ok(),
                };
                let request = ExportTarget {
                    path: &target,
                    format: WriteFormat::Png,
                    encoding: EncodingChoice::Options(EncodeOptions::default()),
                    replace: false,
                };
                match export(&picture, &request) {
                    // The export converts the picture to PNG (8 or 16 bit RGB(A)): compared closely, not bit for bit.
                    Ok(outcome) => match image::open(&target).map(|back| compare(&expected, &back, None)) {
                        Ok(Ok(how)) => report.push(format!(
                            "{name}: exported ({reason:?}, {}), {how}",
                            outcome.encoding.describe()
                        )),
                        Ok(Err(why)) => failures.push(format!("{name}: exported, but {why}")),
                        Err(e) => failures.push(format!("{name}: the export does not decode: {e}")),
                    },
                    Err(e) => failures.push(format!("{name}: export failed: {e}")),
                }
            }
        }
        assert_eq!(
            std::fs::read(&source).expect("read the sample"),
            original,
            "{name}: the original changed"
        );
    }
    eprintln!("{}", report.join("\n"));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
