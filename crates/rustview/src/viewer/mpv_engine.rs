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
mod playback;
mod render;
mod still;
pub use controls::SeekPrecision;
pub use playback::PlaybackNumbers;
pub use render::{MpvRenderContext, NativeDisplay, RenderCore};
pub mod self_test;
mod zoom;

pub type MpvEventCallback = Arc<dyn Fn(&MpvPlaybackState) + Send + Sync>;

/// The players that exist now (Settings > Debug, Runtime).
static LIVE_PLAYERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many video players exist now.
pub fn live_players() -> usize {
    LIVE_PLAYERS.load(Ordering::Relaxed)
}

/// Called from the observer thread when the mute state changed (a hotkey, the on-screen controller or `set_mute`).
pub type MpvMuteCallback = Arc<dyn Fn(bool) + Send + Sync>;
/// Called from the observer thread when the user's pause state changed (`true` = paused). A pause that only comes from
/// the view being hidden is not the user's and is not reported.
pub type MpvPauseCallback = Arc<dyn Fn(bool) + Send + Sync>;

/// Central thread-safe MPV player engine wrapping `libmpv2` with plugins, custom config files and property observation. The video
/// is drawn through mpv's render API into textures that Slint shows (`viewer::video_surface`); mpv has no window of its own.
pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    state: Arc<RwLock<MpvPlaybackState>>,
    current_file_path: Arc<RwLock<Option<PathBuf>>>,
    is_view_active: Arc<AtomicBool>,
    is_user_paused: Arc<AtomicBool>,
    is_tab_grid_mode: Arc<AtomicBool>,
    observer_running: Arc<AtomicBool>,
    callbacks: Arc<RwLock<Vec<MpvEventCallback>>>,
    mute_callbacks: Arc<RwLock<Vec<MpvMuteCallback>>>,
    pause_callbacks: Arc<RwLock<Vec<MpvPauseCallback>>>,
    /// What the player knows about the mpv script `ab_repeat.lua` (`viewer::ab_repeat`), and who listens to its changes.
    ab: Arc<RwLock<ab_repeat::AbSnapshot>>,
    ab_callbacks: Arc<RwLock<Vec<ab_repeat::MpvAbCallback>>>,
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
            // 1. Hardware acceleration & rendering: mpv's render API (the video surface's render thread draws every frame)
            let _ = init.set_property("vo", "libmpv");
            let _ = init.set_property("hwdec", "auto-safe");

            // 2. Audio & playback lifecycle
            let _ = init.set_property("keep-open", "yes");
            let _ = init.set_property("idle", "yes");

            // 3. No on-screen controller: RustView takes the pointer over videos and draws a control bar of its own
            // (`viewer::multiview::video_bar`), so mpv's would never see the mouse. The OSD bar stays.
            let _ = init.set_property("osc", "no");
            let _ = init.set_property("osd-bar", "yes");

            // 4. Configure OSC behavior: Disable top deadzone so HUD triggers across 100% of video height
            let _ = init.set_property("script-opts", ab_repeat::script_opts().as_str());

            // 5. Input: mpv has no window, so it never sees a key or the mouse; RustView handles both over a video
            // (`docs/hotkeys.md`). Scripts may still bind keys for their own commands (`ab_repeat.lua`).
            let _ = init.set_property("input-default-bindings", "yes");

            // 6. Enable loading of user config files (mpv.conf, input.conf, script-opts, etc.)
            let _ = init.set_property("config", "yes");

            // 7. Enable loading of user scripts & Lua/JS plugins (~/.config/mpv/scripts/)
            let _ = init.set_property("load-scripts", "yes");
            let _ = init.set_property("load-osd-console", "yes");
            let _ = init.set_property("load-auto-profiles", "yes");
            let _ = init.set_property("ytdl", "yes");

            // 8. Resolve and set MPV configuration directory
            let config_dir = directories::BaseDirs::new()
                .map_or_else(|| PathBuf::from("."), |dirs| dirs.config_dir().to_path_buf())
                .join("mpv");

            if config_dir.exists() {
                let _ = init.set_property("config-dir", config_dir.to_string_lossy().as_ref());
                tracing::info!(
                    config_dir = %config_dir.display(),
                    "[MpvPlayer: Init] Loaded user MPV configuration and plugin directory"
                );
            }

            Ok(())
        })
        .or_else(|_| Mpv::new())
        .map_err(|e| ViewerError::Mpv(format!("Failed to initialize libmpv2 context: {e:?}")))?;

        // Ensure OSC deadzone is completely zeroed out
        let _ = mpv_instance.set_property("script-opts", ab_repeat::script_opts().as_str());

        // The user's mpv.conf is read after the options above (`osc=yes` there loads mpv's controller again, which never sees
        // the pointer but runs); setting it now unloads that script.
        if mpv_instance.get_property::<bool>("osc").unwrap_or(false) {
            match mpv_instance.set_property("osc", false) {
                Ok(()) => tracing::info!(
                    "[MpvPlayer: Init] mpv's on-screen controller switched off again (the user's mpv.conf had it on)"
                ),
                Err(e) => {
                    tracing::warn!(error = ?e, "[MpvPlayer: Init] mpv's on-screen controller could not be switched off");
                }
            }
        }

        // mpv's warnings and errors go into the log (everything into mpv.log while it is switched on)
        messages::request(&mpv_instance);

        let mpv_arc = Arc::new(mpv_instance);
        let state = Arc::new(RwLock::new(MpvPlaybackState::default()));
        let current_file_path = Arc::new(RwLock::new(None));
        let is_view_active = Arc::new(AtomicBool::new(true));
        let is_user_paused = Arc::new(AtomicBool::new(false));
        let is_tab_grid_mode = Arc::new(AtomicBool::new(false));
        let observer_running = Arc::new(AtomicBool::new(true));
        let callbacks: Arc<RwLock<Vec<MpvEventCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let mute_callbacks: Arc<RwLock<Vec<MpvMuteCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let pause_callbacks: Arc<RwLock<Vec<MpvPauseCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let ab = Arc::new(RwLock::new(ab_repeat::AbSnapshot::default()));
        let ab_callbacks: Arc<RwLock<Vec<ab_repeat::MpvAbCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let ab_registration = ab_repeat::register(&mpv_arc, &ab, &ab_callbacks);
        ab_repeat::watch(&mpv_arc, &ab, crate::viewer::ab_repeat::prefs::current().enabled);

        let player = Self {
            mpv: mpv_arc.clone(),
            state: state.clone(),
            current_file_path: current_file_path.clone(),
            is_view_active: is_view_active.clone(),
            is_user_paused: is_user_paused.clone(),
            is_tab_grid_mode: is_tab_grid_mode.clone(),
            observer_running: observer_running.clone(),
            callbacks: callbacks.clone(),
            mute_callbacks: mute_callbacks.clone(),
            pause_callbacks: pause_callbacks.clone(),
            ab,
            ab_callbacks,
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

    #[inline]
    pub fn set_interactive(&self, interactive: bool) {
        self.set_tab_grid_mode(!interactive);
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

        self.notify_subscribers();
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
        let _ = self.mpv.command("quit", &[]);
    }
}
