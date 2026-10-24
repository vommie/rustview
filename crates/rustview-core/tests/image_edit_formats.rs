//! Every picture of a media tree, through the editor: probe, the decision how it is saved, then a crop saved over a copy of it
//! (read back and compared) or exported as a new file, or the refusal of an animated or multi-page file. The originals are only
//! read. The tree is the one of the test environment (`RUSTVIEW_EDIT_SAMPLES=<folder>`); without it the test says so and passes.

use rustview_core::image_edit::conflict::FileIdentity;
use rustview_core::image_edit::formats::{EditRefusal, SaveSupport, WriteFormat, save_support};
use rustview_core::image_edit::geometry::PixelRect;
use rustview_core::image_edit::options::{EncodeOptions, OverwriteMode};
use rustview_core::image_edit::probe::probe;
use rustview_core::image_edit::render::decode_any;
use rustview_core::image_edit::save::{EditedPicture, EncodingChoice, ExportTarget, export, save_in_place};
use rustview_core::image_edit::stack::EditPlan;
use rustview_core::media::{MediaFormat, MediaType};
use std::path::{Path, PathBuf};

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
                        match back {
                            Ok(back) if (back.width(), back.height()) == crop.size() => {
                                report.push(format!("{name}: saved in place ({})", outcome.encoding.describe()));
                            }
                            Ok(back) => failures.push(format!(
                                "{name}: saved as {}x{} instead of {}x{}",
                                back.width(),
                                back.height(),
                                crop.width,
                                crop.height
                            )),
                            Err(e) => failures.push(format!("{name}: the saved file does not decode: {e}")),
                        }
                    }
                    Err(e) => failures.push(format!("{name}: saving failed: {e}")),
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
                    Ok(_) => match image::image_dimensions(&target) {
                        Ok(dims) if dims == crop.size() => report.push(format!("{name}: exported ({reason:?})")),
                        other => failures.push(format!("{name}: the export is {other:?}")),
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
