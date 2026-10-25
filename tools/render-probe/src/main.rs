//! Probe for the rendering audit: mpv's render API (OpenGL, as RustCut does it) inside Slint windows.
//!
//! `render-probe grid SECS FILE...` plays 1-4 files (looped, no sound) in one 1280x720 window, each drawn by its own mpv
//! core into a texture that Slint shows as an image, with a caption drawn over the videos (what needs extra windows and
//! X11 shapes with native mpv windows). Every 5 s and at the end it prints Slint's and every video's frame rate.
//!
//! `render-probe handover FILE` plays a file in window A, moves the player to window B after 3 s (a Slint window has a GL
//! context of its own and Slint does not share them, so mpv's render context is freed in A and created anew in B), pauses it,
//! moves it back to A after 3 more seconds, and prints how long the video was gone and whether position and pause survived.
#![allow(unsafe_code, reason = "GL and libmpv FFI through the helpers copied from RustCut")]
#![allow(clippy::print_stdout, clippy::print_stderr, reason = "a command line probe prints its results")]

mod gl;
mod mpv_render;

use gl::{StateGuard, VideoTarget};
use libmpv2::Mpv;
use mpv_render::MpvRender;
use slint::{ComponentHandle, GraphicsAPI, Model, RenderingState};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

slint::slint! {
    export component ProbeWindow inherits Window {
        in property <[image]> frames;
        in property <int> columns: 1;
        in property <string> caption;
        preferred-width: 1280px;
        preferred-height: 720px;
        background: #000000;
        property <int> rows: Math.ceil(root.frames.length / root.columns);
        for frame[i] in root.frames : Image {
            x: Math.mod(i, root.columns) * root.width / root.columns;
            y: Math.floor(i / root.columns) * root.height / Math.max(1, root.rows);
            width: root.width / root.columns;
            height: root.height / Math.max(1, root.rows);
            source: frame;
            image-fit: contain;
        }
        Rectangle {
            x: 12px;
            y: root.height - 44px;
            width: caption-text.preferred-width + 20px;
            height: 32px;
            border-radius: 16px;
            background: #11111bb3;
            caption-text := Text {
                text: root.caption;
                color: #cdd6f4;
                font-size: 14px;
            }
        }
    }
}

/// One mpv core and what it draws into in the window that shows it.
struct Player {
    mpv: Arc<Mpv>,
    render: Option<MpvRender>,
    target: Option<VideoTarget>,
    frames: u64,
    render_time: Duration,
}

fn new_mpv() -> Arc<Mpv> {
    let mpv = Mpv::with_initializer(|init| {
        init.set_option("vo", "libmpv")?;
        init.set_option("ao", "null")?;
        init.set_option("hwdec", "auto-safe")?;
        init.set_option("keep-open", "yes")?;
        init.set_option("idle", "yes")?;
        init.set_option("config", "no")?;
        init.set_option("terminal", "no")?;
        Ok(())
    })
    .expect("mpv core");
    Arc::new(mpv)
}

/// The GL state of one window, between RenderingSetup and RenderingTeardown.
struct WindowGl {
    gl: glow::Context,
}

/// The size of tile `index` of `count` in a window of `size` (physical pixels), as the Slint grid lays them out.
fn tile_size(size: slint::PhysicalSize, count: usize, columns: usize) -> (u32, u32) {
    let rows = count.div_ceil(columns).max(1);
    let cols = u32::try_from(columns).unwrap_or(1).max(1);
    let rows = u32::try_from(rows).unwrap_or(1);
    ((size.width / cols).max(1), (size.height / rows).max(1))
}

/// Draws `player`'s newest frame into its texture (a new texture when the size changed). Returns a new image to present.
fn draw(gl: &glow::Context, player: &mut Player, size: (u32, u32)) -> Option<slint::Image> {
    let render = player.render.as_ref()?;
    let new_frame = render.has_new_frame();
    let resized = player.target.as_ref().is_none_or(|t| t.size() != size);
    if !new_frame && !resized {
        return None;
    }
    let guard = StateGuard::enter(gl);
    let mut presented = None;
    if resized {
        if let Some(old) = player.target.take() {
            old.delete(gl);
        }
        let target = VideoTarget::new(gl, size.0, size.1).ok()?;
        presented = Some(target.image());
        player.target = Some(target);
    }
    let target = player.target.as_ref()?;
    let fbo = target.framebuffer_id()?;
    let start = Instant::now();
    let result = render.render(fbo, i32::try_from(size.0).ok()?, i32::try_from(size.1).ok()?);
    drop(guard);
    if result.is_ok() {
        player.frames += 1;
        player.render_time += start.elapsed();
    }
    presented
}

fn request_redraw(window: &slint::Weak<ProbeWindow>) {
    let weak = window.clone();
    let _ = weak.upgrade_in_event_loop(|w| w.window().request_redraw());
}

fn grid(secs: u64, files: &[String]) {
    let window = ProbeWindow::new().expect("window");
    let count = files.len().clamp(1, 4);
    let columns = if count == 1 { 1 } else { 2 };
    window.set_columns(i32::try_from(columns).unwrap_or(1));
    window.set_caption(format!("render API: {count} video(s) - this caption is a plain Slint element over the video").into());
    let players: Vec<Rc<RefCell<Player>>> = (0..count)
        .map(|_| {
            Rc::new(RefCell::new(Player {
                mpv: new_mpv(),
                render: None,
                target: None,
                frames: 0,
                render_time: Duration::ZERO,
            }))
        })
        .collect();
    let images = Rc::new(slint::VecModel::from(vec![slint::Image::default(); count]));
    window.set_frames(slint::ModelRc::from(images.clone()));
    let files: Vec<String> = files.iter().take(count).cloned().collect();
    let slint_frames = Rc::new(RefCell::new(0u64));
    let mut gpu: Option<WindowGl> = None;
    let weak = window.as_weak();
    {
        let players = players.clone();
        let slint_frames = slint_frames.clone();
        let images = images.clone();
        window
            .window()
            .set_rendering_notifier(move |state, api| match state {
                RenderingState::RenderingSetup => {
                    let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
                        eprintln!("not OpenGL");
                        return;
                    };
                    let gl = gl::load(*get_proc_address);
                    println!("GL: {}", gl::describe(&gl));
                    for (player, file) in players.iter().zip(&files) {
                        let mut p = player.borrow_mut();
                        let mut render = MpvRender::create(Arc::clone(&p.mpv), *get_proc_address).expect("render ctx");
                        let w = weak.clone();
                        render.set_update_callback(move || request_redraw(&w));
                        p.render = Some(render);
                        p.mpv.command("loadfile", &[file.as_str(), "replace"]).expect("loadfile");
                        p.mpv.set_property("loop-file", "inf").ok();
                    }
                    gpu = Some(WindowGl { gl });
                }
                RenderingState::BeforeRendering => {
                    let Some(g) = gpu.as_ref() else { return };
                    *slint_frames.borrow_mut() += 1;
                    let Some(win) = weak.upgrade() else { return };
                    let size = tile_size(win.window().size(), count, columns);
                    for (i, player) in players.iter().enumerate() {
                        if let Some(image) = draw(&g.gl, &mut player.borrow_mut(), size) {
                            images.set_row_data(i, image);
                        }
                    }
                }
                RenderingState::AfterRendering => {
                    for player in &players {
                        if let Some(render) = player.borrow().render.as_ref() {
                            render.report_swap();
                        }
                    }
                }
                RenderingState::RenderingTeardown => {
                    if let Some(g) = gpu.take() {
                        for player in &players {
                            let mut p = player.borrow_mut();
                            if let Some(t) = p.target.take() {
                                t.delete(&g.gl);
                            }
                            p.render = None;
                        }
                    }
                }
                _ => {}
            })
            .expect("notifier");
    }
    let started = Instant::now();
    let report = slint::Timer::default();
    {
        let players = players.clone();
        let slint_frames = slint_frames.clone();
        let mut last = (Instant::now(), 0u64, vec![0u64; count]);
        report.start(slint::TimerMode::Repeated, Duration::from_secs(5), move || {
            let now = Instant::now();
            let dt = now.duration_since(last.0).as_secs_f64();
            let sf = *slint_frames.borrow();
            let per: Vec<String> = players
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let p = p.borrow();
                    let fps = (p.frames - last.2[i]) as f64 / dt;
                    let drops = p.mpv.get_property::<i64>("frame-drop-count").unwrap_or(-1);
                    let vo_drops = p.mpv.get_property::<i64>("vo-drop-frame-count").unwrap_or(-1);
                    last.2[i] = p.frames;
                    format!("v{i} {fps:.1} fps (dec drops {drops}, vo drops {vo_drops})")
                })
                .collect();
            println!(
                "t={:>4.0}s slint {:.1} fps | {}",
                started.elapsed().as_secs_f64(),
                (sf - last.1) as f64 / dt,
                per.join(" | ")
            );
            last.0 = now;
            last.1 = sf;
        });
    }
    let quit = slint::Timer::default();
    quit.start(slint::TimerMode::SingleShot, Duration::from_secs(secs), || {
        let _ = slint::quit_event_loop();
    });
    window.run().expect("run");
    for (i, p) in players.iter().enumerate() {
        let p = p.borrow();
        let ms = if p.frames > 0 { p.render_time.as_secs_f64() * 1000.0 / p.frames as f64 } else { 0.0 };
        println!("summary v{i}: {} frames, {:.2} ms mean mpv render", p.frames, ms);
    }
}

/// Which window shows the player of the handover probe, and what the probe measured.
#[derive(Default)]
struct Handover {
    /// 0 = window A, 1 = window B.
    owner: usize,
    /// The player has to leave its window (its render context is freed there on the next frame).
    leave: bool,
    left_at: Option<Instant>,
    pos_before: f64,
    /// The video track before the old render context went (mpv sets `vid` to "no" when it frees it).
    vid_before: String,
    log: Vec<String>,
}

fn handover(file: &str) {
    let windows = [ProbeWindow::new().expect("A"), ProbeWindow::new().expect("B")];
    windows[0].set_caption("window A".into());
    windows[1].set_caption("window B".into());
    let player = Rc::new(RefCell::new(Player {
        mpv: new_mpv(),
        render: None,
        target: None,
        frames: 0,
        render_time: Duration::ZERO,
    }));
    let state = Rc::new(RefCell::new(Handover::default()));
    let file = file.to_string();
    let weaks = [windows[0].as_weak(), windows[1].as_weak()];
    for (me, window) in windows.iter().enumerate() {
        let images = Rc::new(slint::VecModel::from(vec![slint::Image::default()]));
        window.set_frames(slint::ModelRc::from(images.clone()));
        let player = player.clone();
        let state = state.clone();
        let file = file.clone();
        let weaks = weaks.clone();
        let mut gl_here: Option<glow::Context> = None;
        window
            .window()
            .set_rendering_notifier(move |rendering, api| match rendering {
                RenderingState::RenderingSetup => {
                    let GraphicsAPI::NativeOpenGL { get_proc_address } = api else { return };
                    gl_here = Some(gl::load(*get_proc_address));
                }
                RenderingState::BeforeRendering => {
                    let Some(gl) = gl_here.as_ref() else { return };
                    let GraphicsAPI::NativeOpenGL { get_proc_address } = api else { return };
                    let mut st = state.borrow_mut();
                    let mut p = player.borrow_mut();
                    if st.owner == me && st.leave && p.render.is_some() {
                        // Free in this window's context, then let the other window take over.
                        if let Some(t) = p.target.take() {
                            t.delete(gl);
                        }
                        st.pos_before = p.mpv.get_property::<f64>("time-pos").unwrap_or(-1.0);
                        st.vid_before = p.mpv.get_property::<String>("vid").unwrap_or_default();
                        p.render = None;
                        st.left_at = Some(Instant::now());
                        st.leave = false;
                        st.owner = 1 - me;
                        let vid = p.mpv.get_property::<String>("vid").unwrap_or_default();
                        let line = format!("freed in window {me}: time-pos {:.3}, vid after free '{vid}'", st.pos_before);
                        st.log.push(line);
                        images.set_row_data(0, slint::Image::default());
                        request_redraw(&weaks[1 - me]);
                        return;
                    }
                    if st.owner != me {
                        return;
                    }
                    if p.render.is_none() {
                        let mut render = MpvRender::create(Arc::clone(&p.mpv), *get_proc_address).expect("render ctx");
                        let w = weaks[me].clone();
                        render.set_update_callback(move || request_redraw(&w));
                        p.render = Some(render);
                        if st.left_at.is_none() {
                            p.mpv.command("loadfile", &[file.as_str(), "replace"]).expect("loadfile");
                            p.mpv.set_property("loop-file", "inf").ok();
                        } else {
                            // Video was disabled when the old context went ("vid" = "no"): select the track again.
                            let vid = st.vid_before.clone();
                            let set = p.mpv.set_property("vid", vid.as_str());
                            st.log.push(format!("render context created in window {me}; vid back to '{vid}': {set:?}"));
                        }
                        p.frames = 0;
                    }
                    let Some(win) = weaks[me].upgrade() else { return };
                    let size = tile_size(win.window().size(), 1, 1);
                    let before = p.frames;
                    if let Some(image) = draw(gl, &mut p, size) {
                        images.set_row_data(0, image);
                    }
                    let video_up = p.mpv.get_property::<i64>("video-params/w").unwrap_or(0) > 0;
                    if before < p.frames && video_up && st.left_at.is_some_and(|at| at.elapsed() < Duration::from_secs(100)) {
                        if let Some(at) = st.left_at {
                            let pos = p.mpv.get_property::<f64>("time-pos").unwrap_or(-1.0);
                            let paused = p.mpv.get_property::<bool>("pause").unwrap_or(false);
                            let line = format!(
                                "first frame in window {me} after {:.0} ms; time-pos {pos:.3} (before {:.3}), paused {paused}",
                                at.elapsed().as_secs_f64() * 1000.0,
                                st.pos_before
                            );
                            st.log.push(line);
                            st.left_at = None;
                        }
                    }
                }
                RenderingState::AfterRendering => {
                    if state.borrow().owner == me
                        && let Some(render) = player.borrow().render.as_ref()
                    {
                        render.report_swap();
                    }
                }
                RenderingState::RenderingTeardown => {
                    if let Some(gl) = gl_here.take() {
                        let mut p = player.borrow_mut();
                        if state.borrow().owner == me {
                            if let Some(t) = p.target.take() {
                                t.delete(&gl);
                            }
                            p.render = None;
                        }
                    }
                }
                _ => {}
            })
            .expect("notifier");
    }
    windows[0].show().expect("show A");
    windows[1].show().expect("show B");
    let steps = slint::Timer::default();
    let step = Rc::new(RefCell::new(0));
    {
        let state = state.clone();
        let player = player.clone();
        let weaks = weaks.clone();
        steps.start(slint::TimerMode::Repeated, Duration::from_secs(3), move || {
            let mut n = step.borrow_mut();
            *n += 1;
            match *n {
                1 => {
                    state.borrow_mut().leave = true;
                    request_redraw(&weaks[state.borrow().owner]);
                }
                2 => {
                    let p = player.borrow();
                    p.mpv.set_property("pause", true).ok();
                    state.borrow_mut().log.push(format!(
                        "paused in window B at {:.3}",
                        p.mpv.get_property::<f64>("time-pos").unwrap_or(-1.0)
                    ));
                    drop(p);
                    state.borrow_mut().leave = true;
                    request_redraw(&weaks[state.borrow().owner]);
                }
                _ => {
                    let _ = slint::quit_event_loop();
                }
            }
        });
    }
    slint::run_event_loop().expect("run");
    let p = player.borrow();
    let pos = p.mpv.get_property::<f64>("time-pos").unwrap_or(-1.0);
    let paused = p.mpv.get_property::<bool>("pause").unwrap_or(false);
    drop(p);
    for line in &state.borrow().log {
        println!("{line}");
    }
    println!("end: time-pos {pos:.3}, paused {paused}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("grid") if args.len() >= 4 => grid(args[2].parse().unwrap_or(20), &args[3..]),
        Some("handover") if args.len() >= 3 => handover(&args[2]),
        _ => eprintln!("usage: render-probe grid SECS FILE... | render-probe handover FILE"),
    }
}
