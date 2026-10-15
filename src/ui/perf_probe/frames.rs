//! Frame bookkeeping fed by the rendering notifier of the main window while a probe scenario runs.

use crate::telemetry::{IntervalStats, IntervalSummary};
use parking_lot::Mutex;
use slint::RenderingState;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Render time (microseconds) accumulated since the driver last asked; it is subtracted from a tick gap to get the time the
/// UI thread spent on everything but drawing.
static RENDER_SINCE_TICK_US: AtomicU64 = AtomicU64::new(0);
static LOG: Mutex<FrameLog> = Mutex::new(FrameLog::new());

struct FrameLog {
    before: Option<Instant>,
    last_after: Option<Instant>,
    gaps: IntervalStats,
    render: IntervalStats,
}

impl FrameLog {
    const fn new() -> Self {
        Self {
            before: None,
            last_after: None,
            gaps: IntervalStats::new_const(),
            render: IntervalStats::new_const(),
        }
    }
}

/// Called for every rendering state of the main window; costs one relaxed load while no scenario runs.
pub fn note_rendering_state(state: &RenderingState) {
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    let now = Instant::now();
    let mut log = LOG.lock();
    match state {
        RenderingState::BeforeRendering => log.before = Some(now),
        RenderingState::AfterRendering => {
            if let Some(before) = log.before.take() {
                log.render.record(now - before);
                RENDER_SINCE_TICK_US.fetch_add((now - before).as_micros() as u64, Ordering::Relaxed);
            }
            if let Some(last) = log.last_after.replace(now) {
                log.gaps.record(now - last);
            }
        }
        _ => {}
    }
}

/// Microseconds spent rendering since the previous call.
pub fn take_render_since_tick_us() -> u64 {
    RENDER_SINCE_TICK_US.swap(0, Ordering::Relaxed)
}

pub fn begin() {
    RENDER_SINCE_TICK_US.store(0, Ordering::Relaxed);
    let mut log = LOG.lock();
    log.before = None;
    log.last_after = None;
    log.gaps.clear();
    log.render.clear();
    ACTIVE.store(true, Ordering::Relaxed);
}

pub struct FrameReport {
    pub gaps: IntervalSummary,
    pub render: IntervalSummary,
}

pub fn finish() -> FrameReport {
    ACTIVE.store(false, Ordering::Relaxed);
    let log = LOG.lock();
    FrameReport {
        gaps: log.gaps.summary(),
        render: log.render.summary(),
    }
}
