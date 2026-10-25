//! The live values of the Runtime card and the report: version, process, memory, thumbnail cache, players, display, the
//! environment variables that change how `RustView` runs, and the log. Everything is read from memory or `/proc`.

use crate::system::diagnostics::{graphics, window_system_facts};
use rustview_core::diagnostics::dependencies::WindowSystemKind;
use rustview_core::diagnostics::health;
use rustview_core::logging;
use rustview_core::settings::LogObscurer;
use rustview_core::settings::memory_budget::{BYTES_PER_MB, MemoryPlan};
use rustview_core::telemetry::{format_bytes_metric, format_count};
use rustview_core::thumbnails::RamCacheStats;
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};
use rustview_core::util::startup_clock;

/// The environment variables shown when they are set.
const OVERRIDES: [&str; 7] = [
    "RUST_LOG",
    "RUSTVIEW_WINDOW_SYSTEM",
    "SLINT_BACKEND",
    "SLINT_SCALE_FACTOR",
    "SLINT_STYLE",
    "MPV_HOME",
    "LIBGL_ALWAYS_SOFTWARE",
];

/// What the rows are made from (gathered on the thread that shows them).
pub struct RuntimeInputs {
    pub plan: MemoryPlan,
    pub cache: RamCacheStats,
}

/// "no player", "1 player", "3 players".
fn players_text(count: usize) -> String {
    match count {
        0 => "no player".to_owned(),
        1 => "1 player".to_owned(),
        n => format!("{n} players"),
    }
}

/// "2 h 14 min", "3 min 5 s", "12 s".
pub fn duration_text(total_secs: u64) -> String {
    let (hours, minutes, seconds) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
    if hours > 0 {
        format!("{hours} h {minutes} min")
    } else if minutes > 0 {
        format!("{minutes} min {seconds} s")
    } else {
        format!("{seconds} s")
    }
}

fn uptime_secs() -> u64 {
    (startup_clock::elapsed_ms() / 1000.0).to_int::<u64>()
}

fn window_system_text() -> String {
    let (kind, reason) = window_system_facts();
    let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_owned());
    let name = match kind {
        WindowSystemKind::X11Session => "X11",
        WindowSystemKind::XWayland => "XWayland",
        WindowSystemKind::Wayland => "native Wayland",
        WindowSystemKind::NoDisplay => "no display",
    };
    let reason = reason.map(|r| format!(" ({r})")).unwrap_or_default();
    format!("{name}{reason}, session {session}")
}

fn overrides_text() -> String {
    let set: Vec<String> = OVERRIDES
        .iter()
        .filter_map(|name| std::env::var(name).ok().map(|value| format!("{name}={value}")))
        .collect();
    if set.is_empty() {
        "none".to_owned()
    } else {
        set.join(", ")
    }
}

fn log_text() -> String {
    let level = logging::env_override().map_or_else(
        || logging::level().label().to_owned(),
        |value| format!("RUST_LOG={value}"),
    );
    let files = if logging::sinks().files_running() {
        "files on"
    } else {
        "files off"
    };
    let names = if LogObscurer::is_enabled() {
        "names hidden"
    } else {
        "names shown"
    };
    format!(
        "{level}, {} lines in memory, {files}, {names} in the terminal",
        format_count(logging::ring().len())
    )
}

/// The rows "label: value" of the Runtime card.
pub fn rows(inputs: &RuntimeInputs) -> Vec<(String, String)> {
    let build = if cfg!(debug_assertions) {
        "debug build"
    } else {
        "release build"
    };
    let metrics = rustview_core::telemetry::get_system_memory_metrics();
    let threads = procfs::process::Process::myself()
        .and_then(|process| process.stat())
        .map_or_else(|_| "?".to_owned(), |stat| stat.num_threads.to_string());
    let cache = &inputs.cache;
    let lookups = cache.hits + cache.misses;
    let hit_rate = if lookups == 0 {
        String::new()
    } else {
        format!(
            ", {} % hits",
            (cache.hits.az::<f64>() * 100.0 / lookups.az::<f64>()).round()
        )
    };
    let gl = graphics::gl_info().map_or_else(
        || "not known yet".to_owned(),
        |info| format!("{}, {}", info.renderer, health::gl_version_text(&info.version)),
    );
    let context = graphics::mpv_context().map_or_else(String::new, |c| format!(", draw with {c}"));
    vec![
        ("Version".to_owned(), format!("{} ({build})", env!("CARGO_PKG_VERSION"))),
        (
            "Process".to_owned(),
            format!(
                "pid {}, up {}, {threads} threads",
                std::process::id(),
                duration_text(uptime_secs())
            ),
        ),
        (
            "RAM".to_owned(),
            format!(
                "{} MB in use, budget {} MB{}",
                format_count(metrics.process_rss_mb.round().to_int::<usize>()),
                format_count((inputs.plan.budget_bytes / BYTES_PER_MB).saturating_as::<usize>()),
                if inputs.plan.automatic { " (automatic)" } else { "" }
            ),
        ),
        (
            "Thumbnail cache".to_owned(),
            format!(
                "{} in RAM, {} of {}{hit_rate}",
                format_count(cache.entries),
                format_bytes_metric(cache.used_bytes.saturating_as::<u64>()),
                format_bytes_metric(cache.budget_bytes.saturating_as::<u64>())
            ),
        ),
        (
            "Videos".to_owned(),
            format!("{}{context}", players_text(crate::viewer::mpv_engine::live_players())),
        ),
        ("Window system".to_owned(), window_system_text()),
        ("Graphics".to_owned(), gl),
        ("Environment".to_owned(), overrides_text()),
        ("Log".to_owned(), log_text()),
    ]
}

/// The one line of the closed card.
pub fn summary() -> String {
    let metrics = rustview_core::telemetry::get_system_memory_metrics();
    format!(
        "{} MB RAM, up {}",
        format_count(metrics.process_rss_mb.round().to_int::<usize>()),
        duration_text(uptime_secs())
    )
}

#[cfg(test)]
mod tests {
    use super::{duration_text, players_text};

    #[test]
    fn durations_read_like_a_clock() {
        assert_eq!(duration_text(12), "12 s");
        assert_eq!(duration_text(185), "3 min 5 s");
        assert_eq!(duration_text(2 * 3600 + 14 * 60 + 9), "2 h 14 min");
    }

    #[test]
    fn players_are_counted_in_words() {
        assert_eq!(players_text(0), "no player");
        assert_eq!(players_text(1), "1 player");
        assert_eq!(players_text(3), "3 players");
    }
}
