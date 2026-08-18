use super::error::SystemHandlerError;
use crate::settings::LogObscurer;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Centralized Linux system default application launcher and file handler subsystem.
/// Dispatches media files and directories to desktop default handlers (e.g. xdg-open, gio open)
/// asynchronously without blocking the UI event loop.
pub struct SystemFileHandler;

impl SystemFileHandler {
    /// Preferred launcher binary commands on Linux / Unix desktops in order of priority.
    const LAUNCHERS: &'static [&'static str] = &["xdg-open", "gio", "kde-open5", "kde-open", "open"];

    /// Opens the specified media file with the Linux system default application asynchronously.
    ///
    /// # Behavior
    /// 1. Validates that the file physically exists on storage.
    /// 2. Resolves and normalizes the target path.
    /// 3. Spawns the desktop launcher in a detached background thread with decoupled standard I/O streams.
    /// 4. Logs diagnostics with privacy-respecting path obfuscation.
    pub fn open_file(path: impl AsRef<Path>) -> Result<(), SystemHandlerError> {
        let p = path.as_ref();

        if !p.exists() {
            tracing::warn!(
                file = %LogObscurer::path(p),
                "[SystemFileHandler] Cannot open file: path does not exist on disk"
            );
            return Err(SystemHandlerError::FileNotFound(p.to_path_buf()));
        }

        let canonical_path = match p.canonicalize() {
            Ok(cp) => cp,
            Err(_) => p.to_path_buf(),
        };

        Self::spawn_launcher(canonical_path, "open_file")
    }

    /// Opens the parent directory containing the file or opens the directory itself in the default file manager.
    pub fn open_directory(path: impl AsRef<Path>) -> Result<(), SystemHandlerError> {
        let p = path.as_ref();
        let target_dir = if p.is_dir() {
            p.to_path_buf()
        } else if let Some(parent) = p.parent() {
            parent.to_path_buf()
        } else {
            p.to_path_buf()
        };

        if !target_dir.exists() {
            tracing::warn!(
                directory = %LogObscurer::path(&target_dir),
                "[SystemFileHandler] Cannot open directory: path does not exist on disk"
            );
            return Err(SystemHandlerError::FileNotFound(target_dir));
        }

        let canonical_dir = match target_dir.canonicalize() {
            Ok(cp) => cp,
            Err(_) => target_dir,
        };

        Self::spawn_launcher(canonical_dir, "open_directory")
    }

    /// Internal asynchronous launcher execution in a detached worker thread.
    fn spawn_launcher(
        target_path: PathBuf,
        context_label: &'static str,
    ) -> Result<(), SystemHandlerError> {
        let t0 = Instant::now();

        tracing::info!(
            target = %LogObscurer::path(&target_path),
            context = context_label,
            "[SystemFileHandler] Spawning asynchronous system default handler..."
        );

        // Pre-check launcher availability to fail fast if no launcher is installed
        let candidate_launcher = Self::find_available_launcher()
            .ok_or(SystemHandlerError::NoHandlerAvailable)?;

        let target_clone = target_path.clone();

        std::thread::Builder::new()
            .name(format!("sys-launcher-{}", context_label))
            .spawn(move || {
                let t_spawn = Instant::now();

                let mut cmd = Command::new(candidate_launcher);

                // For 'gio', the subcommand 'open' is required
                if candidate_launcher == "gio" {
                    cmd.arg("open");
                }

                cmd.arg(&target_clone);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match cmd.status() {
                    Ok(status) => {
                        let elapsed_ms = t_spawn.elapsed().as_millis();
                        if status.success() {
                            tracing::info!(
                                launcher = candidate_launcher,
                                target = %LogObscurer::path(&target_clone),
                                elapsed_ms = elapsed_ms,
                                "[SystemFileHandler] Successfully launched default application"
                            );
                        } else {
                            tracing::warn!(
                                launcher = candidate_launcher,
                                target = %LogObscurer::path(&target_clone),
                                exit_code = ?status.code(),
                                elapsed_ms = elapsed_ms,
                                "[SystemFileHandler] Launcher exited with non-zero status code"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            launcher = candidate_launcher,
                            target = %LogObscurer::path(&target_clone),
                            error = %e,
                            "[SystemFileHandler] Failed to execute launcher process"
                        );
                    }
                }
            })
            .map_err(SystemHandlerError::Io)?;

        tracing::debug!(
            target = %LogObscurer::path(&target_path),
            launcher = candidate_launcher,
            dispatch_us = t0.elapsed().as_micros(),
            "[SystemFileHandler] Dispatch request handed off to background process successfully"
        );

        Ok(())
    }

    /// Scans system PATH for the first available launcher binary.
    fn find_available_launcher() -> Option<&'static str> {
        for &launcher in Self::LAUNCHERS {
            let which_found = Command::new("which")
                .arg(launcher)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|s| s.success());

            if which_found {
                return Some(launcher);
            }

            let direct_probe = Command::new(launcher)
                .arg("--version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok();

            if direct_probe {
                return Some(launcher);
            }
        }
        Some("xdg-open")
    }
}
