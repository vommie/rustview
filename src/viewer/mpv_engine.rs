use super::error::ViewerError;
use super::types::MpvPlaybackState;
use crate::settings::LogObscurer;
use crate::settings::schema::{MPV_OBSERVER_ACTIVE_POLL_MS, MPV_OBSERVER_IDLE_POLL_MS};
use libmpv2::Mpv;
use parking_lot::{Mutex, RwLock};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub type MpvEventCallback = Arc<dyn Fn(&MpvPlaybackState) + Send + Sync>;
pub type MpvFullscreenCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvCloseTabCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvGridClickCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvConvertToTabCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvRenameCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvDeleteCallback = Arc<dyn Fn() + Send + Sync>;

/// The message mpv sends to the application when DEL is pressed in its window (`script-message` reaches every client).
const DELETE_KEY_MESSAGE: &str = "rustview-delete";

fn bind_delete_key(mpv: &Mpv) {
    let _ = mpv.command("keybind", &["DEL", &format!("script-message {DELETE_KEY_MESSAGE}")]);
}
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
        .map_err(|e| ViewerError::Mpv(format!("Failed to initialize libmpv2 context: {:?}", e)))?;

        // Ensure OSC deadzone is completely zeroed out
        let _ = mpv_instance.set_property("script-opts", "osc-deadzonesize=0,osc-minmousemove=0");

        // Ensure window states are clean
        let _ = mpv_instance.set_property("window-minimized", false);
        let _ = mpv_instance.set_property("window-maximized", false);
        let _ = mpv_instance.set_property("ontop", false);
        let _ = mpv_instance.set_property("border", true);
        let _ = mpv_instance.set_property("snap-window", false);

        // Bind single left click to ignore so click-to-focus does not accidentally pause/unpause video
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT", "ignore"]);

        // Bind double-click, middle-click, 'f', 'F', and 'ESC' in MPV to cycle fullscreen
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["MBTN_MID", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["f", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["F", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["ESC", "cycle fullscreen"]);

        // Bind F2 and f2 in MPV without OSD to toggle snap-window (intercepted by RustView observer for rename)
        let _ = mpv_instance.command("keybind", &["F2", "no-osd cycle snap-window"]);
        let _ = mpv_instance.command("keybind", &["f2", "no-osd cycle snap-window"]);

        // DEL is reported to the application as a client message; the Multiview turns it into its delete flow
        bind_delete_key(&mpv_instance);

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
        if let Err(e) = mpv_instance.command(
            "define-section",
            &["rustview_tabgrid", tabgrid_bindings, "force"],
        ) {
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
        let fullscreen_callbacks: Arc<RwLock<Vec<MpvFullscreenCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let close_tab_callbacks: Arc<RwLock<Vec<MpvCloseTabCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let grid_left_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let grid_middle_click_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let grid_wheel_up_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let grid_wheel_down_callbacks: Arc<RwLock<Vec<MpvGridClickCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let convert_to_tab_callbacks: Arc<RwLock<Vec<MpvConvertToTabCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let rename_callbacks: Arc<RwLock<Vec<MpvRenameCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
        let delete_callbacks: Arc<RwLock<Vec<MpvDeleteCallback>>> =
            Arc::new(RwLock::new(Vec::new()));
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
            .set_property("wid", wid as i64)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set MPV 'wid' property: {:?}", e)))?;

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
            let _ = self.mpv.command("keybind", &["MBTN_LEFT", "ignore"]);
            let _ = self
                .mpv
                .command("keybind", &["MBTN_MID", "cycle fullscreen"]);
            let _ = self
                .mpv
                .command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
            let _ = self.mpv.command("keybind", &["ESC", "cycle fullscreen"]);

            // Re-assert F2 binding
            let _ = self
                .mpv
                .command("keybind", &["F2", "no-osd cycle snap-window"]);
            let _ = self
                .mpv
                .command("keybind", &["f2", "no-osd cycle snap-window"]);
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
                let _ = self
                    .mpv
                    .command("keybind", &[k, "no-osd cycle window-maximized"]);
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

        if !active {
            let _ = self.mpv.set_property("pause", true);
            tracing::debug!(
                user_paused = self.is_user_paused.load(Ordering::SeqCst),
                "[MpvPlayer: AutoPause] Video tab backgrounded and auto-paused"
            );
        } else {
            let user_paused = self.is_user_paused.load(Ordering::SeqCst);
            if !user_paused {
                let _ = self.mpv.set_property("pause", false);
                tracing::debug!(
                    "[MpvPlayer: AutoResume] Foregrounded video tab auto-resumed (playback active)"
                );
            } else {
                let _ = self.mpv.set_property("pause", true);
                tracing::debug!(
                    "[MpvPlayer: StayPaused] Foregrounded video tab kept paused (user-paused)"
                );
            }
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

    /// Loads a video file from disk and starts immediate playback (resets user pause state to false).
    pub fn load_file(&self, path: &Path) -> Result<(), ViewerError> {
        if !path.exists() {
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        let t0 = Instant::now();
        let path_str = path.to_string_lossy();

        self.mpv
            .command("loadfile", &[&path_str, "replace"])
            .map_err(|e| {
                ViewerError::Mpv(format!("Failed to execute 'loadfile' command: {:?}", e))
            })?;

        self.is_user_paused.store(false, Ordering::SeqCst);
        let _ = self.mpv.set_property("pause", false);

        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.to_string());

        *self.current_file_path.write() = Some(path.to_path_buf());

        {
            let mut st = self.state.write();
            st.filename = filename;
            st.is_playing = true;
            st.is_paused = false;
            st.is_user_paused = false;
            st.is_eof = false;
            st.is_idle = false;
            st.position_secs = 0.0;
        }

        tracing::info!(
            file = %LogObscurer::path(path),
            elapsed_ms = t0.elapsed().as_millis(),
            "[MpvPlayer: LoadFile] Loaded and started video playback"
        );

        self.notify_subscribers();
        Ok(())
    }

    /// Reloads the currently loaded video file from disk (used by F5/Refresh).
    pub fn reload_current_file(&self) -> Result<(), ViewerError> {
        let current_path = self.current_file_path.read().clone();
        if let Some(ref path) = current_path {
            tracing::info!(
                file = %LogObscurer::path(path),
                "[MpvPlayer: Reload] Reloading current video file"
            );
            self.load_file(path)
        } else {
            Ok(())
        }
    }

    pub fn play(&self) -> Result<(), ViewerError> {
        self.is_user_paused.store(false, Ordering::SeqCst);
        self.mpv
            .set_property("pause", false)
            .map_err(|e| ViewerError::Mpv(format!("Failed to unpause MPV: {:?}", e)))?;

        {
            let mut st = self.state.write();
            st.is_paused = false;
            st.is_user_paused = false;
            st.is_playing = true;
        }
        self.notify_subscribers();
        Ok(())
    }

    pub fn pause(&self) -> Result<(), ViewerError> {
        if self.is_view_active.load(Ordering::SeqCst)
            && !self.is_tab_grid_mode.load(Ordering::SeqCst)
        {
            self.is_user_paused.store(true, Ordering::SeqCst);
        }
        self.mpv
            .set_property("pause", true)
            .map_err(|e| ViewerError::Mpv(format!("Failed to pause MPV: {:?}", e)))?;

        {
            let mut st = self.state.write();
            st.is_paused = true;
            st.is_user_paused = self.is_user_paused.load(Ordering::SeqCst);
        }
        self.notify_subscribers();
        Ok(())
    }

    pub fn toggle_pause(&self) -> Result<(), ViewerError> {
        let is_paused = self.is_paused();
        if is_paused { self.play() } else { self.pause() }
    }

    pub fn seek_relative(&self, seconds: f64) -> Result<(), ViewerError> {
        let sec_str = seconds.to_string();
        self.mpv
            .command("seek", &[&sec_str, "relative"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek relative: {:?}", e)))?;
        Ok(())
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), ViewerError> {
        let sec_str = seconds.to_string();
        self.mpv
            .command("seek", &[&sec_str, "absolute"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek absolute: {:?}", e)))?;
        Ok(())
    }

    pub fn set_volume(&self, volume_percent: f64) -> Result<(), ViewerError> {
        let clamped = volume_percent.clamp(0.0, 150.0);
        self.mpv
            .set_property("volume", clamped)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set volume: {:?}", e)))?;

        self.state.write().volume = clamped;
        self.notify_subscribers();
        Ok(())
    }

    pub fn get_volume(&self) -> f64 {
        self.mpv.get_property::<f64>("volume").unwrap_or(100.0)
    }

    /// Stops the observer thread and destroys mpv synchronously. When this returns, libmpv and its video output are gone, so
    /// the X11 window it rendered into can be destroyed safely. Dropping the player instead only sends `quit`; the actual
    /// shutdown then races with the destruction of the window. Must be called with the last reference to the player.
    pub fn shutdown_blocking(self: Arc<Self>) {
        self.observer_running.store(false, Ordering::Release);
        let _ = self.mpv.command("stop", &[]);
        if let Some(observer) = self.observer_handle.lock().take() {
            let _ = observer.join();
        }
        drop(self);
    }

    /// The color mpv paints around a video that does not fill its window (letterbox bars). mpv 0.37 and newer call the
    /// option `background-color`; older versions take the color as `background`.
    pub fn set_background_color(&self, rgb: [u8; 3]) {
        let color = crate::util::hex_color::format_rgb(rgb);
        if self.mpv.set_property("background-color", color.as_str()).is_err()
            && self.mpv.set_property("background", color.as_str()).is_err()
        {
            tracing::debug!("[MpvPlayer: Background] mpv did not accept a background color");
        }
    }

    /// Current mute state of this player (`false` if mpv cannot answer).
    pub fn get_mute(&self) -> bool {
        self.mpv.get_property::<bool>("mute").unwrap_or(false)
    }

    pub fn set_mute(&self, muted: bool) -> Result<(), ViewerError> {
        self.mpv
            .set_property("mute", muted)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set mute: {:?}", e)))
    }

    /// Display size and rotation of the playing video as reported by mpv (`video-params/dw`, `dh`, `rotate`).
    /// `dw`/`dh` are not rotated; `None` until the first frame of the file is decoded.
    pub fn video_display_params(&self) -> Option<(i64, i64, i64)> {
        let width = self.mpv.get_property::<i64>("video-params/dw").ok()?;
        let height = self.mpv.get_property::<i64>("video-params/dh").ok()?;
        let rotate = self
            .mpv
            .get_property::<i64>("video-params/rotate")
            .unwrap_or(0);
        Some((width, height, rotate))
    }

    pub fn get_position(&self) -> f64 {
        self.mpv.get_property::<f64>("time-pos").unwrap_or(0.0)
    }

    pub fn get_duration(&self) -> f64 {
        self.mpv.get_property::<f64>("duration").unwrap_or(0.0)
    }

    pub fn is_paused(&self) -> bool {
        self.mpv.get_property::<bool>("pause").unwrap_or(false)
    }

    pub fn is_eof(&self) -> bool {
        self.mpv
            .get_property::<bool>("eof-reached")
            .unwrap_or(false)
    }

    pub fn stop(&self) {
        let _ = self.mpv.command("stop", &[]);
        *self.current_file_path.write() = None;
        {
            let mut st = self.state.write();
            st.is_playing = false;
            st.is_paused = false;
            st.is_user_paused = false;
            st.is_idle = true;
            st.position_secs = 0.0;
            st.filename.clear();
        }
        self.notify_subscribers();
        tracing::debug!("[MpvPlayer: Stop] Playback stopped and reset to idle");
    }

    pub fn get_playback_state(&self) -> MpvPlaybackState {
        self.state.read().clone()
    }

    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(&MpvPlaybackState) + Send + Sync + 'static,
    {
        self.callbacks.write().push(Arc::new(callback));
    }

    pub fn on_toggle_fullscreen<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.fullscreen_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_close_tab<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.close_tab_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_rename_requested<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.rename_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_delete_requested<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.delete_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_grid_left_click<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_left_click_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_grid_middle_click<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_middle_click_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_grid_wheel_up<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_wheel_up_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_grid_wheel_down<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_wheel_down_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Reports every mute change, whatever caused it (the single place that watches mpv for it).
    pub fn on_mute_changed<F>(&self, callback: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        let mut cbs = self.mute_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Reports every change of the user's pause state, whatever caused it (a hotkey, the on-screen controller, `pause()`).
    pub fn on_user_pause_changed<F>(&self, callback: F)
    where
        F: Fn(bool) + Send + Sync + 'static,
    {
        let mut cbs = self.pause_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    pub fn on_convert_to_background_tab<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.convert_to_tab_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    fn notify_subscribers(&self) {
        let snapshot = self.state.read().clone();
        let callbacks = self.callbacks.read().clone();
        for cb in callbacks {
            cb(&snapshot);
        }
    }

    /// Background polling and event observer loop running at high frequency without busy waiting.
    fn spawn_event_observer_thread(&self) {
        let mpv = self.mpv.clone();
        let state = self.state.clone();
        let running = self.observer_running.clone();
        let is_view_active = self.is_view_active.clone();
        let is_user_paused = self.is_user_paused.clone();
        let is_tab_grid_mode = self.is_tab_grid_mode.clone();
        let callbacks = self.callbacks.clone();
        let fs_callbacks = self.fullscreen_callbacks.clone();
        let close_callbacks = self.close_tab_callbacks.clone();
        let grid_left_callbacks = self.grid_left_click_callbacks.clone();
        let grid_middle_callbacks = self.grid_middle_click_callbacks.clone();
        let grid_wheel_up_callbacks = self.grid_wheel_up_callbacks.clone();
        let grid_wheel_down_callbacks = self.grid_wheel_down_callbacks.clone();
        let convert_callbacks = self.convert_to_tab_callbacks.clone();
        let rename_callbacks = self.rename_callbacks.clone();
        let delete_callbacks = self.delete_callbacks.clone();
        let mute_callbacks = self.mute_callbacks.clone();
        let pause_callbacks = self.pause_callbacks.clone();

        let observer = std::thread::Builder::new()
            .name("mpv-event-observer".into())
            .spawn(move || {
                tracing::debug!("[MpvPlayer: Observer] Event observer thread started");

                let mut last_deinterlace = false;
                let mut last_sub_vis = true;
                let mut last_mute: Option<bool> = None;
                let mut last_user_paused: Option<bool> = None;

                while running.load(Ordering::Relaxed) {
                    // A player that is neither shown nor in the tab grid has nobody to react to: poll it less often.
                    let hidden = !is_view_active.load(Ordering::Relaxed)
                        && !is_tab_grid_mode.load(Ordering::Relaxed);
                    let poll_ms = if hidden {
                        MPV_OBSERVER_IDLE_POLL_MS
                    } else {
                        MPV_OBSERVER_ACTIVE_POLL_MS
                    };
                    if let Some(Ok(libmpv2::events::Event::ClientMessage(args))) = mpv.wait_event(poll_ms as f64 / 1000.0)
                        && args.first() == Some(&DELETE_KEY_MESSAGE)
                    {
                        let delete_cbs = delete_callbacks.read().clone();
                        for cb in delete_cbs {
                            cb();
                        }
                    }

                    let in_grid = is_tab_grid_mode.load(Ordering::Relaxed);

                    // 1. Intercept TabGrid Mouse Clicks and Wheel Scrolling
                    if in_grid {
                        let ontop_val = mpv.get_property::<bool>("ontop").unwrap_or(false);
                        if ontop_val {
                            let _ = mpv.set_property("ontop", false);
                            let cbs = grid_left_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }

                        let border_val = mpv.get_property::<bool>("border").unwrap_or(true);
                        if !border_val {
                            let _ = mpv.set_property("border", true);
                            let cbs = grid_middle_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }

                        // Wheel Up in TabGrid Mode (scrolls grid up)
                        let deinterlace_val =
                            mpv.get_property::<bool>("deinterlace").unwrap_or(false);
                        if deinterlace_val != last_deinterlace {
                            last_deinterlace = deinterlace_val;
                            let cbs = grid_wheel_up_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }

                        // Wheel Down in TabGrid Mode (scrolls grid down)
                        let sub_vis_val =
                            mpv.get_property::<bool>("sub-visibility").unwrap_or(true);
                        if sub_vis_val != last_sub_vis {
                            last_sub_vis = sub_vis_val;
                            let cbs = grid_wheel_down_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }
                    }

                    // 2. Intercept F2 Rename Shortcut from MPV via snap-window property toggle
                    let rename_requested = mpv.get_property::<bool>("snap-window").unwrap_or(false);
                    if rename_requested {
                        let _ = mpv.set_property("snap-window", false);
                        let ren_cbs = rename_callbacks.read().clone();
                        for cb in ren_cbs {
                            cb();
                        }
                    }

                    // 3. Intercept double-click / middle-click / 'f' / 'ESC' Fullscreen triggers from MPV
                    let fs_requested = mpv.get_property::<bool>("fullscreen").unwrap_or(false);
                    if fs_requested {
                        let _ = mpv.set_property("fullscreen", false);
                        let fs_cbs = fs_callbacks.read().clone();
                        for cb in fs_cbs {
                            cb();
                        }
                    }

                    // 4. Intercept Ctrl+W Close Tab triggers from MPV via window-minimized toggle
                    let close_requested = mpv
                        .get_property::<bool>("window-minimized")
                        .unwrap_or(false);
                    if close_requested {
                        let _ = mpv.set_property("window-minimized", false);
                        let close_cbs = close_callbacks.read().clone();
                        for cb in close_cbs {
                            cb();
                        }
                    }

                    // 5. Intercept Ctrl+Click / Ctrl+DoubleClick in Fullscreen to convert the fullscreen Multiview to a background tab
                    let convert_requested = mpv
                        .get_property::<bool>("window-maximized")
                        .unwrap_or(false);
                    if convert_requested {
                        let _ = mpv.set_property("window-maximized", false);
                        let convert_cbs = convert_callbacks.read().clone();
                        for cb in convert_cbs {
                            cb();
                        }
                    }

                    // 6. Sync live playback properties
                    let pos = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
                    let dur = mpv.get_property::<f64>("duration").unwrap_or(0.0);
                    // The pause state counts as the user's choice only if the view stayed active while it was read.
                    // `set_view_active` stores the flag first and pauses afterwards, so a pause it caused is always
                    // seen with the flag already cleared on one of the two samples.
                    let active_before = is_view_active.load(Ordering::SeqCst)
                        && !is_tab_grid_mode.load(Ordering::SeqCst);
                    let paused = mpv.get_property::<bool>("pause").unwrap_or(false);
                    let active_after = is_view_active.load(Ordering::SeqCst)
                        && !is_tab_grid_mode.load(Ordering::SeqCst);
                    let vol = mpv.get_property::<f64>("volume").unwrap_or(100.0);
                    let eof = mpv.get_property::<bool>("eof-reached").unwrap_or(false);
                    let idle = mpv.get_property::<bool>("idle-active").unwrap_or(true);

                    if active_before && active_after {
                        is_user_paused.store(paused, Ordering::Relaxed);
                    }

                    let user_paused_val = is_user_paused.load(Ordering::Relaxed);

                    // Mute and pause changes made inside mpv (hotkeys, OSC) are reported to whoever listens.
                    let muted = mpv.get_property::<bool>("mute").unwrap_or(false);
                    if last_mute.is_some_and(|before| before != muted) {
                        for cb in mute_callbacks.read().clone() {
                            cb(muted);
                        }
                    }
                    last_mute = Some(muted);
                    if last_user_paused.is_some_and(|before| before != user_paused_val) {
                        for cb in pause_callbacks.read().clone() {
                            cb(user_paused_val);
                        }
                    }
                    last_user_paused = Some(user_paused_val);

                    let has_changed = {
                        let mut st = state.write();
                        let changed = (st.position_secs - pos).abs() > 0.1
                            || (st.duration_secs - dur).abs() > 0.1
                            || st.is_paused != paused
                            || st.is_user_paused != user_paused_val
                            || (st.volume - vol).abs() > 0.5
                            || st.is_eof != eof
                            || st.is_idle != idle;

                        if changed {
                            st.position_secs = pos;
                            st.duration_secs = dur;
                            st.is_paused = paused;
                            st.is_user_paused = user_paused_val;
                            st.is_playing = !idle && !paused;
                            st.volume = vol;
                            st.is_eof = eof;
                            st.is_idle = idle;
                        }
                        changed
                    };

                    if has_changed {
                        let snapshot = state.read().clone();
                        let cb_list = callbacks.read().clone();
                        for cb in cb_list {
                            cb(&snapshot);
                        }
                    }
                }

                tracing::debug!("[MpvPlayer: Observer] Event observer thread terminated");
            })
            .expect("Failed to spawn MPV event observer thread");
        *self.observer_handle.lock() = Some(observer);
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.observer_running.store(false, Ordering::Release);
        let _ = self.mpv.command("quit", &[]);
    }
}
