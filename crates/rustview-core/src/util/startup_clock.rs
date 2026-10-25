//! Startup timeline: every stage of the launch logs how long the process has been running (`[Startup: Stage]`).
//!
//! The clock starts when `main` begins. The time before that (the dynamic loader mapping the libraries) is not part of it;
//! compare the first log line with the moment of the launch to see it. The stages are also kept in memory for the startup
//! timeline of Settings > Debug.

use parking_lot::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static FIRST_FRAME_SEEN: AtomicBool = AtomicBool::new(false);
static STAGES: Mutex<Vec<(String, f64)>> = Mutex::new(Vec::new());

/// The stage name of the first rendered frame.
pub const FIRST_FRAME: &str = "FirstFrame";

/// Starts the clock (first call wins).
pub fn begin() {
    let _ = START.set(Instant::now());
}

/// Milliseconds since [`begin`] (0 before it).
pub fn elapsed_ms() -> f64 {
    START.get().map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0)
}

/// Logs that the launch reached `stage` and keeps it for [`stages`].
pub fn mark(stage: &str) {
    let elapsed = elapsed_ms();
    STAGES.lock().push((stage.to_owned(), elapsed));
    tracing::info!(
        stage,
        elapsed_ms = format!("{elapsed:.1}"),
        "[Startup: Stage] Launch stage reached"
    );
}

/// The stages of this launch with their milliseconds since `main` began, in the order they were reached.
pub fn stages() -> Vec<(String, f64)> {
    STAGES.lock().clone()
}

/// Logs the first rendered frame (once; later calls are one atomic load).
pub fn mark_first_frame() {
    if !FIRST_FRAME_SEEN.swap(true, Ordering::Relaxed) {
        mark(FIRST_FRAME);
    }
}
