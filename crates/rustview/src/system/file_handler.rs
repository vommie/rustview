use super::error::SystemHandlerError;
use rustview_core::settings::LogObscurer;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Centralized Linux system default application launcher and file manager handler subsystem.
/// Dispatches media files and directories to desktop default handlers (e.g. xdg-open, dolphin, gio)
/// asynchronously without blocking the UI event loop.
pub struct SystemFileHandler;

impl SystemFileHandler {
    /// Preferred default application launcher binaries in order of priority.
    const DEFAULT_LAUNCHERS: &[&'static str] = &["xdg-open", "gio", "kde-open5", "kde-open", "open"];

    /// Opens the specified media file with the Linux system default application asynchronously.
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

        Self::spawn_default_app(canonical_path)
    }

    /// Opens a folder directly in the file manager (e.g. Dolphin).
    pub fn open_folder(folder_path: impl AsRef<Path>) -> Result<(), SystemHandlerError> {
        let p = folder_path.as_ref();

        if !p.exists() {
            tracing::warn!(
                directory = %LogObscurer::path(p),
                "[SystemFileHandler] Cannot open directory: path does not exist on disk"
            );
            return Err(SystemHandlerError::FileNotFound(p.to_path_buf()));
        }

        let canonical_dir = match p.canonicalize() {
            Ok(cp) => cp,
            Err(_) => p.to_path_buf(),
        };

        Self::spawn_folder_in_file_manager(canonical_dir)
    }

    /// Opens a directory directly in the file manager (alias for `open_folder`).
    #[inline]
    pub fn open_directory(dir_path: impl AsRef<Path>) -> Result<(), SystemHandlerError> {
        Self::open_folder(dir_path)
    }

    /// Opens the containing folder in the file manager with the specified file(s) highlighted/selected.
    pub fn reveal_in_file_manager<P: AsRef<Path>>(paths: &[P]) -> Result<(), SystemHandlerError> {
        if paths.is_empty() {
            return Ok(());
        }

        let mut valid_canonical_paths = Vec::with_capacity(paths.len());
        for p in paths {
            let path_ref = p.as_ref();
            if path_ref.exists() {
                let canonical = match path_ref.canonicalize() {
                    Ok(cp) => cp,
                    Err(_) => path_ref.to_path_buf(),
                };
                valid_canonical_paths.push(canonical);
            }
        }

        if valid_canonical_paths.is_empty() {
            let first = paths[0].as_ref().to_path_buf();
            tracing::warn!(
                file = %LogObscurer::path(&first),
                "[SystemFileHandler] Cannot reveal items: none of the specified paths exist on disk"
            );
            return Err(SystemHandlerError::FileNotFound(first));
        }

        Self::spawn_reveal_in_file_manager(valid_canonical_paths)
    }

    /// Single-file convenience wrapper for `reveal_in_file_manager`.
    pub fn reveal_single_in_file_manager(path: impl AsRef<Path>) -> Result<(), SystemHandlerError> {
        Self::reveal_in_file_manager(&[path])
    }

    fn spawn_default_app(target_path: PathBuf) -> Result<(), SystemHandlerError> {
        let launcher = Self::find_available_launcher(Self::DEFAULT_LAUNCHERS)
            .unwrap_or("xdg-open");

        tracing::info!(
            target = %LogObscurer::path(&target_path),
            launcher = launcher,
            "[SystemFileHandler] Spawning default application handler..."
        );

        let target_clone = target_path.clone();
        let launcher_owned = launcher.to_string();

        std::thread::Builder::new()
            .name("sys-launcher-open".into())
            .spawn(move || {
                let t_spawn = Instant::now();
                let mut cmd = Command::new(&launcher_owned);

                if launcher_owned == "gio" {
                    cmd.arg("open");
                }

                cmd.arg(&target_clone);
                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                match cmd.status() {
                    Ok(status) if status.success() => {
                        tracing::info!(
                            launcher = launcher_owned,
                            target = %LogObscurer::path(&target_clone),
                            elapsed_ms = t_spawn.elapsed().as_millis(),
                            "[SystemFileHandler] Successfully launched default application"
                        );
                    }
                    Ok(status) => {
                        tracing::warn!(
                            launcher = launcher_owned,
                            target = %LogObscurer::path(&target_clone),
                            exit_code = ?status.code(),
                            elapsed_ms = t_spawn.elapsed().as_millis(),
                            "[SystemFileHandler] Launcher exited with non-zero status code"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            launcher = launcher_owned,
                            target = %LogObscurer::path(&target_clone),
                            error = %e,
                            "[SystemFileHandler] Failed to execute launcher process"
                        );
                    }
                }
            })
            .map_err(SystemHandlerError::Io)?;

        Ok(())
    }

    fn spawn_folder_in_file_manager(folder_path: PathBuf) -> Result<(), SystemHandlerError> {
        tracing::info!(
            folder = %LogObscurer::path(&folder_path),
            "[SystemFileHandler] Spawning folder in file manager (Dolphin)..."
        );

        let target_clone = folder_path.clone();

        std::thread::Builder::new()
            .name("sys-launcher-folder".into())
            .spawn(move || {
                let t_spawn = Instant::now();

                // 1. Primary: Try Dolphin directly
                let dolphin_res = Command::new("dolphin")
                    .arg(&target_clone)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();

                if let Ok(mut child) = dolphin_res
                    && child.wait().is_ok_and(|s| s.success())
                {
                    tracing::info!(
                        folder = %LogObscurer::path(&target_clone),
                        elapsed_ms = t_spawn.elapsed().as_millis(),
                        "[SystemFileHandler] Opened folder in Dolphin"
                    );
                    return;
                }

                // 2. Fallback: xdg-open
                let mut fallback_cmd = Command::new("xdg-open");
                fallback_cmd.arg(&target_clone);
                fallback_cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                if let Ok(mut child) = fallback_cmd.spawn() {
                    let _ = child.wait();
                    tracing::info!(
                        folder = %LogObscurer::path(&target_clone),
                        elapsed_ms = t_spawn.elapsed().as_millis(),
                        "[SystemFileHandler] Opened folder via xdg-open fallback"
                    );
                }
            })
            .map_err(SystemHandlerError::Io)?;

        Ok(())
    }

    fn spawn_reveal_in_file_manager(target_paths: Vec<PathBuf>) -> Result<(), SystemHandlerError> {
        tracing::info!(
            count = target_paths.len(),
            first_target = %LogObscurer::path(&target_paths[0]),
            "[SystemFileHandler] Revealing file(s) with pre-selection in Dolphin..."
        );

        std::thread::Builder::new()
            .name("sys-launcher-reveal".into())
            .spawn(move || {
                let t_spawn = Instant::now();

                // 1. Primary: Dolphin --select <paths...>
                let mut dolphin_cmd = Command::new("dolphin");
                dolphin_cmd.arg("--select");
                for p in &target_paths {
                    dolphin_cmd.arg(p);
                }
                dolphin_cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                if let Ok(mut child) = dolphin_cmd.spawn()
                    && child.wait().is_ok_and(|s| s.success())
                {
                    tracing::info!(
                        count = target_paths.len(),
                        elapsed_ms = t_spawn.elapsed().as_millis(),
                        "[SystemFileHandler] Successfully revealed file(s) in Dolphin"
                    );
                    return;
                }

                // 2. Fallback: Nautilus --select / Nemo --select
                for fm in &["nautilus", "nemo"] {
                    let mut fm_cmd = Command::new(fm);
                    fm_cmd.arg("--select");
                    for p in &target_paths {
                        fm_cmd.arg(p);
                    }
                    fm_cmd
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    if let Ok(mut child) = fm_cmd.spawn()
                        && child.wait().is_ok_and(|s| s.success())
                    {
                        tracing::info!(
                            manager = fm,
                            count = target_paths.len(),
                            elapsed_ms = t_spawn.elapsed().as_millis(),
                            "[SystemFileHandler] Revealed file(s) in fallback file manager"
                        );
                        return;
                    }
                }

                // 3. Fallback: Open parent directory via xdg-open
                if let Some(parent) = target_paths[0].parent() {
                    let mut xdg_cmd = Command::new("xdg-open");
                    xdg_cmd.arg(parent);
                    xdg_cmd
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    if let Ok(mut child) = xdg_cmd.spawn() {
                        let _ = child.wait();
                        tracing::info!(
                            parent = %LogObscurer::path(parent),
                            elapsed_ms = t_spawn.elapsed().as_millis(),
                            "[SystemFileHandler] Opened containing directory via xdg-open"
                        );
                    }
                }
            })
            .map_err(SystemHandlerError::Io)?;

        Ok(())
    }

    fn find_available_launcher(launchers: &[&'static str]) -> Option<&'static str> {
        for &launcher in launchers {
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
        None
    }
}
