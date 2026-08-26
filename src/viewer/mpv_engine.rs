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

/// Central thread-safe MPV player engine wrapping `libmpv2` with native OSC, plugins, custom config files, and property observation.
pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    state: Arc<RwLock<MpvPlaybackState>>,
    current_file_path: Arc<RwLock<Option<PathBuf>>>,
    attached_wid: Arc<RwLock<Option<u64>>>,
    observer_running: Arc<AtomicBool>,
    callbacks: Arc<RwLock<Vec<MpvEventCallback>>>,
    fullscreen_callbacks: Arc<RwLock<Vec<MpvFullscreenCallback>>>,
}

impl MpvPlayer {
    /// Initializes a high-performance MPV player instance configured for native X11 embedding with full user config, plugins and double-click trapping.
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

        // Bind double-click and 'f' in MPV to cycle fullscreen (intercepted by RustView observer)
        let _ = mpv_instance.command("keybind", &["MBTN_LEFT_DBL", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["f", "cycle fullscreen"]);
        let _ = mpv_instance.command("keybind", &["F", "cycle fullscreen"]);

        // Ignore ESC and quit keys in MPV so Slint handles navigation and window closures
        let _ = mpv_instance.command("keybind", &["ESC", "ignore"]);
        let _ = mpv_instance.command("keybind", &["q", "ignore"]);
        let _ = mpv_instance.command("keybind", &["Q", "ignore"]);

        let mpv_arc = Arc::new(mpv_instance);
        let state = Arc::new(RwLock::new(MpvPlaybackState::default()));
        let current_file_path = Arc::new(RwLock::new(None));
        let attached_wid = Arc::new(RwLock::new(None));
        let observer_running = Arc::new(AtomicBool::new(true));
        let callbacks: Arc<RwLock<Vec<MpvEventCallback>>> = Arc::new(RwLock::new(Vec::new()));
        let fullscreen_callbacks: Arc<RwLock<Vec<MpvFullscreenCallback>>> =
            Arc::new(RwLock::new(Vec::new()));

        let player = Self {
            mpv: mpv_arc.clone(),
            state: state.clone(),
            current_file_path: current_file_path.clone(),
            attached_wid: attached_wid.clone(),
            observer_running: observer_running.clone(),
            callbacks: callbacks.clone(),
            fullscreen_callbacks: fullscreen_callbacks.clone(),
        };

        player.spawn_event_observer_thread();

        tracing::info!(
            elapsed_us = t0.elapsed().as_micros(),
            "[MpvPlayer: Init] Native libmpv2 player engine initialized with full config, plugins and double-click fullscreen trapping"
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

    /// Loads a video file from disk and starts immediate playback.
    pub fn load_file(&self, path: &Path) -> Result<(), ViewerError> {
        if !path.exists() {
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        let t0 = Instant::now();
        let path_str = path.to_string_lossy();

        self.mpv
            .command("loadfile", &[&path_str, "replace"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to execute 'loadfile' command: {:?}", e)))?;

        // Ensure playback starts immediately
        let _ = self.mpv.set_property("pause", false);

        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path_str.to_string());

        *self.current_file_path.write() = Some(path.to_path_buf());

        {
            let mut st = self.state.write();
            st.filename = filename.clone();
            st.is_playing = true;
            st.is_paused = false;
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

    /// Resumes playback.
    pub fn play(&self) -> Result<(), ViewerError> {
        self.mpv
            .set_property("pause", false)
            .map_err(|e| ViewerError::Mpv(format!("Failed to unpause MPV: {:?}", e)))?;

        self.state.write().is_paused = false;
        self.state.write().is_playing = true;
        self.notify_subscribers();
        Ok(())
    }

    /// Pauses playback.
    pub fn pause(&self) -> Result<(), ViewerError> {
        self.mpv
            .set_property("pause", true)
            .map_err(|e| ViewerError::Mpv(format!("Failed to pause MPV: {:?}", e)))?;

        self.state.write().is_paused = true;
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

    /// Registers a listener callback triggered when double-click or 'f' requests fullscreen in MPV.
    pub fn on_toggle_fullscreen<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.fullscreen_callbacks.write().push(Arc::new(callback));
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
        let callbacks = self.callbacks.clone();
        let fs_callbacks = self.fullscreen_callbacks.clone();

        std::thread::Builder::new()
            .name("mpv-event-observer".into())
            .spawn(move || {
                tracing::debug!("[MpvPlayer: Observer] Event observer thread started");

                while running.load(Ordering::Relaxed) {
                    // Poll MPV event queue with a responsive 20ms timeout
                    let _ = mpv.wait_event(0.02);

                    // 1. Intercept double-click / 'f' Fullscreen triggers from MPV
                    let fs_requested = mpv.get_property::<bool>("fullscreen").unwrap_or(false);
                    if fs_requested {
                        // Reset MPV's internal fullscreen property to false immediately so RustView manages the fullscreen window
                        let _ = mpv.set_property("fullscreen", false);

                        let fs_cbs = fs_callbacks.read().clone();
                        for cb in fs_cbs {
                            cb();
                        }
                    }

                    // 2. Sync live playback properties
                    let pos = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
                    let dur = mpv.get_property::<f64>("duration").unwrap_or(0.0);
                    let paused = mpv.get_property::<bool>("pause").unwrap_or(false);
                    let vol = mpv.get_property::<f64>("volume").unwrap_or(100.0);
                    let eof = mpv.get_property::<bool>("eof-reached").unwrap_or(false);
                    let idle = mpv.get_property::<bool>("idle-active").unwrap_or(true);

                    let has_changed = {
                        let mut st = state.write();
                        let changed = (st.position_secs - pos).abs() > 0.1
                            || (st.duration_secs - dur).abs() > 0.1
                            || st.is_paused != paused
                            || (st.volume - vol).abs() > 0.5
                            || st.is_eof != eof
                            || st.is_idle != idle;

                        if changed {
                            st.position_secs = pos;
                            st.duration_secs = dur;
                            st.is_paused = paused;
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
