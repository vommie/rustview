use super::error::ViewerError;
use super::types::MpvPlaybackState;
use libmpv2::Mpv;
use parking_lot::{Mutex, RwLock};
use rustview_core::util::cast::SaturatingAs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod controls;
mod events;
mod osd_text;
pub mod overlay;
mod playback;
pub use controls::SeekPrecision;
pub use osd_text::OsdText;
pub use playback::PlaybackNumbers;
mod rating;
mod zoom;

pub type MpvEventCallback = Arc<dyn Fn(&MpvPlaybackState) + Send + Sync>;
pub type MpvFullscreenCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvConvertToTabCallback = Arc<dyn Fn() + Send + Sync>;

/// The message mpv sends when its window is clicked (a plain left click: the Multiview selects the tile; never a pause).
const CLICK_MESSAGE: &str = "rustview-click";

fn bind_click(mpv: &Mpv) {
    let _ = mpv.command("keybind", &["MBTN_LEFT", &format!("script-message {CLICK_MESSAGE}")]);
}

/// Called from the observer thread when the player's window was clicked.
pub type MpvClickCallback = Arc<dyn Fn() + Send + Sync>;
/// Called from the observer thread when the mute state changed (a hotkey, the on-screen controller or `set_mute`).
pub type MpvMuteCallback = Arc<dyn Fn(bool) + Send + Sync>;
/// Called from the observer thread when the user's pause state changed (`true` = paused). A pause that only comes from
/// the view being hidden is not the user's and is not reported.
pub type MpvPauseCallback = Arc<dyn Fn(bool) + Send + Sync>;

/// Central thread-safe MPV player engine wrapping `libmpv2` with native OSC, plugins, custom config files, and property observation.
pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    state: Arc<RwLock<MpvPlaybackState>>,
    current_file_path: Arc<RwLock<Option<PathBuf>>>,
    attached_wid: Arc<RwLock<Option<u64>>>,
    is_view_active: Arc<AtomicBool>,
    is_user_paused: Arc<AtomicBool>,
    is_tab_grid_mode: Arc<AtomicBool>,
    observer_running: Arc<AtomicBool>,
    callbacks: Arc<RwLock<Vec<MpvEventCallback>>>,
    fullscreen_callbacks: Arc<RwLock<Vec<MpvFullscreenCallback>>>,
    convert_to_tab_callbacks: Arc<RwLock<Vec<MpvConvertToTabCallback>>>,
    click_callbacks: Arc<RwLock<Vec<MpvClickCallback>>>,
    mute_callbacks: Arc<RwLock<Vec<MpvMuteCallback>>>,
    pause_callbacks: Arc<RwLock<Vec<MpvPauseCallback>>>,
    observer_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MpvPlayer {
    /// Initializes a high-performance MPV player instance configured for native X11 embedding with full user config, plugins, and double-click/middle-click trapping.
    pub fn new() -> Result<Self, ViewerError> {
        let t0 = Instant::now();

        let mpv_instance = Mpv::with_initializer(|init| {
            // 1. Hardware acceleration & rendering
            let _ = init.set_property("vo", "null");
            let _ = init.set_property("force-window", "no");
            let _ = init.set_property("hwdec", "auto-safe");
            // Without it `vo=gpu` refuses a software GL renderer (llvmpipe, e.g. in a VM) and the video stays black; on a real GPU
            // the flag changes nothing.
            let _ = init.set_property("gpu-sw", "yes");

            // 2. Audio & playback lifecycle
            let _ = init.set_property("keep-open", "yes");
            let _ = init.set_property("idle", "yes");

            // 3. No on-screen controller: RustView takes the pointer over videos and draws a control bar of its own
            // (`viewer::multiview::video_bar`), so mpv's would never see the mouse. The OSD bar stays.
            let _ = init.set_property("osc", "no");
            let _ = init.set_property("osd-bar", "yes");

            // 4. Configure OSC behavior: Disable top deadzone so HUD triggers across 100% of video height
            let _ = init.set_property("script-opts", "osc-deadzonesize=0,osc-minmousemove=0");

            // 5. Native input handling
            let _ = init.set_property("input-default-bindings", "yes");
            // mpv never takes the keyboard: a key pressed over a video reaches the RustView window under it, which handles every
            // key itself (`docs/hotkeys.md`).
            let _ = init.set_property("input-vo-keyboard", "no");

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
        let _ = mpv_instance.set_property("script-opts", "osc-deadzonesize=0,osc-minmousemove=0");

        // Ensure window states are clean
        let _ = mpv_instance.set_property("window-maximized", false);
        let _ = mpv_instance.set_property("ontop", false);
        let _ = mpv_instance.set_property("border", true);

        // A single left click never pauses the video; it is reported, so the Multiview can select the tile
        bind_click(&mpv_instance);

        // Bind double-click and middle-click in MPV to cycle fullscreen (mpv takes no keys at all, see above)
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["MBTN_MID", "cycle fullscreen"]);

        // Bind Ctrl+Click and Ctrl+DoubleClick in MPV to toggle window-maximized
        let convert_keys = [
            "Ctrl+MBTN_LEFT",
            "ctrl+MBTN_LEFT",
            "Ctrl+mbtn_left",
            "ctrl+mbtn_left",
            "Ctrl+MBTN_LEFT_DBL",
            "ctrl+MBTN_LEFT_DBL",
            "Ctrl+mbtn_left_dbl",
            "ctrl+mbtn_left_dbl",
            "Ctrl+MBTN_LEFT_HOLD",
            "ctrl+MBTN_LEFT_HOLD",
        ];
        for k in convert_keys {
            let _ = mpv_instance.command("keybind", &[k, "no-osd cycle window-maximized"]);
        }

        let mpv_arc = Arc::new(mpv_instance);
        let state = Arc::new(RwLock::new(MpvPlaybackState::default()));
        let current_file_path = Arc::new(RwLock::new(None));
        let attached_wid = Arc::new(RwLock::new(None));
        let is_view_active = Arc::new(AtomicBool::new(true));
        let is_user_paused = Arc::new(AtomicBool::new(false));
        let is_tab_grid_mode = Arc::new(AtomicBool::new(false));
        let observer_running = Arc::new(AtomicBool::new(true));
        let callbacks: Arc<RwLock<Vec<MpvEventCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let fullscreen_callbacks: Arc<RwLock<Vec<MpvFullscreenCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let convert_to_tab_callbacks: Arc<RwLock<Vec<MpvConvertToTabCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let click_callbacks: Arc<RwLock<Vec<MpvClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let mute_callbacks: Arc<RwLock<Vec<MpvMuteCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let pause_callbacks: Arc<RwLock<Vec<MpvPauseCallback>>> = Arc::new(RwLock::new(Vec::new()));

        let player = Self {
            mpv: mpv_arc.clone(),
            state: state.clone(),
            current_file_path: current_file_path.clone(),
            attached_wid: attached_wid.clone(),
            is_view_active: is_view_active.clone(),
            is_user_paused: is_user_paused.clone(),
            is_tab_grid_mode: is_tab_grid_mode.clone(),
            observer_running: observer_running.clone(),
            callbacks: callbacks.clone(),
            fullscreen_callbacks: fullscreen_callbacks.clone(),
            convert_to_tab_callbacks: convert_to_tab_callbacks.clone(),
            click_callbacks: click_callbacks.clone(),
            mute_callbacks: mute_callbacks.clone(),
            pause_callbacks: pause_callbacks.clone(),
            observer_handle: Mutex::new(None),
        };

        player.spawn_event_observer_thread();

        tracing::info!(
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Init] Native libmpv2 player engine initialized with unreserved mouse wheel pass-through and the keys of the keymap"
        );

        Ok(player)
    }

    /// Attaches the MPV video render output to the specified X11 window ID (`wid`) and activates GPU video output.
    pub fn attach_to_window(&self, wid: u64) -> Result<(), ViewerError> {
        let t0 = Instant::now();
        self.mpv
            .set_property("wid", wid.saturating_as::<i64>())
            .map_err(|e| ViewerError::Mpv(format!("Failed to set MPV 'wid' property: {e:?}")))?;

        if let Err(e) = self.mpv.set_property("vo", "gpu") {
            tracing::warn!(wid, error = ?e, "[MpvPlayer: Attach] Failed to switch the video output to 'gpu'");
        }
        *self.attached_wid.write() = Some(wid);

        tracing::info!(
            wid,
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Attach] Attached MPV video output to X11 subwindow and enabled GPU rendering"
        );

        Ok(())
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
        self.observer_running.store(false, Ordering::Release);
        let _ = self.mpv.command("quit", &[]);
    }
}
