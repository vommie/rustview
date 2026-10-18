//! Scripted scenarios for the Browse view (scrolling, folder switches, queries), driven by one 16 ms timer on the UI thread.
//!
//! The probe measures what the user feels: how late the event loop runs its ticks while the view scrolls or loads (a tick that
//! comes late means the UI thread was busy), split into rendering and everything else, the time until a folder or query shows
//! its first screen complete, the cost of the slice rebuilds and the memory use. Nothing here touches the drive.

use super::frames;
use super::plan::{ProbeView, Scenario, plan_from_env};
use crate::settings::LogObscurer;
use crate::telemetry::{IntervalStats, IntervalSummary, get_system_memory_metrics};
use crate::thumbnails::ThumbnailManager;
use crate::ui::MainWindow;
use crate::ui::bridge_state::UiState;
use parking_lot::RwLock;
use slint::platform::WindowEvent;
use slint::{ComponentHandle, Model, Timer, TimerMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const TICK: Duration = Duration::from_millis(16);
const READY_TIMEOUT: Duration = Duration::from_secs(120);
const LOAD_TIMEOUT: Duration = Duration::from_secs(30);
const SETTLE_AFTER_READY: Duration = Duration::from_millis(1500);
const SETTLE_BETWEEN: Duration = Duration::from_millis(800);
const SETTLE_AFTER_SWITCH: Duration = Duration::from_millis(1200);
/// A scroll that moved less than this many logical pixels in one tick has reached an end of the list.
const END_EPSILON_PX: f32 = 0.5;
/// Ticks to ignore before an unchanged viewport counts as the end (the first scroll needs a layout pass).
const WARMUP_TICKS: u64 = 5;
const WORST_TICKS_KEPT: usize = 5;
const RSS_SAMPLE_EVERY_TICKS: u64 = 30;
/// Tiles that must show their thumbnail before a folder or query counts as loaded (about the first screen).
const FIRST_SCREEN_TILES: usize = 24;
/// A load whose first item and count never change within this time is measured as "unchanged".
const UNCHANGED_AFTER: Duration = Duration::from_millis(1500);
/// Ticks spent expanding the folder tree to find the folder of a switch scenario.
const FOLDER_SEARCH_TICKS: u32 = 40;

enum Phase {
    WaitReady { since: Instant },
    Settle { until: Instant },
    Prepare { until: Instant },
    Run,
    Load,
    Jump,
}

/// Tick bookkeeping shared by all scenario kinds.
struct TickStats {
    ticks: IntervalStats,
    nonrender: IntervalStats,
    last_tick: Option<Instant>,
    tick_count: u64,
    /// (interval in microseconds, tick number, viewport y) of the slowest ticks.
    worst: Vec<(u64, u64, f32)>,
    rss_start_mb: f64,
    rss_peak_mb: f64,
}

impl TickStats {
    fn new() -> Self {
        let rss = get_system_memory_metrics().process_rss_mb;
        frames::begin();
        Self {
            ticks: IntervalStats::default(),
            nonrender: IntervalStats::default(),
            last_tick: None,
            tick_count: 0,
            worst: Vec::with_capacity(WORST_TICKS_KEPT + 1),
            rss_start_mb: rss,
            rss_peak_mb: rss,
        }
    }

    fn on_tick(&mut self, now: Instant, viewport_y: f32) {
        let render_us = frames::take_render_since_tick_us();
        if let Some(last) = self.last_tick.replace(now) {
            let gap = now - last;
            self.ticks.record(gap);
            self.nonrender.record(gap.saturating_sub(Duration::from_micros(render_us)));
            note_worst(&mut self.worst, gap.as_micros() as u64, self.tick_count, viewport_y);
        }
        if self.tick_count.is_multiple_of(RSS_SAMPLE_EVERY_TICKS) {
            self.rss_peak_mb = self.rss_peak_mb.max(get_system_memory_metrics().process_rss_mb);
        }
        self.tick_count += 1;
    }
}

struct ScrollRun {
    scenario: Scenario,
    pixels_per_second: f32,
    stats: Option<TickStats>,
    last_y: f32,
    heading_down: bool,
    reloads_start: usize,
    slice_us_start: u64,
    started: Instant,
}

struct LoadRun {
    scenario: Scenario,
    stats: Option<TickStats>,
    started: Instant,
    sync_us: u64,
    prev_total: i32,
    prev_first_id: i32,
    count_ready: Option<Duration>,
    visible_ready: Option<Duration>,
    folder_target: Option<(i32, String)>,
}

struct JumpRun {
    scenario: Scenario,
    stats: Option<TickStats>,
    remaining: u32,
    seed: u64,
    jumped_at: Option<Instant>,
    /// List position of the current jump; the jump is complete when the tile pool holds it and every tile has its picture.
    target: usize,
    first_id_before: i32,
    rested_until: Option<Instant>,
    latencies: IntervalStats,
    started: Instant,
}

enum Run {
    Scroll(ScrollRun),
    Load(LoadRun),
    Jump(JumpRun),
}

struct Driver {
    window: slint::Weak<MainWindow>,
    state: Arc<RwLock<UiState>>,
    thumbnails: Arc<ThumbnailManager>,
    plan: Vec<Scenario>,
    next: usize,
    phase: Phase,
    run: Option<Run>,
    /// Ticks spent looking for the folder of the current switch scenario in the (expanding) tree.
    folder_search_ticks: u32,
}

/// Starts the probe when `RUSTVIEW_PERF` names scenarios; does nothing otherwise.
pub fn start_if_requested(window: &MainWindow, state: &Arc<RwLock<UiState>>, thumbnails: &Arc<ThumbnailManager>) {
    let plan = plan_from_env();
    if plan.is_empty() {
        return;
    }
    tracing::info!(
        scenarios = plan.len(),
        plan = %plan.iter().map(ToString::to_string).collect::<Vec<_>>().join(","),
        "[Perf: Probe] Scripted scenarios requested; waiting for the folder and its thumbnails"
    );
    let driver = Rc::new(RefCell::new(Driver {
        window: window.as_weak(),
        state: state.clone(),
        thumbnails: thumbnails.clone(),
        plan,
        next: 0,
        phase: Phase::WaitReady { since: Instant::now() },
        run: None,
        folder_search_ticks: 0,
    }));
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, TICK, move || driver.borrow_mut().on_tick());
    // The probe lives as long as the process.
    Box::leak(Box::new(timer));
}

fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn ms_fmt(us: u64) -> String {
    format!("{:.2}", ms(us))
}

impl Driver {
    fn on_tick(&mut self) {
        let Some(window) = self.window.upgrade() else { return };
        match self.phase {
            Phase::WaitReady { since } => self.wait_ready(&window, since),
            Phase::Settle { until } => {
                if Instant::now() >= until {
                    self.begin_scenario(&window);
                }
            }
            Phase::Prepare { until } => {
                if Instant::now() >= until {
                    self.begin_scroll();
                }
            }
            Phase::Run => self.scroll_tick(&window),
            Phase::Load => self.load_tick(&window),
            Phase::Jump => self.jump_tick(&window),
        }
    }

    fn pipeline_idle(&self) -> bool {
        let progress = self.thumbnails.progress();
        !progress.is_generating && !progress.is_preloading && self.thumbnails.is_drain_queue_empty()
    }

    fn wait_ready(&mut self, window: &MainWindow, since: Instant) {
        let idle = self.pipeline_idle();
        let loaded = window.get_total_items_count() > 0;
        if loaded && idle {
            tracing::info!(
                items = window.get_total_items_count(),
                "[Perf: Probe] Folder loaded and thumbnails settled; starting the scenarios"
            );
            self.phase = Phase::Settle { until: Instant::now() + SETTLE_AFTER_READY };
        } else if since.elapsed() > READY_TIMEOUT {
            tracing::warn!(loaded, idle, "[Perf: Probe] The folder did not settle in time; closing without measuring");
            self.close(window);
        }
    }

    fn begin_scenario(&mut self, window: &MainWindow) {
        let Some(scenario) = self.plan.get(self.next).cloned() else {
            tracing::info!(scenarios = self.plan.len(), "[Perf: Probe] All scenarios finished; closing the application");
            self.close(window);
            return;
        };
        match scenario {
            Scenario::Scroll { view, pixels_per_second } => self.prepare_scroll(window, scenario.clone(), view, pixels_per_second),
            Scenario::Jump { count } => self.prepare_jump(window, scenario, count),
            Scenario::Switch { .. } | Scenario::Query { .. } => self.prepare_load(window, scenario),
        }
    }

    fn prepare_scroll(&mut self, window: &MainWindow, scenario: Scenario, view: ProbeView, pixels_per_second: f32) {
        let want_table = view == ProbeView::Table;
        let mut wait = SETTLE_BETWEEN;
        if window.get_is_table_view() != want_table {
            window.invoke_view_mode_toggled_with_index(want_table, 0);
            wait = SETTLE_AFTER_SWITCH;
        }
        window.invoke_scroll_media_to_top();
        self.phase = Phase::Prepare { until: Instant::now() + wait };
        let (reloads_start, slice_us_start) = {
            let st = self.state.read();
            st.scroll_bench.max_slice_time_us.store(0, Ordering::Relaxed);
            (
                st.scroll_bench.slice_reloads.load(Ordering::Relaxed),
                st.scroll_bench.total_slice_time_us.load(Ordering::Relaxed),
            )
        };
        self.run = Some(Run::Scroll(ScrollRun {
            scenario,
            pixels_per_second,
            stats: None,
            last_y: 0.0,
            heading_down: true,
            reloads_start,
            slice_us_start,
            started: Instant::now(),
        }));
    }

    fn begin_scroll(&mut self) {
        if let Some(Run::Scroll(run)) = self.run.as_mut() {
            run.started = Instant::now();
            run.stats = Some(TickStats::new());
        }
        self.phase = Phase::Run;
    }

    fn scroll_tick(&mut self, window: &MainWindow) {
        let Some(Run::Scroll(run)) = self.run.as_mut() else { return };
        let Some(stats) = run.stats.as_mut() else { return };
        let y = window.invoke_get_media_viewport_y();
        let now = Instant::now();
        let elapsed = stats.last_tick.map_or(TICK, |last| now - last);
        stats.on_tick(now, y);
        let stalled = stats.tick_count > WARMUP_TICKS && (y - run.last_y).abs() < END_EPSILON_PX;
        run.last_y = y;

        if run.heading_down && stalled {
            run.heading_down = false;
        } else if !run.heading_down && (stalled || y >= -END_EPSILON_PX) {
            self.finish_scroll();
            return;
        }
        let dy = run.pixels_per_second * elapsed.as_secs_f32();
        window.invoke_scroll_media_by(if run.heading_down { -dy } else { dy });
    }

    fn finish_scroll(&mut self) {
        let frame_report = frames::finish();
        let Some(Run::Scroll(run)) = self.run.take() else { return };
        let Some(stats) = run.stats else { return };
        let (reloads, slice_us, slice_max_us) = {
            let st = self.state.read();
            let bench = &st.scroll_bench;
            (
                bench.slice_reloads.load(Ordering::Relaxed) - run.reloads_start,
                bench.total_slice_time_us.load(Ordering::Relaxed) - run.slice_us_start,
                bench.max_slice_time_us.load(Ordering::Relaxed),
            )
        };
        let rss_end = get_system_memory_metrics().process_rss_mb;
        report_scroll(&run.scenario, run.started, &stats, &frame_report.gaps, &frame_report.render, reloads, slice_us, slice_max_us, rss_end);
        self.next += 1;
        self.phase = Phase::Settle { until: Instant::now() + SETTLE_BETWEEN };
    }

    fn prepare_load(&mut self, window: &MainWindow, scenario: Scenario) {
        if !self.pipeline_idle() {
            return;
        }
        let mut load = LoadRun {
            scenario: scenario.clone(),
            stats: None,
            started: Instant::now(),
            sync_us: 0,
            prev_total: window.get_total_items_count(),
            prev_first_id: first_item_id(window),
            count_ready: None,
            visible_ready: None,
            folder_target: None,
        };
        if let Scenario::Switch { folder } = &scenario {
            match find_or_expand_folder(window, folder) {
                FolderLookup::Found(id, path) => load.folder_target = Some((id, path)),
                FolderLookup::Expanded => {
                    // Try again on the next tick; the expanded rows appear after the next layout.
                    self.folder_search_ticks += 1;
                    if self.folder_search_ticks > FOLDER_SEARCH_TICKS {
                        tracing::warn!(scenario = %scenario, "[Perf: Probe] Folder not found in the tree; skipping the scenario");
                        self.next += 1;
                        self.folder_search_ticks = 0;
                    }
                    return;
                }
                FolderLookup::Missing => {
                    tracing::warn!(scenario = %scenario, "[Perf: Probe] Folder not found in the tree; skipping the scenario");
                    self.next += 1;
                    return;
                }
            }
        }
        self.folder_search_ticks = 0;
        let mut stats = TickStats::new();
        stats.last_tick = Some(Instant::now());
        load.stats = Some(stats);
        load.started = Instant::now();
        match &scenario {
            Scenario::Switch { .. } => {
                if let Some((id, path)) = load.folder_target.clone() {
                    window.invoke_folder_selected(id, path.into());
                }
            }
            Scenario::Query { text } => window.invoke_search_query_changed(text.as_str().into()),
            Scenario::Scroll { .. } | Scenario::Jump { .. } => {}
        }
        load.sync_us = load.started.elapsed().as_micros() as u64;
        self.run = Some(Run::Load(load));
        self.phase = Phase::Load;
    }

    fn prepare_jump(&mut self, window: &MainWindow, scenario: Scenario, count: u32) {
        if !self.pipeline_idle() {
            return;
        }
        window.invoke_scroll_media_to_top();
        self.run = Some(Run::Jump(JumpRun {
            scenario,
            stats: Some(TickStats::new()),
            remaining: count,
            seed: 0x2545_F491_4F6C_DD1D,
            jumped_at: None,
            target: 0,
            first_id_before: -1,
            rested_until: Some(Instant::now() + SETTLE_AFTER_SWITCH),
            latencies: IntervalStats::default(),
            started: Instant::now(),
        }));
        self.phase = Phase::Jump;
    }

    fn jump_tick(&mut self, window: &MainWindow) {
        let pool_range = self.state.read().ring.range();
        let Some(Run::Jump(run)) = self.run.as_mut() else { return };
        let Some(stats) = run.stats.as_mut() else { return };
        let now = Instant::now();
        stats.on_tick(now, window.invoke_get_media_viewport_y());
        if let Some(until) = run.rested_until {
            if now < until {
                return;
            }
            run.rested_until = None;
        }
        if let Some(at) = run.jumped_at {
            let moved = pool_range.contains(&run.target)
                && (first_item_id(window) != run.first_id_before || at.elapsed() > UNCHANGED_AFTER / 3);
            if moved && pool_complete(window) {
                run.latencies.record(at.elapsed());
                run.jumped_at = None;
                run.rested_until = Some(now + SETTLE_BETWEEN / 2);
            } else if at.elapsed() > LOAD_TIMEOUT {
                run.latencies.record(LOAD_TIMEOUT);
                run.jumped_at = None;
            }
            return;
        }
        if run.remaining == 0 {
            self.finish_jump();
            return;
        }
        let total = window.get_total_items_count().max(1) as u64;
        run.seed = run.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let target = ((run.seed >> 33) % total) as i32;
        run.remaining -= 1;
        run.first_id_before = first_item_id(window);
        run.target = target as usize;
        run.jumped_at = Some(now);
        window.invoke_scroll_media_to_index(target, window.get_is_table_view());
    }

    fn finish_jump(&mut self) {
        let frame_report = frames::finish();
        let Some(Run::Jump(run)) = self.run.take() else { return };
        let Some(stats) = run.stats.as_ref() else { return };
        let latency = run.latencies.summary();
        let nonrender = stats.nonrender.summary();
        tracing::info!(
            scenario = %run.scenario,
            jumps = latency.count,
            jump_p50_ms = ms_fmt(latency.p50_us),
            jump_p95_ms = ms_fmt(latency.p95_us),
            jump_max_ms = ms_fmt(latency.max_us),
            nonrender_p95_ms = ms_fmt(nonrender.p95_us),
            nonrender_max_ms = ms_fmt(nonrender.max_us),
            frame_gap_max_ms = ms_fmt(frame_report.gaps.max_us),
            duration_s = format!("{:.1}", run.started.elapsed().as_secs_f64()),
            rss_peak_mb = format!("{:.0}", stats.rss_peak_mb),
            "[Perf: Jump] Scenario finished"
        );
        self.next += 1;
        self.phase = Phase::Settle { until: Instant::now() + SETTLE_BETWEEN };
    }

    fn load_tick(&mut self, window: &MainWindow) {
        let Some(Run::Load(run)) = self.run.as_mut() else { return };
        let Some(stats) = run.stats.as_mut() else { return };
        stats.on_tick(Instant::now(), 0.0);
        let elapsed = run.started.elapsed();
        let total = window.get_total_items_count();
        let changed = total != run.prev_total || first_item_id(window) != run.prev_first_id;
        if changed && run.count_ready.is_none() {
            run.count_ready = Some(elapsed);
        }
        let unchanged_long_enough = !changed && elapsed > UNCHANGED_AFTER;
        // A large folder shows an empty list while it loads in the background; that is not "visible" yet.
        let still_loading = matches!(run.scenario, Scenario::Switch { .. }) && total <= 0;
        if (changed || unchanged_long_enough) && !still_loading && first_screen_ready(window, total) {
            run.visible_ready = Some(elapsed);
        }
        if run.visible_ready.is_some() || elapsed > LOAD_TIMEOUT {
            self.finish_load(window);
        }
    }

    fn finish_load(&mut self, window: &MainWindow) {
        let frame_report = frames::finish();
        let Some(Run::Load(run)) = self.run.take() else { return };
        let Some(stats) = run.stats.as_ref() else { return };
        let rss_end = get_system_memory_metrics().process_rss_mb;
        let nonrender = stats.nonrender.summary();
        let ticks = stats.ticks.summary();
        tracing::info!(
            scenario = %run.scenario,
            sync_ms = ms_fmt(run.sync_us),
            count_ms = run.count_ready.map_or("-".to_string(), |d| ms_fmt(d.as_micros() as u64)),
            visible_ms = run.visible_ready.map_or("timeout".to_string(), |d| ms_fmt(d.as_micros() as u64)),
            items = window.get_total_items_count(),
            ticks = ticks.count,
            tick_max_ms = ms_fmt(ticks.max_us),
            nonrender_p95_ms = ms_fmt(nonrender.p95_us),
            nonrender_max_ms = ms_fmt(nonrender.max_us),
            frame_gap_max_ms = ms_fmt(frame_report.gaps.max_us),
            rss_start_mb = format!("{:.0}", stats.rss_start_mb),
            rss_peak_mb = format!("{:.0}", stats.rss_peak_mb),
            rss_end_mb = format!("{:.0}", rss_end),
            "[Perf: Load] Scenario finished"
        );
        self.next += 1;
        self.phase = Phase::Settle { until: Instant::now() + SETTLE_BETWEEN };
    }

    fn close(&mut self, window: &MainWindow) {
        if let Err(e) = window.window().try_dispatch_event(WindowEvent::CloseRequested) {
            tracing::warn!(error = %e, "[Perf: Probe] The close request could not be delivered");
        }
        // Nothing left to do; keep the phase from starting again.
        self.next = usize::MAX;
        self.phase = Phase::Settle { until: Instant::now() + Duration::from_secs(3600) };
    }
}

enum FolderLookup {
    Found(i32, String),
    Expanded,
    Missing,
}

fn find_or_expand_folder(window: &MainWindow, name: &str) -> FolderLookup {
    let rows = window.get_folders();
    let mut expandable = None;
    for i in 0..rows.row_count() {
        let Some(row) = rows.row_data(i) else { continue };
        // The row's name carries the folder metrics; the last component of its path is the folder's own name.
        if std::path::Path::new(row.path.as_str()).file_name().is_some_and(|folder| folder == name) {
            return FolderLookup::Found(row.id, row.path.to_string());
        }
        if row.has_children && !row.is_expanded && expandable.is_none() {
            expandable = Some(row.id);
        }
    }
    match expandable {
        Some(id) => {
            window.invoke_folder_toggle_expand(id);
            FolderLookup::Expanded
        }
        None => {
            let listing: Vec<String> = (0..rows.row_count())
                .filter_map(|i| rows.row_data(i))
                .map(|row| {
                    format!("{}:{}:d{}:c{}:e{}", row.id, LogObscurer::filename(row.name.as_str()), row.depth, row.has_children, row.is_expanded)
                })
                .collect();
            tracing::info!(wanted = %LogObscurer::filename(name), rows = %listing.join(" | "), "[Perf: Probe] Folder tree rows");
            FolderLookup::Missing
        }
    }
}

fn first_item_id(window: &MainWindow) -> i32 {
    window.get_media_items().row_data(0).map_or(-1, |row| row.id)
}

/// Every tile of the model shows its thumbnail (the pool around the viewport is complete).
fn pool_complete(window: &MainWindow) -> bool {
    let model = window.get_media_items();
    let rows = model.row_count();
    if window.get_is_table_view() {
        return rows > 0;
    }
    rows > 0
        && (0..rows).all(|i| model.row_data(i).is_some_and(|row| row.index < 0 || row.thumbnail.size().width > 0))
}

/// The first screen is complete when its tiles exist and every one of them shows its thumbnail.
fn first_screen_ready(window: &MainWindow, total: i32) -> bool {
    let wanted = (total.max(0) as usize).min(FIRST_SCREEN_TILES);
    if wanted == 0 {
        return true;
    }
    let model = window.get_media_items();
    model.row_count() >= wanted
        && (0..wanted).all(|i| model.row_data(i).is_some_and(|row| row.index < 0 || row.thumbnail.size().width > 0))
}

fn note_worst(worst: &mut Vec<(u64, u64, f32)>, gap_us: u64, tick: u64, y: f32) {
    worst.push((gap_us, tick, y));
    worst.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.0));
    worst.truncate(WORST_TICKS_KEPT);
}

#[allow(clippy::too_many_arguments)]
fn report_scroll(
    scenario: &Scenario,
    started: Instant,
    stats: &TickStats,
    gaps: &IntervalSummary,
    render: &IntervalSummary,
    reloads: usize,
    slice_us: u64,
    slice_max_us: u64,
    rss_end_mb: f64,
) {
    let ticks = stats.ticks.summary();
    let nonrender = stats.nonrender.summary();
    let avg_slice_us = if reloads > 0 { slice_us / reloads as u64 } else { 0 };
    tracing::info!(
        scenario = %scenario,
        duration_s = format!("{:.1}", started.elapsed().as_secs_f64()),
        ticks = ticks.count,
        tick_p50_ms = ms_fmt(ticks.p50_us),
        tick_p95_ms = ms_fmt(ticks.p95_us),
        tick_p99_ms = ms_fmt(ticks.p99_us),
        tick_max_ms = ms_fmt(ticks.max_us),
        tick_over_25ms = ticks.over_25ms,
        tick_over_50ms = ticks.over_50ms,
        tick_over_100ms = ticks.over_100ms,
        nonrender_p50_ms = ms_fmt(nonrender.p50_us),
        nonrender_p95_ms = ms_fmt(nonrender.p95_us),
        nonrender_p99_ms = ms_fmt(nonrender.p99_us),
        nonrender_max_ms = ms_fmt(nonrender.max_us),
        nonrender_over_25ms = nonrender.over_25ms,
        nonrender_over_50ms = nonrender.over_50ms,
        frames = gaps.count + 1,
        frame_gap_p95_ms = ms_fmt(gaps.p95_us),
        frame_gap_max_ms = ms_fmt(gaps.max_us),
        render_p50_ms = ms_fmt(render.p50_us),
        render_p95_ms = ms_fmt(render.p95_us),
        render_max_ms = ms_fmt(render.max_us),
        slice_reloads = reloads,
        slice_avg_ms = ms_fmt(avg_slice_us),
        slice_max_ms = ms_fmt(slice_max_us),
        rss_start_mb = format!("{:.0}", stats.rss_start_mb),
        rss_peak_mb = format!("{:.0}", stats.rss_peak_mb),
        rss_end_mb = format!("{:.0}", rss_end_mb),
        "[Perf: Scroll] Scenario finished"
    );
    for (gap_us, tick, y) in &stats.worst {
        tracing::info!(
            scenario = %scenario,
            tick,
            gap_ms = ms_fmt(*gap_us),
            viewport_y = format!("{y:.0}"),
            "[Perf: Scroll] Slowest tick"
        );
    }
}
