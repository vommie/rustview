//! The video self-test of Settings > Debug (`docs/debug_page.md`): plays a test picture that mpv makes itself (`lavfi`, no file is
//! read) for three seconds in a small window of its own, with the video options of a real player (the user's mpv configuration,
//! `gpu-sw`, mpv's X11 GPU contexts only), and reports how it was drawn.

use super::gpu_context;
use crate::viewer::install_nonfatal_error_handler;
use libmpv2::Mpv;
use libmpv2::events::Event;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Three seconds of mpv's test picture.
pub const TEST_SOURCE: &str = "av://lavfi:testsrc2=size=640x360:rate=30:duration=3";
/// The size of the test window (physical pixels).
pub const WINDOW_WIDTH: i32 = 480;
pub const WINDOW_HEIGHT: i32 = 270;
/// Give up when the test has not ended by then (a hanging GPU context).
const TIMEOUT: Duration = Duration::from_secs(12);

/// What the self-test saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfTestOutcome {
    /// From the start of playback to the first frame on screen.
    pub first_frame_ms: Option<u64>,
    pub gpu_context: Option<String>,
    pub video_output: Option<String>,
    /// `hwdec-current` (`no` = decoded by the CPU).
    pub decoding: Option<String>,
    /// Frames shown and frames dropped (by the decoder or the video output).
    pub frames: i64,
    pub dropped: i64,
    /// mpv's error messages during the test.
    pub errors: Vec<String>,
    /// The test could not start at all.
    pub failure: Option<String>,
}

fn user_config_dir() -> Option<PathBuf> {
    let dir = directories::BaseDirs::new()?.config_dir().join("mpv");
    dir.is_dir().then_some(dir)
}

fn start(top_left: Option<(i32, i32)>) -> Result<Mpv, String> {
    let config_dir = user_config_dir();
    let mpv = Mpv::with_initializer(|init| {
        init.set_property("config", true)?;
        if let Some(dir) = &config_dir {
            init.set_property("config-dir", dir.to_string_lossy().as_ref())?;
        }
        init.set_property("gpu-sw", true)?;
        init.set_property("hwdec", "auto-safe")?;
        init.set_property("ao", "null")?;
        init.set_property("osc", false)?;
        init.set_property("input-default-bindings", false)?;
        init.set_property("input-vo-keyboard", false)?;
        Ok(())
    })
    .map_err(|e| format!("libmpv does not start: {e}"))?;
    super::messages::request(&mpv);
    // The window: small, without a frame, above the settings window; never fullscreen, whatever the user's configuration says.
    let geometry = match top_left {
        Some((x, y)) => format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT}+{x}+{y}"),
        None => format!("{WINDOW_WIDTH}x{WINDOW_HEIGHT}"),
    };
    for (name, value) in [
        ("geometry", geometry.as_str()),
        ("autofit", ""),
        ("border", "no"),
        ("ontop", "yes"),
        ("fullscreen", "no"),
        ("force-window", "yes"),
        ("keep-open", "no"),
        ("title", "RustView video self-test"),
    ] {
        let _ = mpv.set_property(name, value);
    }
    let current = mpv.get_property::<String>("gpu-context").unwrap_or_default();
    if let Some(contexts) = gpu_context::x11_gpu_contexts(&current) {
        let _ = mpv.set_property("gpu-context", contexts.as_str());
    }
    mpv.set_property("vo", "gpu")
        .map_err(|e| format!("the video output 'gpu' was refused: {e}"))?;
    mpv.command("loadfile", &[TEST_SOURCE])
        .map_err(|e| format!("the test picture does not load: {e}"))?;
    Ok(mpv)
}

/// Runs the test (blocks for about three seconds: call on a worker thread). `top_left` places the window (screen pixels).
pub fn run(top_left: Option<(i32, i32)>) -> SelfTestOutcome {
    let mut outcome = SelfTestOutcome::default();
    let mpv = match start(top_left) {
        Ok(mpv) => mpv,
        Err(failure) => {
            tracing::warn!(%failure, "[MpvPlayer: SelfTest] The video self-test did not start");
            outcome.failure = Some(failure);
            return outcome;
        }
    };
    let started = Instant::now();
    let mut playing_since: Option<Instant> = None;
    while started.elapsed() < TIMEOUT {
        let mut ended = false;
        while let Some(event) = mpv.wait_event(0.05) {
            match event {
                Ok(Event::FileLoaded) => playing_since = Some(Instant::now()),
                Ok(Event::PlaybackRestart) if outcome.first_frame_ms.is_none() => {
                    let since = playing_since.unwrap_or(started);
                    outcome.first_frame_ms = Some(u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX));
                }
                Ok(Event::LogMessage {
                    prefix, level, text, ..
                }) => {
                    super::messages::handle(prefix, level, text, None);
                    if matches!(level, "error" | "fatal") {
                        outcome.errors.push(text.trim_end().to_owned());
                    }
                }
                Ok(Event::EndFile(_) | Event::Shutdown) => ended = true,
                _ => {}
            }
            if ended {
                break;
            }
        }
        if outcome.first_frame_ms.is_some() {
            let read = |name: &str| mpv.get_property::<String>(name).ok().filter(|v| !v.is_empty());
            if outcome.gpu_context.is_none() {
                outcome.gpu_context = read("current-gpu-context");
                outcome.video_output = read("current-vo");
                outcome.decoding = read("hwdec-current");
            }
            if let Ok(frames) = mpv.get_property::<i64>("estimated-frame-number") {
                outcome.frames = frames;
            }
            let dropped = |name: &str| mpv.get_property::<i64>(name).unwrap_or(0);
            outcome.dropped = dropped("frame-drop-count") + dropped("vo-drop-frame-count");
        }
        if ended {
            break;
        }
    }
    if started.elapsed() >= TIMEOUT {
        outcome.failure = Some("the test did not end within 12 seconds".to_owned());
    }
    // Dropping the handle ends the player and closes its window (a "quit" first would interrupt mpv's own hooks).
    drop(mpv);
    // libmpv resets the process-wide Xlib error handler when its video output closes.
    install_nonfatal_error_handler();
    tracing::info!(
        first_frame_ms = ?outcome.first_frame_ms,
        gpu_context = ?outcome.gpu_context,
        video_output = ?outcome.video_output,
        decoding = ?outcome.decoding,
        frames = outcome.frames,
        dropped = outcome.dropped,
        errors = outcome.errors.len(),
        "[MpvPlayer: SelfTest] Video self-test finished"
    );
    outcome
}
