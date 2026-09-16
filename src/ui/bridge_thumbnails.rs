use super::MainWindow;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use crate::media::formats::{approximate_aspect_ratio, format_duration_hms_ms};
use crate::media::MediaType;
use crate::telemetry::BenchmarkReportFormatter;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{Image, Model, Timer};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

/// Subsystem managing background thumbnail worker draining, UI status bar reflection, and live busy-state propagation.
pub struct BridgeThumbnails;

impl BridgeThumbnails {
    /// Starts high-performance 60 FPS repeated timer to drain pre-converted SIMD thumbnails into RAM cache and visible rows.
    pub fn start_drain_timer(
        thumbnails: Arc<ThumbnailManager>,
        state: Arc<RwLock<UiState>>,
        window_weak: slint::Weak<MainWindow>,
    ) -> Timer {
        let timer = Timer::default();
        let thumb_ref = thumbnails.clone();
        let st_ref = state.clone();

        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(16),
            move || {
                let Some(window) = window_weak.upgrade() else {
                    return;
                };

                // Keep window.is_system_busy continuously synchronized with high precision
                let is_busy = BridgeMedia::is_system_busy(&window, &thumb_ref, &st_ref);
                if window.get_is_system_busy() != is_busy {
                    window.set_is_system_busy(is_busy);
                }

                let t_drain_start = Instant::now();
                let batch = thumb_ref.drain_ready_batch(64);

                let mut row_set_us = 0u64;
                let items_drained = batch.len();
                let is_table = window.get_is_table_view();

                if !batch.is_empty() {
                    let mut st = st_ref.write();
                    let media_model = window.get_media_items();
                    let start_window = window.get_visible_start_index() as usize;
                    let visible_count = media_model.row_count();

                    for item_data in batch {
                        let is_folder_match = st.is_query_active
                            || st.is_virtual_folder_mode
                            || st.current_folder_id == Some(item_data.folder_id);

                        if !is_folder_match {
                            continue;
                        }

                        if let Some(ref buf) = item_data.pixel_buffer {
                            st.folder_thumbnails.insert(item_data.media_id, buf.clone());
                        }

                        let mut meta_changed = false;
                        if (item_data.orig_width > 0 || item_data.orig_height > 0 || item_data.duration_ms > 0 || item_data.is_corrupted)
                            && let Some(item) = st.all_folder_media.iter_mut().find(|m| m.record.id == item_data.media_id)
                        {
                            if item_data.orig_width > 0 && item.record.width == 0 {
                                item.record.width = item_data.orig_width;
                                item.width_str = item_data.orig_width.to_string().into();
                                meta_changed = true;
                            }
                            if item_data.orig_height > 0 && item.record.height == 0 {
                                item.record.height = item_data.orig_height;
                                item.height_str = item_data.orig_height.to_string().into();
                                meta_changed = true;
                            }
                            if item_data.duration_ms > 0 && item.record.duration_ms == 0 {
                                item.record.duration_ms = item_data.duration_ms;
                                let is_video = matches!(item.record.media_type, MediaType::Video);
                                item.duration_str = format_duration_hms_ms(item_data.duration_ms, is_video).into();
                                meta_changed = true;
                            }
                            if item_data.is_corrupted != item.is_corrupted {
                                item.record.is_corrupted = item_data.is_corrupted;
                                item.is_corrupted = item_data.is_corrupted;
                                item.flags_str = if item.record.is_deleted {
                                    "Deleted".into()
                                } else if item.record.is_corrupted {
                                    "Corrupted".into()
                                } else {
                                    "OK".into()
                                };
                                meta_changed = true;
                            }
                            if meta_changed {
                                item.ar_str = approximate_aspect_ratio(item.record.width, item.record.height).into();
                                item.ar_value = if item.record.height > 0 { item.record.width as f32 / item.record.height as f32 } else { 0.0 };
                            }
                        }

                        let t_row = Instant::now();

                        if let Some(&global_row_idx) = st.media_row_map.get(&item_data.media_id)
                            && global_row_idx >= start_window
                            && global_row_idx < start_window + visible_count
                        {
                            let local_idx = global_row_idx - start_window;
                            if let Some(mut item) = media_model.row_data(local_idx)
                                && item.id == item_data.media_id as i32
                            {
                                if !is_table {
                                    if let Some(ref buf) = item_data.pixel_buffer {
                                        item.thumbnail = Image::from_rgb8(buf.clone());
                                    } else {
                                        item.thumbnail = Image::default();
                                    }
                                }

                                if let Some(updated_rec) = st.all_folder_media.iter().find(|m| m.record.id == item_data.media_id) {
                                    item.width_str = updated_rec.width_str.clone();
                                    item.height_str = updated_rec.height_str.clone();
                                    item.ar_str = updated_rec.ar_str.clone();
                                    item.duration_str = updated_rec.duration_str.clone();
                                    item.flags_str = updated_rec.flags_str.clone();
                                    item.is_corrupted = updated_rec.is_corrupted;
                                    item.is_deleted = updated_rec.is_deleted;
                                }

                                media_model.set_row_data(local_idx, item);
                            }
                        }
                        row_set_us += t_row.elapsed().as_micros() as u64;
                    }
                }

                let total_drain_us = t_drain_start.elapsed().as_micros() as u64;

                if items_drained > 0
                    && let Some(bench) = thumb_ref.active_benchmark()
                {
                    bench.record_drain_cycle(
                        items_drained,
                        row_set_us,
                        total_drain_us,
                    );
                }

                let progress = thumb_ref.progress();
                StatusBarBridge::update_thumbnail_progress(&window, &progress);

                if let Some(bench) = thumb_ref.active_benchmark() {
                    let total_targets = bench.total_targets.load(Ordering::Acquire);
                    let drained_items = bench.drained_items.load(Ordering::Acquire);

                    let is_active_started = progress.preload_total > 0 || progress.generate_total > 0;
                    let is_done = total_targets > 0
                        && ((drained_items >= total_targets)
                            || (is_active_started
                                && progress.preload_current >= progress.preload_total
                                && progress.generate_current >= progress.generate_total
                                && progress.active_workers == 0
                                && thumb_ref.is_drain_queue_empty()));

                    if is_done
                        && !bench.is_finalized.swap(true, Ordering::SeqCst)
                    {
                        let folder_bench = st_ref.read().active_folder_bench.clone();
                        let storage_stats = thumb_ref.storage_stats();
                        let summary_str = BenchmarkReportFormatter::render_summary(
                            folder_bench.as_deref(),
                            &bench,
                            &storage_stats,
                        );
                        tracing::info!("{}", summary_str);
                    }
                }
            },
        );
        timer
    }
}
