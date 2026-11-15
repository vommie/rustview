//! Runs the steps of a plan on the UI thread, one 8 ms timer for the whole run (about two pointer events per frame at 60 Hz,
//! like a mouse). Every step uses the same callbacks and events as the user's own input.

use super::plan::{Anchor, FolderWait, Step, ViewKind};
use super::{input, pointing, recorder, tiles};
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};
use slint::ComponentHandle;
use slint::platform::WindowEvent;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const TICK: Duration = Duration::from_millis(8);
/// How long nothing may work in the background before a waiting step goes on.
const QUIET: Duration = Duration::from_millis(1000);
const READY_TIMEOUT: Duration = Duration::from_secs(180);
const LIST_TIMEOUT: Duration = Duration::from_secs(60);
/// A scan with thumbnail generation of a very large folder may take hours on a slow machine.
const INDEX_TIMEOUT: Duration = Duration::from_hours(6);
/// How long a step waits for the frame that shows its change (nothing is drawn when nothing changed).
const FRAME_WAIT: Duration = Duration::from_millis(400);
/// Tiles whose thumbnail is not stored never fill; the step goes on after this time and says so in the log.
const THUMBS_TIMEOUT: Duration = Duration::from_secs(10);
/// How long the fullscreen of a Multiview may take to show its media.
const FULLSCREEN_TIMEOUT: Duration = Duration::from_secs(15);

pub struct Context {
    pub window: slint::Weak<MainWindow>,
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub settings: Arc<SettingsManager>,
    pub state: Arc<RwLock<UiState>>,
}

enum Running {
    Ready {
        started: Instant,
        quiet_since: Option<Instant>,
    },
    Settle {
        until: Instant,
    },
    /// Background work (a scan, thumbnail generation, a tree refresh) has to end.
    Work {
        started: Instant,
        quiet_since: Option<Instant>,
        event: &'static str,
    },
    /// A list has to be committed (when `commits_before` is given), a frame has to show the new place and its visible
    /// thumbnails have to be there.
    Shown {
        started: Instant,
        commits_before: Option<u64>,
        frames_before: u64,
    },
    Scroll {
        px_per_s: f32,
        last: Instant,
        until: Instant,
    },
    Wheel {
        per_s: f32,
        last: Instant,
        until: Instant,
        owed: f32,
    },
    Sweep {
        origin: (f32, f32),
        dx: f32,
        dy: f32,
        started: Instant,
        seconds: f32,
        cycles: u32,
    },
    MoveTo {
        from: (f32, f32),
        to: (f32, f32),
        started: Instant,
        seconds: f32,
    },
    /// The fullscreen of a Multiview has to show its media.
    Fullscreen {
        started: Instant,
    },
}

enum Begun {
    Done,
    Running(Running),
    Quit,
    Failed(&'static str),
}

pub struct Driver {
    pub(super) ctx: Context,
    steps: VecDeque<Step>,
    step_number: u64,
    running: Option<Running>,
    pub(super) pointer: (f32, f32),
    /// Pointer and key steps go to the fullscreen window of the Multiview.
    pub(super) fullscreen: bool,
    /// The keys the plan gave to actions (`bind`).
    binds: Vec<(String, String)>,
    scenario: Option<String>,
}

impl Driver {
    pub fn new(ctx: Context, steps: Vec<Step>) -> Self {
        Self {
            ctx,
            steps: steps.into(),
            step_number: 0,
            running: None,
            pointer: (0.0, 0.0),
            fullscreen: false,
            binds: Vec::new(),
            scenario: None,
        }
    }

    /// One tick; `true` when the run is over.
    pub fn tick(&mut self) -> bool {
        let Some(window) = self.ctx.window.upgrade() else {
            return true;
        };
        let now = Instant::now();
        if let Some(running) = self.running.take() {
            match self.advance(running, &window, now) {
                Ok(Some(still)) => {
                    self.running = Some(still);
                    return false;
                }
                Ok(None) => {}
                Err(reason) => return self.fail(&window, reason),
            }
        }
        while let Some(step) = self.steps.pop_front() {
            self.step_number += 1;
            match self.begin(step, &window, now) {
                Begun::Done => {}
                Begun::Running(running) => {
                    self.running = Some(running);
                    return false;
                }
                Begun::Quit => return true,
                Begun::Failed(reason) => return self.fail(&window, reason),
            }
        }
        recorder::flush();
        true
    }

    fn fail(&mut self, window: &MainWindow, reason: &'static str) -> bool {
        tracing::error!(step = self.step_number, reason, "[PerfProbe] The run stopped");
        recorder::event("probe_failed_at_step", self.step_number);
        self.steps.clear();
        Self::quit(window);
        true
    }

    fn quit(window: &MainWindow) {
        recorder::flush();
        window.window().dispatch_event(WindowEvent::CloseRequested);
    }

    fn busy(&self, window: &MainWindow) -> bool {
        BridgeMedia::is_system_busy(window, &self.ctx.thumbnails, &self.ctx.state)
    }

    fn key(&self, window: &MainWindow, name: &str, down: bool, up: bool) -> Begun {
        if self.with_target(window, |target| input::key(target, name, down, up)) {
            Begun::Done
        } else {
            Begun::Failed("unknown key")
        }
    }

    fn begin(&mut self, step: Step, window: &MainWindow, now: Instant) -> Begun {
        match step {
            Step::Ready => Begun::Running(Running::Ready {
                started: now,
                quiet_since: None,
            }),
            Step::Settle(time) => Begun::Running(Running::Settle { until: now + time }),
            Step::Begin(name) => {
                recorder::mark(true, &name);
                self.scenario = Some(name);
                Begun::Done
            }
            Step::End => match self.scenario.take() {
                Some(name) => {
                    recorder::mark(false, &name);
                    Begun::Done
                }
                None => Begun::Failed("end without begin"),
            },
            Step::Tree => {
                window.invoke_folder_refresh_all_clicked();
                Begun::Running(Running::Work {
                    started: now,
                    quiet_since: None,
                    event: "tree_refresh_us",
                })
            }
            Step::Folder { name, wait } => {
                let commits_before = super::commits();
                if !self.open_folder(window, &name) {
                    return Begun::Failed("no folder with that name");
                }
                match wait {
                    FolderWait::Indexed => Begun::Running(Running::Work {
                        started: now,
                        quiet_since: None,
                        event: "folder_index_us",
                    }),
                    FolderWait::Shown => Begun::Running(Running::Shown {
                        started: now,
                        commits_before: Some(commits_before),
                        frames_before: recorder::frames(),
                    }),
                    FolderWait::Nothing => Begun::Done,
                }
            }
            Step::View(kind) => {
                if window.get_is_table_view() != (kind == ViewKind::Table) {
                    window.invoke_toggle_table_view();
                }
                Begun::Running(Running::Shown {
                    started: now,
                    commits_before: None,
                    frames_before: recorder::frames(),
                })
            }
            Step::Sort { column, stacked } => {
                let commits_before = super::commits();
                recorder::event("sort_click", column.saturating_as());
                window.invoke_sort_column_clicked(column, stacked);
                Begun::Running(Running::Shown {
                    started: now,
                    commits_before: Some(commits_before),
                    frames_before: recorder::frames(),
                })
            }
            Step::Scroll { px_per_s, seconds } => Begun::Running(Running::Scroll {
                px_per_s,
                last: now,
                until: now + Duration::from_secs_f32(seconds),
            }),
            Step::Wheel {
                notches_per_s,
                seconds,
                here,
            } => {
                if !here {
                    let view = Anchor {
                        name: "view".to_owned(),
                        index: 0,
                    };
                    let Some(point) = self.anchor(window, &view) else {
                        return Begun::Failed("the view has no place");
                    };
                    self.pointer = point;
                }
                self.move_pointer(window);
                Begun::Running(Running::Wheel {
                    per_s: notches_per_s,
                    last: now,
                    until: now + Duration::from_secs_f32(seconds),
                    owed: 0.0,
                })
            }
            Step::Jump(fraction) => {
                let last = self.ctx.state.read().view_len().saturating_sub(1);
                let target = (last.az::<f32>() * fraction).to_int::<i32>();
                recorder::event("jump_to", target.saturating_as());
                recorder::note_input();
                window.invoke_scroll_media_to_index(target, window.get_is_table_view());
                Begun::Running(Running::Shown {
                    started: now,
                    commits_before: None,
                    frames_before: recorder::frames(),
                })
            }
            Step::Press(anchor) => match self.anchor(window, &anchor) {
                Some(point) => {
                    self.pointer = point;
                    self.move_pointer(window);
                    self.with_target(window, |target| input::press(target, point));
                    Begun::Done
                }
                None => Begun::Failed("unknown element"),
            },
            Step::DoubleClick(anchor) => match self.anchor(window, &anchor) {
                Some(point) => {
                    self.pointer = point;
                    self.move_pointer(window);
                    self.with_target(window, |target| {
                        for _ in 0..2 {
                            input::press(target, point);
                            input::release(target, point);
                        }
                    });
                    Begun::Done
                }
                None => Begun::Failed("unknown element"),
            },
            Step::Sweep {
                dx,
                dy,
                seconds,
                cycles,
            } => Begun::Running(Running::Sweep {
                origin: self.pointer,
                dx,
                dy,
                started: now,
                seconds,
                cycles,
            }),
            Step::MoveTo { anchor, seconds } => match self.anchor(window, &anchor) {
                Some(to) => Begun::Running(Running::MoveTo {
                    from: self.pointer,
                    to,
                    started: now,
                    seconds,
                }),
                None => Begun::Failed("unknown element"),
            },
            Step::Drag { dx, dy, seconds } => Begun::Running(Running::MoveTo {
                from: self.pointer,
                to: (self.pointer.0 + dx, self.pointer.1 + dy),
                started: now,
                seconds,
            }),
            Step::RightClick(anchor) => match self.anchor(window, &anchor) {
                Some(point) => {
                    self.pointer = point;
                    self.move_pointer(window);
                    self.with_target(window, |target| input::right_click(target, point));
                    Begun::Done
                }
                None => Begun::Failed("unknown element"),
            },
            Step::ClickBy { dx, dy } => {
                self.pointer = (self.pointer.0 + dx, self.pointer.1 + dy);
                let point = self.pointer;
                self.move_pointer(window);
                self.with_target(window, |target| {
                    input::press(target, point);
                    input::release(target, point);
                });
                Begun::Done
            }
            Step::Release => {
                // A media drag released over no folder is offered to the window under the REAL pointer (XDND), which may be
                // another application: the run stops instead, and closing the window drops nothing.
                if window.get_is_dragging_media()
                    && window.get_drop_target_folder_id() <= 0
                    && window.get_drop_target_row_id() == 0
                {
                    return Begun::Failed("the media drag is over no folder");
                }
                let point = self.pointer;
                self.with_target(window, |target| input::release(target, point));
                Begun::Done
            }
            Step::Key(name) => self.key(window, &name, true, true),
            Step::KeyDown(name) => self.key(window, &name, true, false),
            Step::KeyUp(name) => self.key(window, &name, false, true),
            Step::Bind { action, key } => {
                if crate::ui::keymap::Action::from_id(&action).is_none() {
                    return Begun::Failed("unknown action");
                }
                let mut stored = self.ctx.settings.get().hotkeys.clone();
                self.binds.push((action, key));
                for (action, key) in &self.binds {
                    stored.insert(action.clone(), serde_json::json!([key]));
                }
                crate::ui::hotkeys::apply(&stored);
                Begun::Done
            }
            Step::Window(false) => {
                self.fullscreen = false;
                Begun::Done
            }
            Step::Window(true) => Begun::Running(Running::Fullscreen { started: now }),
            Step::Shot(name) => {
                self.with_target(window, |target| super::shot(target, &name));
                Begun::Done
            }
            Step::Quit => {
                Self::quit(window);
                Begun::Quit
            }
        }
    }

    /// Goes on with a step that takes time; `None` when it is done.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "the state of the step is replaced by the one that is returned"
    )]
    fn advance(
        &mut self,
        running: Running,
        window: &MainWindow,
        now: Instant,
    ) -> Result<Option<Running>, &'static str> {
        match running {
            Running::Ready { started, quiet_since } => {
                let quiet_since = if self.busy(window) {
                    None
                } else {
                    Some(quiet_since.unwrap_or(now))
                };
                if quiet_since.is_some_and(|since| now - since >= QUIET) {
                    let size = window.window().size();
                    recorder::info("window", format_args!("{}x{}", size.width, size.height));
                    recorder::info("scale", window.window().scale_factor());
                    return Ok(None);
                }
                if now - started > READY_TIMEOUT {
                    return Err("the application did not get ready");
                }
                Ok(Some(Running::Ready { started, quiet_since }))
            }
            Running::Settle { until } => Ok((now < until).then_some(Running::Settle { until })),
            Running::Work {
                started,
                quiet_since,
                event,
            } => {
                let quiet_since = if self.busy(window) {
                    None
                } else {
                    Some(quiet_since.unwrap_or(now))
                };
                if let Some(since) = quiet_since
                    && now - since >= QUIET
                    && now - started >= QUIET
                {
                    recorder::event(event, (since - started).as_micros().saturating_as());
                    return Ok(None);
                }
                if now - started > INDEX_TIMEOUT {
                    return Err("the background work did not end");
                }
                Ok(Some(Running::Work {
                    started,
                    quiet_since,
                    event,
                }))
            }
            Running::Shown {
                started,
                commits_before,
                frames_before,
            } => {
                if commits_before.is_some_and(|before| super::commits() == before) {
                    if now - started > LIST_TIMEOUT {
                        return Err("the list was not shown");
                    }
                    return Ok(Some(Running::Shown {
                        started,
                        commits_before,
                        frames_before,
                    }));
                }
                // The view reports its new place with the next turn of the event loop: the tiles are counted only after a
                // frame has shown it.
                if recorder::frames() == frames_before && now - started < FRAME_WAIT {
                    return Ok(Some(Running::Shown {
                        started,
                        commits_before: None,
                        frames_before,
                    }));
                }
                match tiles::count(window, &self.ctx.state) {
                    Some((blank, _)) if blank > 0 => {
                        if now - started > THUMBS_TIMEOUT {
                            recorder::event("thumbnails_missing_after_wait", u64::from(blank));
                            return Ok(None);
                        }
                        Ok(Some(Running::Shown {
                            started,
                            commits_before: None,
                            frames_before,
                        }))
                    }
                    _ => Ok(None),
                }
            }
            Running::Scroll { px_per_s, last, until } => {
                recorder::note_input();
                window.invoke_scroll_media_by(-px_per_s * (now - last).as_secs_f32());
                Ok((now < until).then_some(Running::Scroll {
                    px_per_s,
                    last: now,
                    until,
                }))
            }
            Running::Wheel {
                per_s,
                last,
                until,
                mut owed,
            } => {
                owed += per_s.abs() * (now - last).as_secs_f32();
                let point = self.pointer;
                while owed >= 1.0 {
                    self.with_target(window, |target| input::wheel(target, point, per_s > 0.0));
                    owed -= 1.0;
                }
                Ok((now < until).then_some(Running::Wheel {
                    per_s,
                    last: now,
                    until,
                    owed,
                }))
            }
            Running::Sweep {
                origin,
                dx,
                dy,
                started,
                seconds,
                cycles,
            } => {
                let progress = (now - started).as_secs_f32() / seconds;
                let reach = input::triangle(progress, cycles);
                self.pointer = (origin.0 + dx * reach, origin.1 + dy * reach);
                self.move_pointer(window);
                Ok((progress < 1.0).then_some(Running::Sweep {
                    origin,
                    dx,
                    dy,
                    started,
                    seconds,
                    cycles,
                }))
            }
            Running::MoveTo {
                from,
                to,
                started,
                seconds,
            } => {
                let progress = ((now - started).as_secs_f32() / seconds).min(1.0);
                self.pointer = (from.0 + (to.0 - from.0) * progress, from.1 + (to.1 - from.1) * progress);
                self.move_pointer(window);
                Ok((progress < 1.0).then_some(Running::MoveTo {
                    from,
                    to,
                    started,
                    seconds,
                }))
            }
            Running::Fullscreen { started } => {
                if pointing::fullscreen_shown() {
                    self.fullscreen = true;
                    return Ok(None);
                }
                if now - started > FULLSCREEN_TIMEOUT {
                    return Err("the fullscreen did not open");
                }
                Ok(Some(Running::Fullscreen { started }))
            }
        }
    }
}
