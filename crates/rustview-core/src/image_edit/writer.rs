//! Writing the new file so that the original is never damaged, whatever goes wrong (only on the user's explicit save).
//!
//! The new bytes go to a temporary file next to the original (same directory, so the final rename is atomic), are flushed
//! to the disk and read back; only then is the original replaced, by a rename: at every moment the path holds either the
//! complete old file or the complete new one. Permissions and, where allowed, the owner of the original are kept. How the
//! original is kept until the new file is in place is the user's choice (`OverwriteMode`):
//! * `Safe`: a backup (a hard link, or a copy where links are not possible) next to it, removed once the new file was read
//!   back from its final place; on any failure after the rename the backup is put back.
//! * `Trash`: the original goes to the trash, then the new file takes its place.
//! * `Direct`: the rename alone.
//!
//! A symbolic link is not replaced: the file it points to is written (the link keeps working). A read-only file is not
//! overwritten. The identity of the file is checked once more right before it is replaced.

use super::conflict::{DiskState, FileIdentity, check};
use super::options::OverwriteMode;
use crate::settings::LogObscurer;
use atomic_write_file::AtomicWriteFile;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WriteError {
    #[error("the file changed on disk")]
    Changed(DiskState),
    #[error("the file is read-only")]
    ReadOnly,
    #[error("a file with this name exists already")]
    Exists,
    #[error("the folder does not exist")]
    NoFolder,
    #[error("the new file could not be read back correctly")]
    VerifyFailed,
    #[error("the original could not be moved to the trash: {0}")]
    Trash(String),
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// What happened on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteOutcome {
    /// The file that was written (the target of a symbolic link).
    pub written: PathBuf,
    pub identity: FileIdentity,
}

/// What may be at the path that is overwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    /// The file the edit was made on: refused when it changed in the meantime.
    Unchanged(Option<FileIdentity>),
    /// Whatever is there (the user chose to overwrite anyway); a missing file is created again.
    Anything,
}

/// Replaces the file at `path` with `bytes`.
pub fn overwrite(path: &Path, bytes: &[u8], mode: OverwriteMode, expect: Expect) -> Result<WriteOutcome, WriteError> {
    overwrite_using(path, bytes, mode, expect, &|p| {
        trash::delete(p).map_err(|e| e.to_string())
    })
}

/// `overwrite` with the function that moves a file to the trash (tests use a folder of their own).
pub fn overwrite_using(
    path: &Path,
    bytes: &[u8],
    mode: OverwriteMode,
    expect: Expect,
    to_trash: &dyn Fn(&Path) -> Result<(), String>,
) -> Result<WriteOutcome, WriteError> {
    let target = resolve_target(path)?;
    let existing = fs::metadata(&target).ok();
    if let Some(meta) = &existing {
        if !meta.is_file() {
            return Err(WriteError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            )));
        }
        if meta.permissions().readonly() {
            return Err(WriteError::ReadOnly);
        }
        if std::os::unix::fs::MetadataExt::nlink(meta) > 1 {
            tracing::warn!(
                path = %LogObscurer::path(&target),
                "[ImageEdit: Write] The file has other hard links; they keep the old picture"
            );
        }
    }
    let file = write_temporary(&target, bytes)?;
    // The last look before the original is replaced: nothing may have changed while the picture was encoded.
    if let Expect::Unchanged(identity) = expect {
        let state = check(&target, identity);
        if state.is_conflict() {
            file.discard()?;
            return Err(WriteError::Changed(state));
        }
    }
    let mode = if existing.is_none() {
        OverwriteMode::Direct
    } else {
        mode
    };
    match mode {
        OverwriteMode::Direct => {
            file.commit()?;
            verify_final(&target, bytes)?;
        }
        OverwriteMode::Trash => {
            if let Err(e) = to_trash(&target) {
                file.discard()?;
                return Err(WriteError::Trash(e));
            }
            file.commit()?;
            verify_final(&target, bytes)?;
        }
        OverwriteMode::Safe => commit_with_backup(file, &target, bytes)?,
    }
    let identity = FileIdentity::read(&target)?;
    tracing::info!(
        path = %LogObscurer::path(&target),
        bytes = bytes.len(),
        mode = mode.as_str(),
        "[ImageEdit: Write] File replaced"
    );
    Ok(WriteOutcome {
        written: target,
        identity,
    })
}

/// Writes a new file at `path`; refused when something is there already.
pub fn create_new(path: &Path, bytes: &[u8]) -> Result<WriteOutcome, WriteError> {
    if fs::symlink_metadata(path).is_ok() {
        return Err(WriteError::Exists);
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !parent.is_dir() {
        return Err(WriteError::NoFolder);
    }
    let file = write_temporary(path, bytes)?;
    file.commit()?;
    verify_final(path, bytes)?;
    let identity = FileIdentity::read(path)?;
    tracing::info!(
        path = %LogObscurer::path(path),
        bytes = bytes.len(),
        "[ImageEdit: Write] New file written"
    );
    Ok(WriteOutcome {
        written: path.to_path_buf(),
        identity,
    })
}

/// The file a path stands for: the target of a symbolic link (also of a link whose target is gone: that file is created).
fn resolve_target(path: &Path) -> Result<PathBuf, WriteError> {
    let is_link = fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink());
    if !is_link {
        return Ok(path.to_path_buf());
    }
    if let Ok(target) = fs::canonicalize(path) {
        return Ok(target);
    }
    let link = fs::read_link(path)?;
    Ok(if link.is_absolute() {
        link
    } else {
        path.parent().unwrap_or(Path::new(".")).join(link)
    })
}

/// The new bytes in a temporary file next to `target`, flushed to the disk and read back.
fn write_temporary(target: &Path, bytes: &[u8]) -> Result<AtomicWriteFile, WriteError> {
    let mut file = AtomicWriteFile::options().read(true).open(target)?;
    file.write_all(bytes)?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.seek(SeekFrom::Start(0))?;
    let mut back = Vec::with_capacity(bytes.len());
    file.read_to_end(&mut back)?;
    if back != bytes {
        file.discard()?;
        return Err(WriteError::VerifyFailed);
    }
    Ok(file)
}

fn verify_final(target: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    if fs::read(target)? == bytes {
        Ok(())
    } else {
        Err(WriteError::VerifyFailed)
    }
}

/// The name of the backup of `target` while it is replaced (hidden, next to it).
pub fn backup_path(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    target.with_file_name(format!(".{name}.rustview-backup"))
}

fn commit_with_backup(file: AtomicWriteFile, target: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    let backup = backup_path(target);
    let _ = fs::remove_file(&backup);
    if fs::hard_link(target, &backup).is_err() {
        fs::copy(target, &backup)?;
        fs::File::open(&backup)?.sync_all()?;
    }
    if let Err(e) = file.commit() {
        let _ = fs::remove_file(&backup);
        return Err(e.into());
    }
    match verify_final(target, bytes) {
        Ok(()) => {
            fs::remove_file(&backup)?;
            Ok(())
        }
        Err(e) => {
            tracing::error!(
                path = %LogObscurer::path(target),
                "[ImageEdit: Write] The new file did not read back correctly; the original is restored"
            );
            fs::rename(&backup, target)?;
            Err(e)
        }
    }
}
