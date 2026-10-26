//! The video self-test of Settings > Debug (`docs/debug_page.md`): plays a test picture that mpv makes itself (`lavfi`, no file is
//! read) for three seconds the way videos are drawn in `RustView` (the user's mpv configuration, `vo=libmpv`, mpv's OpenGL render
//! API on a render thread whose context shares textures with the main window), into a texture of its own that is not shown, and
//! reports how it was drawn and whether the picture has content.

use super::render::{NativeDisplay, RenderCore};
use crate::viewer::video_surface::gl_api::{describe, glow_context};
use crate::viewer::video_surface::share_group::ShareGroup;
use crate::viewer::video_surface::target::{INTERNAL_FORMAT, VideoTarget};
use libmpv2::Mpv;
use libmpv2::events::Event;
use parking_lot::{Condvar, Mutex};
use rustview_core::util::cast::SaturatingAs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant};

/// Three seconds of mpv's test picture.
pub const TEST_SOURCE: &str = "av://lavfi:testsrc2=size=640x360:rate=30:duration=3";
/// The size of the texture the test renders into (physical pixels).
const WIDTH: u32 = 480;
const HEIGHT: u32 = 270;
/// Give up when the test has not ended by then (a hanging driver).
const TIMEOUT: Duration = Duration::from_secs(12);

/// What the self-test saw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfTestOutcome {
    /// From the start of playback to the first frame mpv rendered.
    pub first_frame_ms: Option<u64>,
    /// How the video was drawn ("render API, EGL", ...) and the GL renderer of the render context.
    pub render_path: Option<String>,
    pub gl_renderer: Option<String>,
    pub video_output: Option<String>,
    /// `hwdec-current` (`no` = decoded by the CPU).
    pub decoding: Option<String>,
    /// Frames rendered and frames dropped (by the decoder or the video output).
    pub frames: i64,
    pub dropped: i64,
    /// The last rendered picture is not just one color (mpv drew the test picture, not black).
    pub picture_ok: Option<bool>,
    /// mpv's error messages during the test.
    pub errors: Vec<String>,
    /// The test could not start at all.
    pub failure: Option<String>,
}

pub(super) fn user_config_dir() -> Option<PathBuf> {
    let dir = directories::BaseDirs::new()?.config_dir().join("mpv");
    dir.is_dir().then_some(dir)
}

fn start() -> Result<Arc<Mpv>, String> {
    let config_dir = user_config_dir();
    let mpv = Mpv::with_initializer(|init| {
        init.set_property("config", true)?;
        if let Some(dir) = &config_dir {
            init.set_property("config-dir", dir.to_string_lossy().as_ref())?;
        }
        init.set_property("vo", "libmpv")?;
        init.set_property("hwdec", "auto-safe")?;
        init.set_property("ao", "null")?;
        init.set_property("osc", false)?;
        init.set_property("input-default-bindings", false)?;
        Ok(())
    })
    .map_err(|e| format!("libmpv does not start: {e}"))?;
    super::messages::request(&mpv);
    let _ = mpv.set_property("keep-open", "no");
    Ok(Arc::new(mpv))
}

/// What the render thread of the test hands back.
#[derive(Default)]
struct RenderReport {
    ready: bool,
    error: Option<String>,
    gl_renderer: Option<String>,
    picture_ok: Option<bool>,
}

struct RenderShared {
    report: Mutex<RenderReport>,
    changed: Condvar,
    wake: Mutex<bool>,
    woken: Condvar,
    quit: AtomicBool,
    frames: AtomicI64,
    first_frame: Mutex<Option<Instant>>,
}

/// The render thread of the test: a context in the main window's share group, mpv's render context, one texture.
fn render_thread(core: &RenderCore, group: ShareGroup, display: NativeDisplay, shared: &Arc<RenderShared>) {
    let fail = |e: String| {
        let mut report = shared.report.lock();
        report.error = Some(e);
        report.ready = true;
        shared.changed.notify_all();
    };
    let context = match group.create_context() {
        Ok(context) => context,
        Err(e) => return fail(e),
    };
    if let Err(e) = context.make_current() {
        context.destroy();
        return fail(e);
    }
    let gl = glow_context(context.api());
    let mut render = match core.create_context(context.api(), display) {
        Ok(render) => render,
        Err(e) => {
            context.destroy();
            return fail(e);
        }
    };
    let waker = Arc::clone(shared);
    render.set_update_callback(move || {
        *waker.wake.lock() = true;
        waker.woken.notify_one();
    });
    let target = VideoTarget::new(&gl, WIDTH, HEIGHT);
    {
        let mut report = shared.report.lock();
        report.ready = true;
        report.gl_renderer = Some(describe(&gl));
        if let Err(e) = &target {
            report.error = Some(e.clone());
        }
        shared.changed.notify_all();
    }
    if let Ok(target) = &target {
        while !shared.quit.load(Ordering::Acquire) {
            {
                let mut wake = shared.wake.lock();
                if !*wake {
                    shared.woken.wait_for(&mut wake, Duration::from_millis(100));
                }
                *wake = false;
            }
            if render.has_new_frame()
                && render
                    .render(
                        target.fbo(),
                        WIDTH.saturating_as(),
                        HEIGHT.saturating_as(),
                        INTERNAL_FORMAT,
                    )
                    .is_ok()
            {
                shared.frames.fetch_add(1, Ordering::Relaxed);
                shared.first_frame.lock().get_or_insert_with(Instant::now);
            }
        }
        shared.report.lock().picture_ok = Some(target.has_content(&gl));
    }
    drop(render);
    if let Ok(target) = target {
        target.delete(&gl);
    }
    context.destroy();
}

/// Runs the test (blocks for about three seconds: call on a worker thread). `group` is the share group of the main window
/// (`video_surface::hosts::share_group`, read on the UI thread); without one, nothing can be drawn.
pub fn run(group: Option<(ShareGroup, NativeDisplay)>) -> SelfTestOutcome {
    let mut outcome = SelfTestOutcome::default();
    let Some((group, display)) = group else {
        outcome.failure = Some("the main window has no OpenGL context to draw videos in".to_owned());
        return outcome;
    };
    let mpv = match start() {
        Ok(mpv) => mpv,
        Err(failure) => {
            tracing::warn!(%failure, "[MpvPlayer: SelfTest] The video self-test did not start");
            outcome.failure = Some(failure);
            return outcome;
        }
    };
    outcome.render_path = Some(format!("mpv's render API, {}", group.api().name()));
    let shared = Arc::new(RenderShared {
        report: Mutex::new(RenderReport::default()),
        changed: Condvar::new(),
        wake: Mutex::new(false),
        woken: Condvar::new(),
        quit: AtomicBool::new(false),
        frames: AtomicI64::new(0),
        first_frame: Mutex::new(None),
    });
    let core = RenderCore::of(&mpv);
    let thread_shared = Arc::clone(&shared);
    let renderer = std::thread::Builder::new()
        .name("video-self-test-render".into())
        .spawn(move || render_thread(&core, group, display, &thread_shared));
    let renderer = match renderer {
        Ok(handle) => handle,
        Err(e) => {
            outcome.failure = Some(format!("the render thread does not start: {e}"));
            return outcome;
        }
    };
    {
        let mut report = shared.report.lock();
        while !report.ready {
            shared.changed.wait_for(&mut report, TIMEOUT);
        }
        if let Some(error) = report.error.clone() {
            outcome.failure = Some(error);
        }
    }
    if outcome.failure.is_none() {
        play(&mpv, &shared, &mut outcome);
    }
    shared.quit.store(true, Ordering::Release);
    shared.woken.notify_one();
    let _ = renderer.join();
    let report = shared.report.lock();
    outcome.gl_renderer.clone_from(&report.gl_renderer);
    outcome.picture_ok = report.picture_ok;
    drop(report);
    // The render context is gone (joined above); now the core may end.
    drop(mpv);
    tracing::info!(
        first_frame_ms = ?outcome.first_frame_ms,
        render_path = ?outcome.render_path,
        gl = ?outcome.gl_renderer,
        decoding = ?outcome.decoding,
        frames = outcome.frames,
        dropped = outcome.dropped,
        picture_ok = ?outcome.picture_ok,
        errors = outcome.errors.len(),
        "[MpvPlayer: SelfTest] Video self-test finished"
    );
    outcome
}

/// Plays the test picture until it ends and reads mpv's numbers.
fn play(mpv: &Mpv, shared: &RenderShared, outcome: &mut SelfTestOutcome) {
    if let Err(e) = mpv.command("loadfile", &[TEST_SOURCE]) {
        outcome.failure = Some(format!("the test picture does not load: {e}"));
        return;
    }
    let started = Instant::now();
    let mut ended = false;
    while !ended && started.elapsed() < TIMEOUT {
        while let Some(event) = mpv.wait_event(0.05) {
            match event {
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
        if let Some(first) = *shared.first_frame.lock() {
            outcome.first_frame_ms.get_or_insert_with(|| {
                u64::try_from(first.saturating_duration_since(started).as_millis()).unwrap_or(u64::MAX)
            });
            let read = |name: &str| mpv.get_property::<String>(name).ok().filter(|v| !v.is_empty());
            if outcome.video_output.is_none() {
                outcome.video_output = read("current-vo");
                outcome.decoding = read("hwdec-current");
            }
            let dropped = |name: &str| mpv.get_property::<i64>(name).unwrap_or(0);
            outcome.dropped = dropped("frame-drop-count") + dropped("decoder-frame-drop-count");
        }
        outcome.frames = shared.frames.load(Ordering::Relaxed);
    }
    if !ended {
        outcome.failure = Some("the test did not end within 12 seconds".to_owned());
    }
}
