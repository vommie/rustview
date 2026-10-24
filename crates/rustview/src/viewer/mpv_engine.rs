use super::error::ViewerError;
use super::types::MpvPlaybackState;
use libmpv2::Mpv;
use parking_lot::{Mutex, RwLock};
use rustview_core::util::cast::SaturatingAs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

mod events;
pub mod overlay;
mod playback;
mod rating;

pub type MpvEventCallback = Arc<dyn Fn(&MpvPlaybackState) + Send + Sync>;
pub type MpvFullscreenCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvCloseTabCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvGridClickCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvConvertToTabCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvRenameCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvDeleteCallback = Arc<dyn Fn() + Send + Sync>;
/// Called from the observer thread when a rating key (Ctrl+0..5) was pressed in the player's window.
pub type MpvRatingCallback = Arc<dyn Fn(u8) + Send + Sync>;

/// The message mpv sends to the application when DEL is pressed in its window (`script-message` reaches every client).
const DELETE_KEY_MESSAGE: &str = "rustview-delete";

fn bind_delete_key(mpv: &Mpv) {
    let _ = mpv.command("keybind", &["DEL", &format!("script-message {DELETE_KEY_MESSAGE}")]);
}

/// The message mpv sends when its window is clicked (a plain left click: the Multiview selects the tile; never a pause).
const CLICK_MESSAGE: &str = "rustview-click";

fn bind_click(mpv: &Mpv) {
    let _ = mpv.command("keybind", &["MBTN_LEFT", &format!("script-message {CLICK_MESSAGE}")]);
}

/// The message mpv sends for Esc in its window: the Multiview runs its Esc chain (crop tools, then close or nothing), the same
/// as for Esc anywhere else, instead of toggling a fullscreen.
const ESCAPE_MESSAGE: &str = "rustview-escape";

fn bind_escape(mpv: &Mpv) {
    let _ = mpv.command("keybind", &["ESC", &format!("script-message {ESCAPE_MESSAGE}")]);
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
    close_tab_callbacks: Arc<RwLock<Vec<MpvCloseTabCallback>>>,
    grid_left_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>>,
    grid_middle_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>>,
    grid_wheel_up_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>>,
    grid_wheel_down_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>>,
    convert_to_tab_callbacks: Arc<RwLock<Vec<MpvConvertToTabCallback>>>,
    rename_callbacks: Arc<RwLock<Vec<MpvRenameCallback>>>,
    delete_callbacks: Arc<RwLock<Vec<MpvDeleteCallback>>>,
    rating_callbacks: Arc<RwLock<Vec<MpvRatingCallback>>>,
    click_callbacks: Arc<RwLock<Vec<MpvClickCallback>>>,
    escape_callbacks: Arc<RwLock<Vec<MpvClickCallback>>>,
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

            // 3. Native On-Screen Controller (OSC) & OSD bar
            let _ = init.set_property("osc", "yes");
            let _ = init.set_property("osd-bar", "yes");

            // 4. Configure OSC behavior: Disable top deadzone so HUD triggers across 100% of video height
            let _ = init.set_property("script-opts", "osc-deadzonesize=0,osc-minmousemove=0");

            // 5. Native input handling
            let _ = init.set_property("input-default-bindings", "yes");
            let _ = init.set_property("input-vo-keyboard", "yes");

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
        let _ = mpv_instance.set_property("window-minimized", false);
        let _ = mpv_instance.set_property("window-maximized", false);
        let _ = mpv_instance.set_property("ontop", false);
        let _ = mpv_instance.set_property("border", true);
        let _ = mpv_instance.set_property("snap-window", false);

        // A single left click never pauses the video; it is reported, so the Multiview can select the tile
        bind_click(&mpv_instance);

        // Bind double-click, middle-click, 'f' and 'F' in MPV to cycle fullscreen; Esc is reported on its own
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["MBTN_MID", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["f", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["F", "cycle fullscreen"]);
        bind_escape(&mpv_instance);

        // Bind F2 and f2 in MPV without OSD to toggle snap-window (intercepted by RustView observer for rename)
        let _ = mpv_instance.command("keybind", &["F2", "no-osd cycle snap-window"]);
        let _ = mpv_instance.command("keybind", &["f2", "no-osd cycle snap-window"]);

        // DEL is reported to the application as a client message; the Multiview turns it into its delete flow
        bind_delete_key(&mpv_instance);
        rating::bind_rating_keys(&mpv_instance);

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

        // Bind Ctrl+W variations in MPV without OSD to toggle window-minimized
        let close_keys = [
            "ctrl+w",
            "Ctrl+w",
            "ctrl+W",
            "Ctrl+W",
            "meta+w",
            "Meta+w",
            "cmd+w",
            "Command+w",
            "ctrl+F4",
            "Ctrl+F4",
        ];
        for k in close_keys {
            let _ = mpv_instance.command("keybind", &[k, "no-osd cycle window-minimized"]);
        }

        // Ignore quit keys in MPV so Slint handles navigation and lifecycle
        let _ = mpv_instance.command("keybind", &["q", "ignore"]);
        let _ = mpv_instance.command("keybind", &["Q", "ignore"]);

        // Define the dedicated tabgrid input section for Grid View (active ONLY in Tab-Grid mode).
        // This isolates wheel and click hijacking to Tab-Grid mode without overriding or ignoring normal wheel bindings.
        let tabgrid_bindings = "MBTN_LEFT no-osd cycle ontop\nMBTN_MID no-osd cycle border\nWHEEL_UP no-osd cycle deinterlace\nWHEEL_DOWN no-osd cycle sub-visibility\nF2 no-osd cycle snap-window\nf2 no-osd cycle snap-window\n";
        if let Err(e) = mpv_instance.command("define-section", &["rustview_tabgrid", tabgrid_bindings, "force"]) {
            tracing::warn!(
                error = ?e,
                "[MpvPlayer: Init] Failed to define 'rustview_tabgrid' input section"
            );
        } else {
            tracing::debug!(
                "[MpvPlayer: Init] Defined 'rustview_tabgrid' input section for isolated grid scroll handling"
            );
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
        let close_tab_callbacks: Arc<RwLock<Vec<MpvCloseTabCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let grid_left_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let grid_middle_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let grid_wheel_up_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let grid_wheel_down_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let convert_to_tab_callbacks: Arc<RwLock<Vec<MpvConvertToTabCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let rename_callbacks: Arc<RwLock<Vec<MpvRenameCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let delete_callbacks: Arc<RwLock<Vec<MpvDeleteCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let rating_callbacks: Arc<RwLock<Vec<MpvRatingCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let click_callbacks: Arc<RwLock<Vec<MpvClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let escape_callbacks: Arc<RwLock<Vec<MpvClickCallback>>> = Arc::new(RwLock::new(Vec::new()));
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
            close_tab_callbacks: close_tab_callbacks.clone(),
            grid_left_click_callbacks: grid_left_click_callbacks.clone(),
            grid_middle_click_callbacks: grid_middle_click_callbacks.clone(),
            grid_wheel_up_callbacks: grid_wheel_up_callbacks.clone(),
            grid_wheel_down_callbacks: grid_wheel_down_callbacks.clone(),
            convert_to_tab_callbacks: convert_to_tab_callbacks.clone(),
            rename_callbacks: rename_callbacks.clone(),
            delete_callbacks: delete_callbacks.clone(),
            rating_callbacks: rating_callbacks.clone(),
            click_callbacks: click_callbacks.clone(),
            escape_callbacks: escape_callbacks.clone(),
            mute_callbacks: mute_callbacks.clone(),
            pause_callbacks: pause_callbacks.clone(),
            observer_handle: Mutex::new(None),
        };

        player.spawn_event_observer_thread();

        tracing::info!(
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Init] Native libmpv2 player engine initialized with unreserved mouse wheel pass-through and F2 rename interception"
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

    /// Toggles Tab-Grid mode on this MPV player instance.
    /// When `grid_mode` is true, enables the `rustview_tabgrid` input section to intercept scroll and clicks for grid navigation.
    /// When `grid_mode` is false (normal Tab View or Fullscreen), disables `rustview_tabgrid` so that mouse wheel (up/down)
    /// falls back to MPV's native or user-configured bindings without being swallowed or ignored.
    pub fn set_tab_grid_mode(&self, grid_mode: bool) {
        if self.is_tab_grid_mode.swap(grid_mode, Ordering::SeqCst) == grid_mode {
            return;
        }

        if grid_mode {
            tracing::info!(
                "[MpvPlayer: GridMode] Activating Tab-Grid mode (OSC off, wheel scrolls grid, clicks select/fullscreen)"
            );
            let _ = self.mpv.set_property("osc", "no");
            let _ = self.mpv.set_property("input-default-bindings", "no");
            let _ = self.mpv.set_property("input-vo-keyboard", "no");
            let _ = self.mpv.set_property("input-cursor", "yes");

            // Enable the dedicated tabgrid input section (intercepts mouse clicks and wheel for grid scrolling)
            let _ = self.mpv.command(
                "enable-section",
                &["rustview_tabgrid", "allow-hide-cursor+allow-vo-dragging"],
            );
        } else {
            tracing::info!(
                "[MpvPlayer: GridMode] Deactivating Tab-Grid mode (restoring normal/fullscreen playback, mouse wheel passed directly to MPV)"
            );
            // Disable the tabgrid section to restore underlying keybindings and unreserve mouse wheel
            let _ = self.mpv.command("disable-section", &["rustview_tabgrid"]);

            let _ = self.mpv.set_property("osc", "yes");
            let _ = self.mpv.set_property("input-default-bindings", "yes");
            let _ = self.mpv.set_property("input-vo-keyboard", "yes");
            let _ = self.mpv.set_property("input-cursor", "yes");

            // Restore standard bindings with Middle-Click Fullscreen toggle and Ctrl+DoubleClick
            bind_click(&self.mpv);
            let _ = self.mpv.command("keybind", &["MBTN_MID", "cycle fullscreen"]);
            let _ = self.mpv.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
            bind_escape(&self.mpv);

            // Re-assert F2 binding
            let _ = self.mpv.command("keybind", &["F2", "no-osd cycle snap-window"]);
            let _ = self.mpv.command("keybind", &["f2", "no-osd cycle snap-window"]);
            bind_delete_key(&self.mpv);

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
                let _ = self.mpv.command("keybind", &[k, "no-osd cycle window-maximized"]);
            }
        }
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
