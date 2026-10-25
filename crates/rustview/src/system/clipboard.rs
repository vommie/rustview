use super::error::SystemHandlerError;
use super::window_system::{self, Choice};
use arboard::{Clipboard, ImageData};
use image::ImageEncoder;
use parking_lot::Mutex;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::Az;
use std::borrow::Cow;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Instant;

static PERSISTENT_CLIPBOARD: LazyLock<Mutex<Option<Clipboard>>> = LazyLock::new(|| Mutex::new(Clipboard::new().ok()));

/// Detailed benchmark metrics recorded during image clipboard operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardImageMetrics {
    pub file_size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub io_read_us: u128,
    pub png_prep_us: u128,
    pub cli_pipe_us: u128,
    pub arboard_set_us: u128,
    pub total_us: u128,
    pub success: bool,
}

/// Centralized clipboard management subsystem for Linux desktop environments (X11 & Wayland).
/// Supports high-performance asynchronous textual and native image clipboard operations with full ICCCM INCR support.
pub struct SystemClipboard;

impl SystemClipboard {
    /// Detects the standard MIME type corresponding to the file extension (from the format table).
    pub fn mime_type_from_path(path: &Path) -> &'static str {
        rustview_core::media::MediaFormat::from_path(path).spec().mime
    }

    /// Decodes any image `RustView` can show to RGBA: the `image` crate first (with alpha), then the viewer's decoders
    /// (`FFmpeg` formats, SVG).
    fn decode_rgba(path: &Path) -> Result<(u32, u32, Vec<u8>), SystemHandlerError> {
        let native = if rustview_core::media::MediaFormat::from_path(path) == rustview_core::media::MediaFormat::Svg {
            None
        } else {
            rustview_core::media::image_decode::open_reader(path, 128 * 1024)
                .ok()
                .and_then(|reader| reader.decode().ok())
        };
        if let Some(img) = native {
            let rgba = img.into_rgba8();
            return Ok((rgba.width(), rgba.height(), rgba.into_raw()));
        }
        let (width, height, buffer) = crate::viewer::HighResImageLoader::load_full_resolution(path)
            .map_err(|e| SystemHandlerError::Clipboard(e.to_string()))?;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for px in buffer.as_bytes().as_chunks::<3>().0 {
            rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        Ok((width, height, rgba))
    }

    /// Copies arbitrary UTF-8 text into the system clipboard using the persistent clipboard daemon.
    pub fn set_text(text: &str) -> Result<(), SystemHandlerError> {
        let t0 = Instant::now();

        let mut arboard_success = false;

        // 1. Set on persistent arboard Clipboard
        {
            let mut guard = PERSISTENT_CLIPBOARD.lock();
            if guard.is_none() {
                *guard = Clipboard::new().ok();
            }
            if let Some(ref mut cb) = *guard {
                arboard_success = cb.set_text(text.to_string()).is_ok();
            }
        }

        // 2. Also pipe to system CLI clipboard tools
        let cli_success = Self::set_text_via_cli(text);

        if arboard_success || cli_success {
            tracing::debug!(
                len = text.len(),
                arboard = arboard_success,
                cli = cli_success,
                elapsed_us = t0.elapsed().as_micros(),
                "[Benchmark: ClipboardText] Text copied to clipboard successfully"
            );
            Ok(())
        } else {
            tracing::warn!("[SystemClipboard] All clipboard backends failed to set text");
            Err(SystemHandlerError::Clipboard(
                "Failed to set text to system clipboard".into(),
            ))
        }
    }

    /// Copies a list of filenames formatted with newline separators to the clipboard.
    pub fn copy_file_names<S: AsRef<str>>(names: &[S]) -> Result<usize, SystemHandlerError> {
        if names.is_empty() {
            return Ok(0);
        }
        let joined = names
            .iter()
            .map(std::convert::AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n");
        Self::set_text(&joined)?;
        Ok(names.len())
    }

    /// Copies a list of filesystem paths formatted with newline separators to the clipboard.
    pub fn copy_file_paths<P: AsRef<Path>>(paths: &[P]) -> Result<usize, SystemHandlerError> {
        if paths.is_empty() {
            return Ok(0);
        }
        let joined = paths
            .iter()
            .map(|p| p.as_ref().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        Self::set_text(&joined)?;
        Ok(paths.len())
    }

    /// Asynchronously copies an image file to the clipboard in a detached worker thread without blocking the UI.
    pub fn copy_image_from_path_async<F>(path: PathBuf, on_complete: F)
    where
        F: FnOnce(Result<(u32, u32, ClipboardImageMetrics), SystemHandlerError>) + Send + 'static,
    {
        std::thread::Builder::new()
            .name("clipboard-image-worker".into())
            .spawn(move || {
                let result = Self::copy_image_from_path_sync(&path);
                on_complete(result);
            })
            .expect("Failed to spawn clipboard image worker thread");
    }

    /// Reads an image from disk and sets it in the system clipboard with full `image/png` X11/Wayland INCR streaming support.
    /// Handles arbitrary image dimensions without freezing KDE Plasma or Krita.
    pub fn copy_image_from_path_sync(path: &Path) -> Result<(u32, u32, ClipboardImageMetrics), SystemHandlerError> {
        let t_total = Instant::now();
        let mut metrics = ClipboardImageMetrics::default();

        if !path.exists() {
            tracing::warn!(
                file = %LogObscurer::path(path),
                "[SystemClipboard] Cannot copy image: file does not exist on disk"
            );
            return Err(SystemHandlerError::FileNotFound(path.to_path_buf()));
        }

        // 1. Fast Buffered File Open & Metadata Probe
        let t_io = Instant::now();
        metrics.file_size_bytes = std::fs::metadata(path)?.len();
        let is_source_png = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"));
        metrics.io_read_us = t_io.elapsed().as_micros();

        // 2. Prepare standardized PNG file on disk for xclip/wl-copy daemon streaming
        let t_prep = Instant::now();
        let mut width = 0u32;
        let mut height = 0u32;

        let png_path: PathBuf = if is_source_png {
            if let Ok((w, h)) = image::image_dimensions(path) {
                width = w;
                height = h;
            }
            path.to_path_buf()
        } else {
            let temp_png = std::env::temp_dir().join("rustview_clipboard_image.png");
            let (decoded_width, decoded_height, raw) = Self::decode_rgba(path)?;
            width = decoded_width;
            height = decoded_height;

            let mut out_file = File::create(&temp_png)?;
            let encoder = image::codecs::png::PngEncoder::new_with_quality(
                &mut out_file,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::NoFilter,
            );
            ImageEncoder::write_image(encoder, &raw, width, height, image::ExtendedColorType::Rgba8)?;
            temp_png
        };
        metrics.width = width;
        metrics.height = height;
        metrics.png_prep_us = t_prep.elapsed().as_micros();

        // 3. Dispatch to the clipboard tool of the window system RustView runs on, using a direct file handle. Under XWayland the
        // X11 clipboard is handed on to Wayland programs by the compositor; wl-copy is the fallback there when xclip is missing.
        let t_cli = Instant::now();
        let cli_success = match window_system::current() {
            Choice::Wayland(_) => Self::set_image_via_wl_copy(&png_path),
            Choice::XWayland => Self::set_image_via_xclip(&png_path) || Self::set_image_via_wl_copy(&png_path),
            Choice::X11Session | Choice::NoDisplay => Self::set_image_via_xclip(&png_path),
        };
        metrics.cli_pipe_us = t_cli.elapsed().as_micros();

        // 4. Update arboard persistent instance as in-memory fallback
        let t_arboard = Instant::now();
        let mut arboard_success = false;
        let mut guard = PERSISTENT_CLIPBOARD.lock();
        if guard.is_none() {
            *guard = Clipboard::new().ok();
        }

        if let Some(ref mut cb) = *guard
            && let Ok(img) = image::open(&png_path)
        {
            let rgba = img.into_rgba8();
            let img_data = ImageData {
                width: rgba.width() as usize,
                height: rgba.height() as usize,
                bytes: Cow::Borrowed(rgba.as_raw()),
            };
            arboard_success = cb.set_image(img_data).is_ok();
        }
        metrics.arboard_set_us = t_arboard.elapsed().as_micros();

        metrics.total_us = t_total.elapsed().as_micros();
        metrics.success = cli_success || arboard_success;

        if metrics.success {
            tracing::info!(
                file = %LogObscurer::path(path),
                width = width,
                height = height,
                file_size_kb = format!("{:.2} KB", metrics.file_size_bytes.az::<f64>() / 1024.0),
                io_read_us = metrics.io_read_us,
                png_prep_ms = metrics.png_prep_us / 1000,
                cli_pipe_ms = metrics.cli_pipe_us / 1000,
                arboard_ms = metrics.arboard_set_us / 1000,
                total_time_ms = metrics.total_us / 1000,
                cli_ok = cli_success,
                arboard_ok = arboard_success,
                "[Benchmark: ClipboardImage] Image successfully placed onto system clipboard for Krita/GIMP"
            );
            Ok((width, height, metrics))
        } else {
            Err(SystemHandlerError::Clipboard(
                "Failed to copy image to system clipboard via all backends".into(),
            ))
        }
    }

    fn set_text_via_cli(text: &str) -> bool {
        // Try wl-copy (Wayland)
        if let Ok(mut child) = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }

        // Try xclip (X11)
        if let Ok(mut child) = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }

        // Try xsel (X11)
        if let Ok(mut child) = Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if child.wait().is_ok_and(|s| s.success()) {
                return true;
            }
        }

        false
    }

    fn set_image_via_xclip(png_path: &Path) -> bool {
        let path_str = png_path.to_string_lossy().to_string();
        if let Ok(mut child) = Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png", "-i", &path_str])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            return child.wait().is_ok_and(|s| s.success());
        }
        false
    }

    fn set_image_via_wl_copy(png_path: &Path) -> bool {
        let Ok(file) = File::open(png_path) else { return false };

        if let Ok(mut child) = Command::new("wl-copy")
            .args(["--type", "image/png"])
            .stdin(Stdio::from(file))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            return child.wait().is_ok_and(|s| s.success());
        }
        false
    }
}
