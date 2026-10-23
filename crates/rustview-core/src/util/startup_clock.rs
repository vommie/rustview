//! Startup timeline: every stage of the launch logs how long the process has been running (`[Startup: Stage]`).
//!
//! The clock starts when `main` begins. The time before that (the dynamic loader mapping the libraries) is not part of it;
//! compare the first log line with the moment of the launch to see it.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();
static FIRST_FRAME_SEEN: AtomicBool = AtomicBool::new(false);

/// Starts the clock (first call wins).
pub fn begin() {
    let _ = START.set(Instant::now());
}

/// Milliseconds since [`begin`] (0 before it).
pub fn elapsed_ms() -> f64 {
    START.get().map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0)
}

/// Logs that the launch reached `stage`.
pub fn mark(stage: &str) {
    tracing::info!(
        stage,
        elapsed_ms = format!("{:.1}", elapsed_ms()),
        "[Startup: Stage] Launch stage reached"
    );
}

/// Logs the first rendered frame (once; later calls are one atomic load).
pub fn mark_first_frame() {
    if !FIRST_FRAME_SEEN.swap(true, Ordering::Relaxed) {
        mark("FirstFrame");
    }
}
