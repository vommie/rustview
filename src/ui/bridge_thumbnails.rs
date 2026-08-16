use super::MainWindow;
use super::bridge_state::UiState;
use super::models::rgb8_to_shared_buffer;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{Image, Model, Timer};
use std::sync::Arc;

/// Subsystem managing background thumbnail worker draining and UI status bar reflection.
pub struct BridgeThumbnails;

impl BridgeThumbnails {
    /// Starts repeated timer to drain decoded SIMD thumbnails into RAM cache and visible rows.
    pub fn start_drain_timer(
        thumbnails: Arc<ThumbnailManager>,
        state: Arc<RwLock<UiState>>,
        window_weak: slint::Weak<MainWindow>,
    ) -> Timer {
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(25),
            move || {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };

                let batch = thumbnails.drain_ready_batch(64);
                if !batch.is_empty() {
                    let mut st = state.write();
                    let media_model = window.get_media_items();

                    for item_data in batch {
                        let shared_buf = rgb8_to_shared_buffer(
                            &item_data.rgb8_pixels,
                            item_data.width,
                            item_data.height,
                        );

                        st.folder_thumbnails
                            .insert(item_data.media_id, shared_buf.clone());

                        if let Some(&row_idx) = st.media_row_map.get(&item_data.media_id)
                            && let Some(mut item) = media_model.row_data(row_idx)
                        {
                            item.thumbnail = Image::from_rgb8(shared_buf);
                            media_model.set_row_data(row_idx, item);
                        }
                    }
                }

                let progress = thumbnails.progress();
                window.set_is_preloading(progress.is_preloading);
                window.set_preload_current(progress.preload_current as i32);
                window.set_preload_total(progress.preload_total as i32);

                window.set_is_generating(progress.is_generating);
                window.set_generate_current(progress.generate_current as i32);
                window.set_generate_total(progress.generate_total as i32);
                window.set_active_workers(progress.active_workers as i32);
            },
        );
        timer
    }
}
