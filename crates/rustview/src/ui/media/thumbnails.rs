use crate::ui::MainWindow;
use crate::ui::browse_window::{apply_record, update_tile};
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::thumbnail_regeneration::ThumbnailRegeneration;
use parking_lot::RwLock;
use rustview_core::telemetry::BenchmarkReportFormatter;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::SaturatingAs;
use slint::{Image, Timer};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Subsystem managing background thumbnail draining, UI status bar reflection, and live busy-state propagation.
pub struct BridgeThumbnails;

/// How often the status housekeeping (busy flag, progress, benchmark completion) runs while something is going on, and while
/// the application is idle. Thumbnails themselves are drained when the workers announce them, not by this timer.
const HOUSEKEEPING_ACTIVE: Duration = Duration::from_millis(100);
const HOUSEKEEPING_IDLE: Duration = Duration::from_millis(1000);
/// Thumbnails applied to tiles per event loop iteration; more waiting items get a further iteration, so input and drawing are
/// never starved by a large burst.
const DRAIN_BATCH: usize = 64;

impl BridgeThumbnails {
    /// Installs the event-driven drain (workers announce ready thumbnails, the UI thread applies them) and starts the slow
    /// housekeeping timer. The returned timer must be kept alive.
    pub fn start_drain_timer(
        thumbnails: Arc<ThumbnailManager>,
        state: Arc<RwLock<UiState>>,
        window_weak: slint::Weak<MainWindow>,
    ) -> Rc<Timer> {
        let scheduled = Arc::new(AtomicBool::new(false));
        {
            let (thumbnails, state, window_weak, scheduled) = (
                thumbnails.clone(),
                state.clone(),
                window_weak.clone(),
                scheduled.clone(),
            );
            let announce_thumbnails = thumbnails.clone();
            announce_thumbnails.set_ready_notifier(Arc::new(move || {
                Self::schedule_drain(&thumbnails, &state, &window_weak, &scheduled);
            }));
        }

        let timer = Rc::new(Timer::default());
        let own_timer = timer.clone();
        timer.start(slint::TimerMode::Repeated, HOUSEKEEPING_IDLE, move || {
            if let Some(window) = window_weak.upgrade() {
                let active = Self::housekeeping(&window, &thumbnails, &state);
                own_timer.set_interval(if active { HOUSEKEEPING_ACTIVE } else { HOUSEKEEPING_IDLE });
            }
        });
        timer
    }

    /// Posts one drain to the UI thread unless one is already posted (many thumbnails announced in a burst cost one wake-up).
    fn schedule_drain(
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
        window_weak: &slint::Weak<MainWindow>,
        scheduled: &Arc<AtomicBool>,
    ) {
        if scheduled.swap(true, Ordering::AcqRel) {
            return;
        }
        let (thumbnails, state, weak, flag) = (
            thumbnails.clone(),
            state.clone(),
            window_weak.clone(),
            scheduled.clone(),
        );
        let posted = window_weak.upgrade_in_event_loop(move |window| {
            flag.store(false, Ordering::Release);
            let more_waiting = Self::drain_once(&window, &thumbnails, &state);
            Self::housekeeping(&window, &thumbnails, &state);
            if more_waiting {
                Self::schedule_drain(&thumbnails, &state, &weak, &flag);
            }
        });
        if posted.is_err() {
            scheduled.store(false, Ordering::Release);
        }
    }

    /// Applies one batch of ready thumbnails to the tiles; returns whether more are waiting.
    fn drain_once(window: &MainWindow, thumb_ref: &ThumbnailManager, st_ref: &RwLock<UiState>) -> bool {
        let t_drain_start = Instant::now();
        let batch = thumb_ref.drain_ready_batch(DRAIN_BATCH);

        let mut row_set_us = 0u64;
        // Set when a video turns corrupted: the random-position regeneration controls must re-evaluate.
        let mut regeneration_state_dirty = false;
        let items_drained = batch.len();
        let is_table = window.get_is_table_view();

        if !batch.is_empty() {
            let mut st = st_ref.write();

            for item_data in batch {
                let media_id = item_data.media_id;
                let mut metadata_changed = false;

                if (item_data.orig_width > 0
                    || item_data.orig_height > 0
                    || item_data.duration_ms > 0
                    || item_data.is_corrupted
                    || item_data.is_animated)
                    && let Some(item) = st.all_folder_media.find_mut(media_id)
                {
                    if item_data.orig_width > 0 && item.record.width == 0 {
                        item.record.width = item_data.orig_width;
                        metadata_changed = true;
                    }
                    if item_data.orig_height > 0 && item.record.height == 0 {
                        item.record.height = item_data.orig_height;
                        metadata_changed = true;
                    }
                    if item_data.duration_ms > 0 && item.record.duration_ms == 0 {
                        item.record.duration_ms = item_data.duration_ms;
                        metadata_changed = true;
                    }
                    if item_data.is_animated && !item.is_animated {
                        item.is_animated = true;
                        metadata_changed = true;
                    }
                    if item_data.is_corrupted != item.is_corrupted {
                        regeneration_state_dirty = true;
                        item.record.is_corrupted = item_data.is_corrupted;
                        metadata_changed = true;
                    }
                    if metadata_changed {
                        item.refresh_derived();
                    }
                }

                let t_row = Instant::now();
                let picture = item_data.pixel_buffer.map(Image::from_rgb8);
                let record = st.record_by_id(media_id);
                update_tile(window, &st, media_id, |row| {
                    if !is_table {
                        row.thumbnail = picture.unwrap_or_default();
                    }
                    if metadata_changed && let Some(record) = record {
                        apply_record(row, record);
                    }
                });
                row_set_us += t_row.elapsed().as_micros().saturating_as::<u64>();
            }
        }

        if regeneration_state_dirty {
            ThumbnailRegeneration::sync_toolbar_state(window, st_ref);
        }

        let total_drain_us = t_drain_start.elapsed().as_micros().saturating_as::<u64>();

        if items_drained > 0
            && let Some(bench) = thumb_ref.active_benchmark()
        {
            bench.record_drain_cycle(items_drained, row_set_us, total_drain_us);
        }

        !thumb_ref.is_drain_queue_empty()
    }

    /// Keeps the busy flag and the progress display in step and reports the finished pipeline. Returns whether something is going
    /// on (the caller then polls faster).
    fn housekeeping(window: &MainWindow, thumb_ref: &ThumbnailManager, st_ref: &RwLock<UiState>) -> bool {
        // Keep window.is_system_busy synchronized
        let is_busy = BridgeMedia::is_system_busy(window, thumb_ref, st_ref);
        if window.get_is_system_busy() != is_busy {
            window.set_is_system_busy(is_busy);
        }

        let progress = thumb_ref.progress();
        StatusBarBridge::update_thumbnail_progress(window, &progress);

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

            if is_done && !bench.is_finalized.swap(true, Ordering::SeqCst) {
                let folder_bench = st_ref.read().active_folder_bench.clone();
                let storage_stats = thumb_ref.storage_stats();
                let summary_str =
                    BenchmarkReportFormatter::render_summary(folder_bench.as_deref(), &bench, &storage_stats);
                tracing::info!("{}", summary_str);
            }
        }

        is_busy || !thumb_ref.is_drain_queue_empty()
    }
}
