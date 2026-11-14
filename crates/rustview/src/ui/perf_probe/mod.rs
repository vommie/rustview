//! Performance probe of the main window for developers: runs a plan of scripted steps (scrolling, resizing, dragging, folder
//! switches) and logs every frame, so the responsiveness of the UI can be measured and compared on any machine
//! (`docs/perf/README.md`, `scripts/perf/`).
//!
//! It runs only when `RUSTVIEW_PERF_PLAN` names a plan file; the log goes to `RUSTVIEW_PERF_LOG` (default: the plan's path
//! plus `.log`). Without the variable nothing is installed and each hook costs one relaxed atomic load.

mod driver;
mod input;
pub mod plan;
mod recorder;
mod tiles;

pub use driver::Context;
pub use recorder::{event, flush, is_active, on_rendering};

use crate::ui::MainWindow;
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

const PLAN_VARIABLE: &str = "RUSTVIEW_PERF_PLAN";
const LOG_VARIABLE: &str = "RUSTVIEW_PERF_LOG";

/// Where the log goes; pictures of the window are saved next to it.
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Lists the Browse view has shown since the start (folder loads, sorts, filters).
static COMMITS: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
}

fn commits() -> u64 {
    COMMITS.load(Ordering::Relaxed)
}

/// The Browse view shows a new list of `total` rows.
pub fn note_list_committed(total: usize) {
    if is_active() {
        COMMITS.fetch_add(1, Ordering::Relaxed);
        event("list_committed", u64::try_from(total).unwrap_or(u64::MAX));
    }
}

/// Saves what the window shows as `LOG.NAME.png` (a `shot` step of the plan).
fn shot(window: &MainWindow, name: &str) {
    let Some(log_path) = LOG_PATH.get() else { return };
    let mut path = log_path.clone().into_os_string();
    path.push(format!(".{name}.png"));
    let saved = window
        .window()
        .take_snapshot()
        .map_err(|e| e.to_string())
        .and_then(|pixels| {
            image::save_buffer(
                &path,
                pixels.as_bytes(),
                pixels.width(),
                pixels.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string())
        });
    if let Err(reason) = saved {
        tracing::error!(reason, "[PerfProbe] The picture of the window was not saved");
    }
}

/// Starts the run when a plan was given. A plan that cannot be used is an error: the application must not stay open and
/// idle on a desktop where a script waits for the run.
pub fn start_if_requested(ctx: Context) -> Result<(), String> {
    let Some(plan_path) = std::env::var_os(PLAN_VARIABLE).map(PathBuf::from) else {
        return Ok(());
    };
    let steps = std::fs::read_to_string(&plan_path)
        .map_err(|e| e.to_string())
        .and_then(|text| plan::parse(&text))
        .map_err(|reason| format!("the probe's plan cannot be used: {reason}"))?;
    let log_path = std::env::var_os(LOG_VARIABLE).map_or_else(
        || {
            let mut path = plan_path.clone().into_os_string();
            path.push(".log");
            PathBuf::from(path)
        },
        PathBuf::from,
    );
    recorder::start(&log_path).map_err(|e| format!("the probe's log cannot be written: {e}"))?;
    let _ = LOG_PATH.set(log_path);
    tracing::info!(steps = steps.len(), "[PerfProbe] Run started");
    recorder::info("version", env!("CARGO_PKG_VERSION"));

    let (window, state) = (ctx.window.clone(), ctx.state.clone());
    recorder::set_tile_counter(Box::new(move || {
        window.upgrade().and_then(|window| tiles::count(&window, &state))
    }));

    let mut driver = driver::Driver::new(ctx, steps);
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, driver::TICK, move || {
        if driver.tick() {
            // The timer cannot be dropped from inside its own callback.
            Timer::single_shot(std::time::Duration::ZERO, || {
                TIMER.with(|slot| drop(slot.borrow_mut().take()));
            });
        }
    });
    TIMER.with(|slot| *slot.borrow_mut() = Some(timer));
    Ok(())
}
