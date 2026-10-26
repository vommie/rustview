use super::error::ViewerError;
use super::types::MpvPlaybackState;
use libmpv2::Mpv;
use parking_lot::{Mutex, RwLock};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub mod ab_repeat;
mod controls;
mod events;
mod messages;
mod options;
mod playback;
mod render;
pub use controls::SeekPrecision;
pub use playback::PlaybackNumbers;
pub use render::{MpvRenderContext, NativeDisplay, RenderCore};
pub mod self_test;
mod zoom;

/// The players that exist now (Settings > Debug, Runtime).
static LIVE_PLAYERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many video players exist now.
pub fn live_players() -> usize {
    LIVE_PLAYERS.load(Ordering::Relaxed)
}

/// What a player reports to its owner, from its observer thread (`MpvPlayer::on_notice`).
#[derive(Debug, Clone, PartialEq)]
pub enum MpvNotice {
    /// The mute state changed, whatever caused it.
    Mute(bool),
    /// The user's pause state changed (`true` = paused). A pause that only comes from the view being hidden is not the user's
    /// and is not reported.
    UserPause(bool),
    /// Pause, end of file, idle state, duration or volume changed (the numbers are in `MpvPlayer::playback`). The position is
    /// not reported: it changes with every frame.
    Playback,
    /// mpv knows the display size of the loaded file (`MpvPlayer::video_display_params`).
    VideoParams,
    /// The file cannot be played (mpv's reason), or mpv stopped by itself.
    Failed(String),
    /// The script `ab_repeat.lua` published a change (its support, A-B or saved repeats).
    AbRepeat(ab_repeat::AbChange),
}

pub type MpvNoticeCallback = Arc<dyn Fn(MpvNotice) + Send + Sync>;

/// One video player: `libmpv` with the user's configuration and scripts (`options`), observed by a thread of its own
/// (`events`). The video is drawn through mpv's render API into textures that Slint shows (`viewer::video_surface`); mpv has no
/// window, takes no input and shows no controller of its own.
pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    state: Arc<RwLock<MpvPlaybackState>>,
    params: Arc<RwLock<events::VideoParams>>,
    current_file_path: Arc<RwLock<Option<PathBuf>>>,
    is_view_active: Arc<AtomicBool>,
    is_user_paused: Arc<AtomicBool>,
    is_tab_grid_mode: Arc<AtomicBool>,
    observer_running: Arc<AtomicBool>,
    listener: Arc<RwLock<Option<MpvNoticeCallback>>>,
    /// What the player knows about the mpv script `ab_repeat.lua` (`viewer::ab_repeat`).
    ab: Arc<RwLock<ab_repeat::AbSnapshot>>,
    /// The player in the list of `ab_repeat` (forwarding changes of saved repeats, applying settings).
    ab_registration: u64,
    observer_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MpvPlayer {
    /// Creates a player with the user's mpv configuration and scripts. Its video output is `libmpv`: nothing is shown until a
    /// video surface created its render context (`viewer::video_surface`), and a file must not be loaded before that.
    pub fn new() -> Result<Self, ViewerError> {
        let t0 = Instant::now();

        let mpv_instance = Mpv::with_initializer(|init| {
            options::before_init(&init);
            Ok(())
        })
        .or_else(|_| Mpv::new())
        .map_err(|e| ViewerError::Mpv(format!("Failed to initialize libmpv2 context: {e:?}")))?;
        options::after_init(&mpv_instance);

        // mpv's warnings and errors go into the log (everything into mpv.log while it is switched on)
        messages::request(&mpv_instance);

        let mpv_arc = Arc::new(mpv_instance);
        let ab = Arc::new(RwLock::new(ab_repeat::AbSnapshot::default()));
        let listener: Arc<RwLock<Option<MpvNoticeCallback>>> = Arc::new(RwLock::new(None));
        let ab_registration = ab_repeat::register(&mpv_arc, &ab, &listener);
        ab_repeat::watch(&mpv_arc, &ab, crate::viewer::ab_repeat::prefs::current().enabled);

        let player = Self {
            mpv: mpv_arc,
            state: Arc::new(RwLock::new(MpvPlaybackState::default())),
            params: Arc::new(RwLock::new(events::VideoParams::default())),
            current_file_path: Arc::new(RwLock::new(None)),
            is_view_active: Arc::new(AtomicBool::new(true)),
            is_user_paused: Arc::new(AtomicBool::new(false)),
            is_tab_grid_mode: Arc::new(AtomicBool::new(false)),
            observer_running: Arc::new(AtomicBool::new(true)),
            listener,
            ab,
            ab_registration,
            observer_handle: Mutex::new(None),
        };

        player.spawn_event_observer_thread();
        LIVE_PLAYERS.fetch_add(1, Ordering::Relaxed);

        tracing::info!(
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Init] libmpv2 player engine initialized (video output: render API)"
        );

        Ok(player)
    }

    /// Marks the player as shown in the tab grid (or not). The flag keeps a pause the grid causes from counting as the user's;
    /// mpv takes no input in either case (`RustView` handles the pointer and the keys over every video, the grid too).
    pub fn set_tab_grid_mode(&self, grid_mode: bool) {
        if self.is_tab_grid_mode.swap(grid_mode, Ordering::SeqCst) == grid_mode {
            return;
        }
        tracing::info!(grid_mode, "[MpvPlayer: GridMode] Tab-Grid mode changed");
    }

    /// Controls active visibility of this video tab. Automatically manages auto-pause and auto-resume without overwriting user pause state.
    pub fn set_view_active(&self, active: bool) {
        self.is_view_active.store(active, Ordering::SeqCst);

        if active {
            let user_paused = self.is_user_paused.load(Ordering::SeqCst);
            if user_paused {
                let _ = self.mpv.set_property("pause", true);
                tracing::debug!("[MpvPlayer: StayPaused] Foregrounded video tab kept paused (user-paused)");
            } else {
                let _ = self.mpv.set_property("pause", false);
                tracing::debug!("[MpvPlayer: AutoResume] Foregrounded video tab auto-resumed (playback active)");
            }
        } else {
            let _ = self.mpv.set_property("pause", true);
            tracing::debug!(
                user_paused = self.is_user_paused.load(Ordering::SeqCst),
                "[MpvPlayer: AutoPause] Video tab backgrounded and auto-paused"
            );
        }
    }

    #[inline]
    pub fn set_user_paused(&self, user_paused: bool) {
        self.is_user_paused.store(user_paused, Ordering::SeqCst);
        self.state.write().is_user_paused = user_paused;
    }

    #[inline]
    pub fn is_view_active(&self) -> bool {
        self.is_view_active.load(Ordering::SeqCst)
    }

    #[inline]
    pub fn is_user_paused(&self) -> bool {
        self.is_user_paused.load(Ordering::SeqCst)
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        LIVE_PLAYERS.fetch_sub(1, Ordering::Relaxed);
        ab_repeat::unregister(self.ab_registration);
        self.observer_running.store(false, Ordering::Release);
        self.wake_observer();
        let _ = self.mpv.command("quit", &[]);
    }
}
