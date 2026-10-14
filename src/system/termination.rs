//! Process termination: the signals that ask the application to quit, the forced exit and the watchdog of the shutdown.
//!
//! A terminal's Ctrl+C (`SIGINT`), a `kill`/session manager (`SIGTERM`) and a closed terminal (`SIGHUP`) run the same orderly
//! shutdown as the close button of the main window: the signal handler only records the signal (nothing else is
//! async-signal-safe), and the UI thread picks it up from its housekeeping timer. A second signal while the shutdown runs
//! ends the process at once, so a stuck shutdown can always be interrupted from the terminal.

use std::ffi::c_int;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGTERM: c_int = 15;
/// `signal()` returns this when it fails.
const SIG_ERR: usize = usize::MAX;
/// The disposition "ignore"; `nohup` starts a program with `SIGHUP` ignored, and that choice is kept.
const SIG_IGN: usize = 1;

/// Exit code of the watchdog when the shutdown did not finish in time.
pub const EXIT_SHUTDOWN_TIMEOUT: i32 = 1;

unsafe extern "C" {
    fn signal(signum: c_int, handler: usize) -> usize;
    fn _exit(status: c_int) -> !;
}

/// The first termination signal that arrived (0 = none).
static RECEIVED: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_termination_signal(signum: c_int) {
    // Only atomics and `_exit` are async-signal-safe; logging happens when the UI thread notices the signal.
    if RECEIVED.swap(signum, Ordering::SeqCst) != 0 {
        // SAFETY: `_exit` is async-signal-safe and never returns.
        unsafe { _exit(128 + signum) }
    }
}

/// Routes `SIGINT`, `SIGTERM` and `SIGHUP` to the orderly shutdown. Call once at startup, before other threads are created.
pub fn install_signal_handlers() {
    for (signum, name) in [(SIGINT, "SIGINT"), (SIGTERM, "SIGTERM"), (SIGHUP, "SIGHUP")] {
        // SAFETY: the handler only touches an atomic and calls `_exit`.
        let previous = unsafe { signal(signum, on_termination_signal as *const () as usize) };
        if previous == SIG_ERR {
            tracing::warn!(signal = name, "[Shutdown: Signals] Could not install the handler; the signal ends the process abruptly");
        } else if previous == SIG_IGN {
            // SAFETY: restores the disposition the process was started with.
            unsafe { signal(signum, SIG_IGN) };
            tracing::debug!(signal = name, "[Shutdown: Signals] The signal was ignored at startup; it stays ignored");
        }
    }
    tracing::debug!("[Shutdown: Signals] SIGINT, SIGTERM and SIGHUP request an orderly shutdown");
}

/// The signal that asked the application to quit, if one arrived.
pub fn received_signal() -> Option<c_int> {
    match RECEIVED.load(Ordering::SeqCst) {
        0 => None,
        signum => Some(signum),
    }
}

pub fn signal_name(signum: c_int) -> &'static str {
    match signum {
        SIGINT => "SIGINT",
        SIGTERM => "SIGTERM",
        SIGHUP => "SIGHUP",
        _ => "signal",
    }
}

/// Ends the process immediately: no destructors, no `atexit` handlers, no waiting for threads.
pub fn exit_now(code: i32) -> ! {
    // SAFETY: `_exit` has no preconditions and never returns.
    unsafe { _exit(code) }
}

/// Ends the process after `limit` if it is still running: a hung teardown (a player that never answers, a lock that is never
/// released) must not leave an invisible process behind. Runs on a thread of its own and does not need the event loop.
pub fn arm_watchdog(limit: Duration) {
    let spawned = std::thread::Builder::new()
        .name("shutdown-watchdog".into())
        .spawn(move || {
            std::thread::sleep(limit);
            tracing::error!(
                limit_ms = limit.as_millis() as u64,
                "[Shutdown: Watchdog] The shutdown did not finish in time; ending the process"
            );
            exit_now(EXIT_SHUTDOWN_TIMEOUT)
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "[Shutdown: Watchdog] Failed to start the watchdog thread");
    }
}
