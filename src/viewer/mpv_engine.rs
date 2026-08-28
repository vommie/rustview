use super::error::ViewerError;
use super::types::MpvPlaybackState;
use crate::settings::LogObscurer;
use libmpv2::Mpv;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

pub type MpvEventCallback = Arc<dyn Fn(&MpvPlaybackState) + Send + Sync>;
pub type MpvFullscreenCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvCloseTabCallback = Arc<dyn Fn() + Send + Sync>;
pub type MpvGridClickCallback = Arc<dyn Fn() + Send + Sync>;

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
}

impl MpvPlayer {
    /// Initializes a high-performance MPV player instance configured for native X11 embedding with full user config, plugins, and double-click/middle-click trapping.
    pub fn new() -> Result<Self, ViewerError> {
        let t0 = Instant::now();

        let mpv_instance = Mpv::with_initializer(|init| {
            // 1. Hardware acceleration & rendering
            let _ = init.set_property("vo", "gpu");
            let _ = init.set_property("hwdec", "auto-safe");

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
            let config_dir = std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| {
                    std::env::var("HOME")
                        .map(|h| PathBuf::from(h).join(".config"))
                        .unwrap_or_else(|_| PathBuf::from("."))
                })
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

        // Ensure window-minimized state is clean
        let _ = mpv_instance.set_property("window-minimized", false);
        let _ = mpv_instance.set_property("ontop", false);
        let _ = mpv_instance.set_property("border", true);

        // Bind double-click, middle-click, 'f', 'F', and 'ESC' in MPV to cycle fullscreen (intercepted by RustView observer)
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["MBTN_MID", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["f", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["F", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["ESC", "cycle fullscreen"]);

        // Bind Ctrl+W variations in MPV without OSD to toggle window-minimized (intercepted by RustView observer)
        let close_keys = [
            "ctrl+w", "Ctrl+w", "ctrl+W", "Ctrl+W",
            "meta+w", "Meta+w", "cmd+w", "Command+w",
            "ctrl+F4", "Ctrl+F4",
        ];
        for k in close_keys {
            let _ = mpv_instance.command("keybind", &[k, "no-osd cycle window-minimized"]);
        }

        // Ignore quit keys in MPV so Slint handles navigation and lifecycle
        let _ = mpv_instance.command("keybind", &["q", "ignore"]);
        let _ = mpv_instance.command("keybind", &["Q", "ignore"]);

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
        };

        player.spawn_event_observer_thread();

        tracing::info!(
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Init] Native libmpv2 player engine initialized with full config, plugins and auto/user pause tracking"
        );

        Ok(player)
    }

    /// Attaches the MPV video render output to the specified X11 window ID (`wid`).
    pub fn attach_to_window(&self, wid: u64) -> Result<(), ViewerError> {
        let t0 = Instant::now();
        self.mpv
            .set_property("wid", wid as i64)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set MPV 'wid' property: {:?}", e)))?;

        *self.attached_wid.write() = Some(wid);

        tracing::info!(
            wid,
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Attach] Attached MPV video output to X11 subwindow"
        );

        Ok(())
    }

    /// Toggles Tab-Grid mode on this MPV player instance.
    /// When `grid_mode == true`: OSC HUD is disabled, video controls are disabled, and mouse buttons/wheel are bound to RustView grid navigation.
    /// When `grid_mode == false`: Native MPV player controls and HUD are restored with middle-click fullscreen toggle.
    pub fn set_tab_grid_mode(&self, grid_mode: bool) {
        if self.is_tab_grid_mode.swap(grid_mode, Ordering::SeqCst) == grid_mode {
            return;
        }

        if grid_mode {
            let _ = self.mpv.set_property("osc", "no");
            let _ = self.mpv.set_property("input-default-bindings", "no");
            let _ = self.mpv.set_property("input-vo-keyboard", "no");
            let _ = self.mpv.set_property("input-cursor", "yes");

            // Intercept mouse clicks on video in TabGrid
            let _ = self.mpv.command("keybind", &["MBTN_LEFT", "no-osd cycle ontop"]);
            let _ = self.mpv.command("keybind", &["MBTN_MID", "no-osd cycle border"]);

            // Intercept mouse wheel on video in TabGrid to scroll the TabGrid smoothly in real time
            let _ = self.mpv.command("keybind", &["WHEEL_UP", "no-osd cycle deinterlace"]);
            let _ = self.mpv.command("keybind", &["WHEEL_DOWN", "no-osd cycle sub-visibility"]);
        } else {
            let _ = self.mpv.set_property("osc", "yes");
            let _ = self.mpv.set_property("input-default-bindings", "yes");
            let _ = self.mpv.set_property("input-vo-keyboard", "yes");
            let _ = self.mpv.set_property("input-cursor", "yes");

            // Restore standard bindings with Middle-Click Fullscreen toggle
            let _ = self.mpv.command("keybind", &["MBTN_LEFT", "ignore"]);
            let _ = self.mpv.command("keybind", &["MBTN_MID", "cycle fullscreen"]);
            let _ = self.mpv.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
            let _ = self.mpv.command("keybind", &["ESC", "cycle fullscreen"]);
            let _ = self.mpv.command("keybind", &["WHEEL_UP", "ignore"]);
            let _ = self.mpv.command("keybind", &["WHEEL_DOWN", "ignore"]);
        }
    }

    /// Helper alias to enable or disable interactivity.
    #[inline]
    pub fn set_interactive(&self, interactive: bool) {
        self.set_tab_grid_mode(!interactive);
    }

    /// Controls active visibility of this video tab. Automatically manages auto-pause and auto-resume without overwriting user pause state.
    pub fn set_view_active(&self, active: bool) {
        self.is_view_active.store(active, Ordering::SeqCst);

        if !active {
            // Auto-pause when backgrounded: do not change is_user_paused flag
            let _ = self.mpv.set_property("pause", true);
            tracing::debug!(
                user_paused = self.is_user_paused.load(Ordering::SeqCst),
                "[MpvPlayer: AutoPause] Video tab backgrounded and auto-paused"
            );
        } else {
            let user_paused = self.is_user_paused.load(Ordering::SeqCst);
            if !user_paused {
                let _ = self.mpv.set_property("pause", false);
                tracing::debug!("[MpvPlayer: AutoResume] Foregrounded video tab auto-resumed (playback active)");
            } else {
                let _ = self.mpv.set_property("pause", true);
                tracing::debug!("[MpvPlayer: StayPaused] Foregrounded video tab kept paused (user-paused)");
            }
        }

        self.notify_subscribers();
    }

    /// Explicitly updates the user pause intention flag.
    #[inline]
    pub fn set_user_paused(&self, user_paused: bool) {
        self.is_user_paused.store(user_paused, Ordering::SeqCst);
        self.state.write().is_user_paused = user_paused;
    }

    /// Returns whether this video tab is currently active and in the foreground.
    #[inline]
    pub fn is_view_active(&self) -> bool {
        self.is_view_active.load(Ordering::SeqCst)
    }

    /// Returns whether this video was manually paused by the user.
    #[inline]
    pub fn is_user_paused(&self) -> bool {
        self.is_user_paused.load(Ordering::SeqCst)
    }

    /// Loads a video file from disk and starts immediate playback (Requirement 1: resets user pause state to false).
    pub fn load_file(&self, path: &Path) -> Result<(), ViewerError> {
        if !path.exists() {
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        let t0 = Instant::now();
        let path_str = path.to_string_lossy();

        self.mpv
            .command("loadfile", &[&path_str, "replace"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to execute 'loadfile' command: {:?}", e)))?;

        // Requirement 1: New file load starts playing and resets user-pause
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

    /// Resumes playback explicitly (marks user pause as false).
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

    /// Pauses playback explicitly (marks user pause as true if active in viewer).
    pub fn pause(&self) -> Result<(), ViewerError> {
        if self.is_view_active.load(Ordering::SeqCst) && !self.is_tab_grid_mode.load(Ordering::SeqCst) {
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

    /// Toggles play / pause state.
    pub fn toggle_pause(&self) -> Result<(), ViewerError> {
        let is_paused = self.is_paused();
        if is_paused {
            self.play()
        } else {
            self.pause()
        }
    }

    /// Seeks relative to current position by `seconds` (positive = forward, negative = backward).
    pub fn seek_relative(&self, seconds: f64) -> Result<(), ViewerError> {
        let sec_str = seconds.to_string();
        self.mpv
            .command("seek", &[&sec_str, "relative"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek relative: {:?}", e)))?;
        Ok(())
    }

    /// Seeks to an absolute timestamp position in seconds.
    pub fn seek_absolute(&self, seconds: f64) -> Result<(), ViewerError> {
        let sec_str = seconds.to_string();
        self.mpv
            .command("seek", &[&sec_str, "absolute"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek absolute: {:?}", e)))?;
        Ok(())
    }

    /// Sets playback volume in percent (0.0 .. 150.0).
    pub fn set_volume(&self, volume_percent: f64) -> Result<(), ViewerError> {
        let clamped = volume_percent.clamp(0.0, 150.0);
        self.mpv
            .set_property("volume", clamped)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set volume: {:?}", e)))?;

        self.state.write().volume = clamped;
        self.notify_subscribers();
        Ok(())
    }

    /// Gets current volume level in percent.
    pub fn get_volume(&self) -> f64 {
        self.mpv.get_property::<f64>("volume").unwrap_or(100.0)
    }

    /// Returns the current playback timestamp position in seconds.
    pub fn get_position(&self) -> f64 {
        self.mpv.get_property::<f64>("time-pos").unwrap_or(0.0)
    }

    /// Returns the total video duration in seconds.
    pub fn get_duration(&self) -> f64 {
        self.mpv.get_property::<f64>("duration").unwrap_or(0.0)
    }

    /// Returns whether the player is currently paused.
    pub fn is_paused(&self) -> bool {
        self.mpv.get_property::<bool>("pause").unwrap_or(false)
    }

    /// Returns whether the current video reached end of file.
    pub fn is_eof(&self) -> bool {
        self.mpv
            .get_property::<bool>("eof-reached")
            .unwrap_or(false)
    }

    /// Stops playback and unloads current file.
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

    /// Returns a cloned snapshot of the current live playback metrics.
    pub fn get_playback_state(&self) -> MpvPlaybackState {
        self.state.read().clone()
    }

    /// Registers a listener callback triggered on playback state updates.
    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(&MpvPlaybackState) + Send + Sync + 'static,
    {
        self.callbacks.write().push(Arc::new(callback));
    }

    /// Registers the listener callback triggered when double-click, middle-click, 'f', or 'ESC' requests fullscreen toggle in MPV.
    pub fn on_toggle_fullscreen<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.fullscreen_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Registers the listener callback triggered when Ctrl+W requests closing active tab from MPV.
    pub fn on_close_tab<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.close_tab_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Registers the callback for left-clicking a video tile in TabGrid mode.
    pub fn on_grid_left_click<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_left_click_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Registers the callback for middle-clicking a video tile in TabGrid mode.
    pub fn on_grid_middle_click<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_middle_click_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Registers the callback for mouse wheel scrolling up over an active video tile in TabGrid mode.
    pub fn on_grid_wheel_up<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_wheel_up_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// Registers the callback for mouse wheel scrolling down over an active video tile in TabGrid mode.
    pub fn on_grid_wheel_down<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.grid_wheel_down_callbacks.write();
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

        std::thread::Builder::new()
            .name("mpv-event-observer".into())
            .spawn(move || {
                tracing::debug!("[MpvPlayer: Observer] Event observer thread started");

                let mut last_deinterlace = false;
                let mut last_sub_vis = true;

                while running.load(Ordering::Relaxed) {
                    // Poll MPV event queue with a responsive 20ms timeout
                    let _ = mpv.wait_event(0.02);

                    let in_grid = is_tab_grid_mode.load(Ordering::Relaxed);
                    let view_active = is_view_active.load(Ordering::Relaxed);

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

                        // Wheel Up in TabGrid Mode
                        let deinterlace_val = mpv.get_property::<bool>("deinterlace").unwrap_or(false);
                        if deinterlace_val != last_deinterlace {
                            last_deinterlace = deinterlace_val;
                            let cbs = grid_wheel_up_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }

                        // Wheel Down in TabGrid Mode
                        let sub_vis_val = mpv.get_property::<bool>("sub-visibility").unwrap_or(true);
                        if sub_vis_val != last_sub_vis {
                            last_sub_vis = sub_vis_val;
                            let cbs = grid_wheel_down_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }
                    }

                    // 2. Intercept double-click / middle-click / 'f' / 'ESC' Fullscreen triggers from MPV (when not in grid mode)
                    let fs_requested = mpv.get_property::<bool>("fullscreen").unwrap_or(false);
                    if fs_requested {
                        let _ = mpv.set_property("fullscreen", false);
                        let fs_cbs = fs_callbacks.read().clone();
                        for cb in fs_cbs {
                            cb();
                        }
                    }

                    // 3. Intercept Ctrl+W Close Tab triggers from MPV via window-minimized toggle
                    let close_requested = mpv.get_property::<bool>("window-minimized").unwrap_or(false);
                    if close_requested {
                        let _ = mpv.set_property("window-minimized", false);
                        let close_cbs = close_callbacks.read().clone();
                        for cb in close_cbs {
                            cb();
                        }
                    }

                    // 4. Sync live playback properties
                    let pos = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
                    let dur = mpv.get_property::<f64>("duration").unwrap_or(0.0);
                    let paused = mpv.get_property::<bool>("pause").unwrap_or(false);
                    let vol = mpv.get_property::<f64>("volume").unwrap_or(100.0);
                    let eof = mpv.get_property::<bool>("eof-reached").unwrap_or(false);
                    let idle = mpv.get_property::<bool>("idle-active").unwrap_or(true);

                    // Requirement 4: If tab is currently active in single viewer or fullscreen and user toggles pause via MPV HUD/keys:
                    if view_active && !in_grid {
                        is_user_paused.store(paused, Ordering::Relaxed);
                    }

                    let user_paused_val = is_user_paused.load(Ordering::Relaxed);

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
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        self.observer_running.store(false, Ordering::Release);
        let _ = self.mpv.command("quit", &[]);
    }
}
