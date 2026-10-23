//! The XDND drop on another window and the external drop paths.

use super::{DndExportMetrics, SystemDndHandler};
use crate::system::error::SystemHandlerError;
use crate::viewer::x11::drop_on_window_under_pointer;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::Az;
use std::path::PathBuf;
use std::time::Instant;

impl SystemDndHandler {
    /// Asynchronously performs the external Drop transaction into the 3rd-party window under the cursor.
    pub fn execute_external_drop_async(paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }

        let paths_clone = paths.to_vec();
        std::thread::Builder::new()
            .name("sys-external-drop".into())
            .spawn(move || {
                let _ = Self::execute_external_drop_sync(&paths_clone);
            })
            .expect("Failed to spawn external drop worker thread");
    }

    /// Synchronously executes the XDND drop transaction on the target window under the global mouse cursor [2.4.1].
    pub fn execute_external_drop_sync(paths: &[PathBuf]) -> Result<DndExportMetrics, SystemHandlerError> {
        let t0 = Instant::now();
        let session_type = Self::detect_session_type();

        let mut canonical_paths = Vec::with_capacity(paths.len());
        let mut total_bytes = 0u64;

        for p in paths {
            if p.exists() {
                let cp = match p.canonicalize() {
                    Ok(c) => c,
                    Err(_) => p.clone(),
                };
                if let Ok(meta) = std::fs::metadata(&cp) {
                    total_bytes += meta.len();
                }
                canonical_paths.push(cp);
            }
        }

        if canonical_paths.is_empty() {
            return Err(SystemHandlerError::FileNotFound(
                paths.first().cloned().unwrap_or_default(),
            ));
        }

        let files_count = canonical_paths.len();

        tracing::info!(
            session_type = session_type,
            files_count = files_count,
            total_kb = format!("{:.2} KB", total_bytes.az::<f64>() / 1024.0),
            first_file = %LogObscurer::path(&canonical_paths[0]),
            "[SystemDndHandler: Drop] Executing external XDND drop transaction on window under cursor..."
        );

        let (success, target_window_id, target_app_class) = Self::perform_xdnd_drop_transaction(&canonical_paths);

        let elapsed_us = t0.elapsed().as_micros();
        let metrics = DndExportMetrics {
            files_count,
            total_bytes,
            session_type,
            target_window_id,
            target_app_class,
            elapsed_us,
            success,
        };

        tracing::info!(
            target_window_id = metrics.target_window_id,
            target_app = %metrics.target_app_class,
            success = metrics.success,
            elapsed_ms = metrics.elapsed_us.az::<f64>() / 1000.0,
            "[Benchmark: ExternalDrop] XDND Drop transaction completed"
        );

        Ok(metrics)
    }

    /// Performs the XDND drop on the window under the pointer (see `viewer::x11::xdnd`). Returns whether a target took the drop, its window
    /// id and its application class.
    pub(super) fn perform_xdnd_drop_transaction(paths: &[PathBuf]) -> (bool, u64, String) {
        let uri_list = Self::format_uri_list(paths);
        let plain_text = format!("{}\n", Self::format_plain_paths(paths));
        match drop_on_window_under_pointer(uri_list.as_bytes(), plain_text.as_bytes()) {
            Ok(outcome) => (
                outcome.target_window.is_some(),
                outcome.target_window.unwrap_or(0),
                outcome.app_class,
            ),
            Err(error) => {
                tracing::warn!(%error, "[SystemDndHandler: Drop] The XDND drop could not be performed");
                (false, 0, "unknown".to_owned())
            }
        }
    }
}
