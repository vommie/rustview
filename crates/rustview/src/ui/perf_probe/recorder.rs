//! The probe's log: one line per rendered frame of the main window, per scenario mark and per event.
//!
//! Line formats (times in microseconds of `CLOCK_MONOTONIC`, so a script can cut the log with its own clock):
//!
//! * `I key value`: facts about the run (window size, scale factor, ...).
//! * `F start render cpu_render cpu_cycle input_age blank shown`: a frame. `start` is when rendering began, `render` how long
//!   Slint drew, `cpu_render` the CPU time of the UI thread while drawing, `cpu_cycle` its CPU time since the previous frame
//!   ended (events, callbacks, layout and drawing: what the application itself costs, whatever the display waits for),
//!   `input_age` the age of the oldest pointer event this frame is the first to show (-1 without one), `blank` the visible
//!   media tiles that had no thumbnail in this frame and `shown` the visible media tiles (-1: not a thumbnail view).
//! * `S time begin|end name rss_kb process_cpu`: a scenario mark with the resident memory and the CPU time of the process.
//! * `E time name value`: an event (a folder list is committed, the first thumbnail is there, ...).

use parking_lot::Mutex;
use rustix::time::{ClockId, clock_gettime};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use slint::RenderingState;
use std::cell::RefCell;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// When the oldest pointer event that no frame has shown yet arrived (0: none).
static PENDING_INPUT_US: AtomicU64 = AtomicU64::new(0);
/// Frames drawn since the recording began.
static FRAMES: AtomicU64 = AtomicU64::new(0);
static LOG: Mutex<Option<Log>> = Mutex::new(None);

/// Visible media tiles without a thumbnail and visible media tiles in all, or `None` where the view shows no thumbnails.
pub type TileCounter = Box<dyn Fn() -> Option<(u32, u32)>>;

thread_local! {
    static TILE_COUNTER: RefCell<Option<TileCounter>> = const { RefCell::new(None) };
}

struct Log {
    out: BufWriter<File>,
    frame_start_us: u64,
    cpu_at_frame_start_us: u64,
    cpu_at_frame_end_us: u64,
    tiles: Option<(u32, u32)>,
}

fn micros(clock: ClockId) -> u64 {
    let now = clock_gettime(clock);
    let seconds: u64 = now.tv_sec.saturating_as();
    let nanos: u64 = now.tv_nsec.saturating_as();
    seconds.saturating_mul(1_000_000).saturating_add(nanos / 1000)
}

/// Microseconds of the monotonic clock.
pub fn now_us() -> u64 {
    micros(ClockId::Monotonic)
}

/// Whether a probe run records; one relaxed load, so the hooks cost nothing without a run.
pub fn is_active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Frames the main window has drawn since the recording began.
pub fn frames() -> u64 {
    FRAMES.load(Ordering::Relaxed)
}

/// Starts recording into `path`.
pub fn start(path: &Path) -> std::io::Result<()> {
    let out = BufWriter::with_capacity(1 << 16, File::create(path)?);
    *LOG.lock() = Some(Log {
        out,
        frame_start_us: 0,
        cpu_at_frame_start_us: 0,
        cpu_at_frame_end_us: micros(ClockId::ThreadCPUTime),
        tiles: None,
    });
    ACTIVE.store(true, Ordering::Relaxed);
    Ok(())
}

/// The function that counts the visible tiles without a thumbnail; called on the UI thread right before a frame is drawn.
pub fn set_tile_counter(counter: TileCounter) {
    TILE_COUNTER.with(|slot| *slot.borrow_mut() = Some(counter));
}

fn write_line(line: std::fmt::Arguments<'_>) {
    if let Some(log) = LOG.lock().as_mut() {
        // A full disk ends the recording silently; the run's script notices the short log.
        let _ = log.out.write_fmt(line);
    }
}

/// A fact about the run.
pub fn info(key: &str, value: impl std::fmt::Display) {
    if is_active() {
        write_line(format_args!("I {key} {value}\n"));
    }
}

/// An event with a value (a duration in microseconds, a count).
pub fn event(name: &str, value: u64) {
    if is_active() {
        write_line(format_args!("E {} {name} {value}\n", now_us()));
    }
}

/// The start or the end of a scenario, with the memory and the CPU time of the process at that moment.
pub fn mark(begin: bool, name: &str) {
    if !is_active() {
        return;
    }
    let rss_kb = (rustview_core::telemetry::get_system_memory_metrics().process_rss_mb * 1024.0).to_int::<u64>();
    let edge = if begin { "begin" } else { "end" };
    write_line(format_args!(
        "S {} {edge} {name} {rss_kb} {}\n",
        now_us(),
        micros(ClockId::ProcessCPUTime)
    ));
}

/// A pointer event reached the window: the next frame is the first that can show its effect.
pub fn note_input() {
    if is_active() {
        let _ = PENDING_INPUT_US.compare_exchange(0, now_us().max(1), Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// Called for every rendering state of the main window.
pub fn on_rendering(state: &RenderingState) {
    if !is_active() {
        return;
    }
    match state {
        RenderingState::BeforeRendering => {
            let tiles = TILE_COUNTER.with(|slot| slot.borrow().as_ref().and_then(|count| count()));
            if let Some(log) = LOG.lock().as_mut() {
                log.frame_start_us = now_us();
                log.cpu_at_frame_start_us = micros(ClockId::ThreadCPUTime);
                log.tiles = tiles;
            }
        }
        RenderingState::AfterRendering => {
            FRAMES.fetch_add(1, Ordering::Relaxed);
            let now = now_us();
            let cpu = micros(ClockId::ThreadCPUTime);
            let input = PENDING_INPUT_US.swap(0, Ordering::Relaxed);
            if let Some(log) = LOG.lock().as_mut()
                && log.frame_start_us != 0
            {
                let input_age = if input == 0 {
                    -1
                } else {
                    now.saturating_sub(input).saturating_as::<i64>()
                };
                let (blank, shown) = log
                    .tiles
                    .map_or((-1, -1), |(blank, shown)| (i64::from(blank), i64::from(shown)));
                let _ = writeln!(
                    log.out,
                    "F {} {} {} {} {input_age} {blank} {shown}",
                    log.frame_start_us,
                    now.saturating_sub(log.frame_start_us),
                    cpu.saturating_sub(log.cpu_at_frame_start_us),
                    cpu.saturating_sub(log.cpu_at_frame_end_us),
                );
                log.cpu_at_frame_end_us = cpu;
                log.frame_start_us = 0;
            }
        }
        _ => {}
    }
}

/// Writes what is buffered; the end of a run and the shutdown call it.
pub fn flush() {
    if let Some(log) = LOG.lock().as_mut() {
        let _ = log.out.flush();
    }
}
