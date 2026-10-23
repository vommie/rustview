//! Saving an edited picture (only on the user's explicit request): overwriting the original in its own format, or writing a
//! new file in a format of the user's choice (export, "Save As"). The one place that ties the decisions together: whether
//! the picture can be written, whether the file is still the one that was edited, the encoding, rendering, checking the
//! result and writing it safely.

use super::conflict::{DiskState, FileIdentity, check};
use super::formats::{EditRefusal, ExportReason, SaveSupport, WriteFormat, save_support};
use super::metadata::read_metadata;
use super::options::{EncodeOptions, Encoding, OverwriteMode, export_encoding, in_place_encoding};
use super::probe::SourceProbe;
use super::render::{RenderInput, render};
use super::stack::EditPlan;
use super::writer::{self, Expect, WriteError};
use crate::media::MediaFormat;
use crate::settings::LogObscurer;
use std::path::{Path, PathBuf};
use std::time::Instant;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SaveError {
    #[error("there is nothing to save")]
    NothingToSave,
    #[error("{}", .0.text())]
    Refused(EditRefusal),
    #[error("{}", .0.text())]
    NeedsExport(ExportReason),
    #[error("the file changed on disk")]
    Conflict(DiskState),
    #[error("the original could not be read: {0}")]
    Read(std::io::Error),
    #[error("the picture could not be encoded: {0}")]
    Render(String),
    #[error("the new file is not valid: {0}")]
    Invalid(String),
    #[error("{0}")]
    Write(WriteError),
}

impl From<WriteError> for SaveError {
    fn from(e: WriteError) -> Self {
        match e {
            WriteError::Changed(state) => Self::Conflict(state),
            other => Self::Write(other),
        }
    }
}

/// The picture that was edited.
#[derive(Debug, Clone)]
pub struct EditedPicture<'a> {
    /// The file of the medium (a symbolic link is followed when writing).
    pub path: &'a Path,
    /// The format of its extension (what the library knows).
    pub format: MediaFormat,
    pub probe: &'a SourceProbe,
    pub plan: EditPlan,
    /// The size of the decoded picture the edits were made on.
    pub original_size: (u32, u32),
    /// The identity of the file when it was decoded.
    pub identity: Option<FileIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub written: PathBuf,
    pub identity: FileIdentity,
    pub encoding: Encoding,
    pub size: (u32, u32),
    pub bytes: usize,
}

/// Overwrites the original in its own format. `overwrite_anyway` is the user's answer to a conflict (write whatever is on disk
/// now, or create the file again).
pub fn save_in_place(
    picture: &EditedPicture<'_>,
    presets: &EncodeOptions,
    mode: OverwriteMode,
    overwrite_anyway: bool,
) -> Result<SaveOutcome, SaveError> {
    let started = Instant::now();
    let write = match save_support(picture.format, picture.probe).map_err(SaveError::Refused)? {
        SaveSupport::InPlace(write) => write,
        SaveSupport::ExportOnly(reason) => return Err(SaveError::NeedsExport(reason)),
    };
    if picture.plan.is_identity() {
        return Err(SaveError::NothingToSave);
    }
    if !overwrite_anyway {
        let state = check(picture.path, picture.identity);
        if state.is_conflict() {
            return Err(SaveError::Conflict(state));
        }
    }
    let data = std::fs::read(picture.path).map_err(SaveError::Read)?;
    let actual = picture.probe.signature.unwrap_or(picture.format);
    let encoding = in_place_encoding(write, picture.probe, &picture.plan, presets);
    let bytes = rendered(picture, &data, actual, write, encoding)?;
    let expect = if overwrite_anyway {
        Expect::Anything
    } else {
        Expect::Unchanged(picture.identity)
    };
    let outcome = writer::overwrite(picture.path, &bytes, mode, expect)?;
    tracing::info!(
        path = %LogObscurer::path(picture.path),
        encoding = %encoding.describe(),
        width = picture.plan.output_size.0,
        height = picture.plan.output_size.1,
        bytes = bytes.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "[ImageEdit: Save] Saved over the original"
    );
    Ok(SaveOutcome {
        written: outcome.written,
        identity: outcome.identity,
        encoding,
        size: picture.plan.output_size,
        bytes: bytes.len(),
    })
}

/// Where and how an export is written.
#[derive(Debug, Clone)]
pub struct ExportTarget<'a> {
    pub path: &'a Path,
    pub format: WriteFormat,
    pub options: EncodeOptions,
    /// The user confirmed that an existing file at `path` is replaced.
    pub replace: bool,
}

/// Writes the edited picture to a new file; the original stays as it is.
pub fn export(picture: &EditedPicture<'_>, target: &ExportTarget<'_>) -> Result<SaveOutcome, SaveError> {
    let started = Instant::now();
    if let Err(refusal) = save_support(picture.format, picture.probe) {
        return Err(SaveError::Refused(refusal));
    }
    let data = std::fs::read(picture.path).map_err(SaveError::Read)?;
    let actual = picture.probe.signature.unwrap_or(picture.format);
    let encoding = export_encoding(target.format, &target.options);
    let bytes = rendered(picture, &data, actual, target.format, encoding)?;
    let outcome = if target.replace {
        writer::overwrite(target.path, &bytes, OverwriteMode::Safe, Expect::Anything)?
    } else {
        writer::create_new(target.path, &bytes)?
    };
    tracing::info!(
        source = %LogObscurer::path(picture.path),
        target = %LogObscurer::path(target.path),
        encoding = %encoding.describe(),
        bytes = bytes.len(),
        elapsed_ms = started.elapsed().as_millis(),
        "[ImageEdit: Save] Exported to a new file"
    );
    Ok(SaveOutcome {
        written: outcome.written,
        identity: outcome.identity,
        encoding,
        size: picture.plan.output_size,
        bytes: bytes.len(),
    })
}

fn rendered(
    picture: &EditedPicture<'_>,
    data: &[u8],
    actual: MediaFormat,
    target: WriteFormat,
    encoding: Encoding,
) -> Result<Vec<u8>, SaveError> {
    let metadata = read_metadata(data, actual);
    let input = RenderInput {
        data,
        path: picture.path,
        format: actual,
        plan: picture.plan,
        original_size: picture.original_size,
        metadata: &metadata,
    };
    let rendered = render(&input, target, encoding).map_err(SaveError::Render)?;
    verify_encoded(&rendered.bytes, target, rendered.size).map_err(SaveError::Invalid)?;
    Ok(rendered.bytes)
}

/// The new file decodes and has the size it should have (checked before anything on disk is touched).
pub fn verify_encoded(bytes: &[u8], format: WriteFormat, size: (u32, u32)) -> Result<(), String> {
    let decoded = match format {
        WriteFormat::Avif => {
            let avif = avif_parse::read_avif(&mut &bytes[..]).map_err(|e| e.to_string())?;
            let meta = avif.primary_item_metadata().map_err(|e| e.to_string())?;
            (meta.max_frame_width.get(), meta.max_frame_height.get())
        }
        other => {
            let format = match other {
                WriteFormat::Jpeg => image::ImageFormat::Jpeg,
                WriteFormat::Png => image::ImageFormat::Png,
                WriteFormat::Webp => image::ImageFormat::WebP,
                WriteFormat::Tiff => image::ImageFormat::Tiff,
                WriteFormat::Bmp => image::ImageFormat::Bmp,
                WriteFormat::Gif => image::ImageFormat::Gif,
                WriteFormat::Qoi => image::ImageFormat::Qoi,
                WriteFormat::Tga => image::ImageFormat::Tga,
                WriteFormat::Farbfeld | WriteFormat::Avif => image::ImageFormat::Farbfeld,
            };
            let picture = image::load_from_memory_with_format(bytes, format).map_err(|e| e.to_string())?;
            (picture.width(), picture.height())
        }
    };
    if decoded == size {
        Ok(())
    } else {
        Err(format!(
            "it is {} x {} px instead of {} x {} px",
            decoded.0, decoded.1, size.0, size.1
        ))
    }
}
