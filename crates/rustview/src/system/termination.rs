//! Process termination: the signals that ask the application to quit, the forced exit and the watchdog of the shutdown.
//!
//! A terminal's Ctrl+C (`SIGINT`), a `kill`/session manager (`SIGTERM`) and a closed terminal (`SIGHUP`) run the same orderly
//! shutdown as the close button of the main window: the `signal-hook` handler only wakes a thread of its own (nothing else is
//! async-signal-safe), which records the signal, and the UI thread picks it up from its housekeeping timer. A second signal while
//! the shutdown runs ends the process at once, so a stuck shutdown can always be interrupted from the terminal.

use procfs::process::Process;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::ffi::c_int;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

/// Exit code of the watchdog when the shutdown did not finish in time.
pub const EXIT_SHUTDOWN_TIMEOUT: i32 = 1;

/// The first termination signal that arrived (0 = none).
static RECEIVED: AtomicI32 = AtomicI32::new(0);

/// Whether the process was started with `signal` ignored (`nohup` ignores `SIGHUP`); that choice is kept.
fn ignored_at_startup(signal: c_int) -> bool {
    let Ok(status) = Process::myself().and_then(|process| process.status()) else {
        return false;
    };
    u32::try_from(signal - 1)
        .ok()
        .and_then(|bit| 1u64.checked_shl(bit))
        .is_some_and(|mask| status.sigign & mask != 0)
}

/// Routes `SIGINT`, `SIGTERM` and `SIGHUP` to the orderly shutdown. Call once at startup, before other threads are created.
pub fn install_signal_handlers() {
    let wanted: Vec<c_int> = [(SIGINT, "SIGINT"), (SIGTERM, "SIGTERM"), (SIGHUP, "SIGHUP")]
        .into_iter()
        .filter(|&(signum, name)| {
            let ignored = ignored_at_startup(signum);
            if ignored {
                tracing::debug!(
                    signal = name,
                    "[Shutdown: Signals] The signal was ignored at startup; it stays ignored"
                );
            }
            !ignored
        })
        .map(|(signum, _)| signum)
        .collect();

    let mut signals = match Signals::new(&wanted) {
        Ok(signals) => signals,
        Err(e) => {
            tracing::warn!(error = %e, "[Shutdown: Signals] Could not install the handlers; the signals end the process abruptly");
            return;
        }
    };
    // The library's handler only wakes this thread (async-signal-safe); logging and the decision happen here. The UI thread picks the
    // signal up from its housekeeping timer.
    let spawned = std::thread::Builder::new()
        .name("termination-signals".into())
        .spawn(move || {
            for signum in signals.forever() {
                if RECEIVED.swap(signum, Ordering::SeqCst) != 0 {
                    exit_now(128 + signum);
                }
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "[Shutdown: Signals] Could not start the signal thread; the signals are held back");
        return;
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
    signal_hook::low_level::exit(code)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_have_names() {
        assert_eq!(signal_name(SIGINT), "SIGINT");
        assert_eq!(signal_name(SIGTERM), "SIGTERM");
        assert_eq!(signal_name(SIGHUP), "SIGHUP");
        assert_eq!(signal_name(99), "signal");
    }

    #[test]
    fn nothing_is_ignored_in_the_test_process() {
        // Test harnesses do not start with SIGINT or SIGTERM ignored.
        assert!(!ignored_at_startup(SIGTERM));
    }
}
