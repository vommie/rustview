//! Loading, playing, seeking and stopping.

use super::MpvPlayer;
use crate::viewer::error::ViewerError;
use crate::viewer::types::MpvPlaybackState;
use rustview_core::settings::LogObscurer;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

/// Where a video is and how it plays, in short.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackNumbers {
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    /// Percent (mpv's `volume`).
    pub volume: f64,
}

impl MpvPlayer {
    /// Loads a video file from disk and starts immediate playback (resets user pause state to false).
    pub fn load_file(&self, path: &Path) -> Result<(), ViewerError> {
        if !path.exists() {
            return Err(ViewerError::FileNotFound(path.to_path_buf()));
        }

        let t0 = Instant::now();
        let path_str = path.to_string_lossy();
        // A zoom belongs to the file it was made on: the next file of a reused player starts whole.
        self.reset_video_zoom();

        self.mpv
            .command("loadfile", &[&path_str, "replace"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to execute 'loadfile' command: {e:?}")))?;

        self.is_user_paused.store(false, Ordering::SeqCst);
        let _ = self.mpv.set_property("pause", false);

        let filename = path
            .file_name()
            .map_or_else(|| path_str.to_string(), |n| n.to_string_lossy().to_string());

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
            .map_err(|e| ViewerError::Mpv(format!("Failed to unpause MPV: {e:?}")))?;

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
        if self.is_view_active.load(Ordering::SeqCst) && !self.is_tab_grid_mode.load(Ordering::SeqCst) {
            self.is_user_paused.store(true, Ordering::SeqCst);
        }
        self.mpv
            .set_property("pause", true)
            .map_err(|e| ViewerError::Mpv(format!("Failed to pause MPV: {e:?}")))?;

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
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek relative: {e:?}")))?;
        Ok(())
    }

    pub fn seek_absolute(&self, seconds: f64) -> Result<(), ViewerError> {
        let sec_str = seconds.to_string();
        self.mpv
            .command("seek", &[&sec_str, "absolute"])
            .map_err(|e| ViewerError::Mpv(format!("Failed to seek absolute: {e:?}")))?;
        Ok(())
    }

    pub fn set_volume(&self, volume_percent: f64) -> Result<(), ViewerError> {
        let clamped = volume_percent.clamp(0.0, 150.0);
        self.mpv
            .set_property("volume", clamped)
            .map_err(|e| ViewerError::Mpv(format!("Failed to set volume: {e:?}")))?;

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
        let color = rustview_core::util::hex_color::format_rgb(rgb);
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
            .map_err(|e| ViewerError::Mpv(format!("Failed to set mute: {e:?}")))
    }

    /// Display size and rotation of the playing video as reported by mpv (`video-params/dw`, `dh`, `rotate`).
    /// `dw`/`dh` are not rotated; `None` until the first frame of the file is decoded.
    pub fn video_display_params(&self) -> Option<(i64, i64, i64)> {
        let width = self.mpv.get_property::<i64>("video-params/dw").ok()?;
        let height = self.mpv.get_property::<i64>("video-params/dh").ok()?;
        let rotate = self.mpv.get_property::<i64>("video-params/rotate").unwrap_or(0);
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
        self.mpv.get_property::<bool>("eof-reached").unwrap_or(false)
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

    /// The numbers of the playback state as the observer saw them last (no call into mpv; for the control bar's ticks).
    pub fn playback(&self) -> PlaybackNumbers {
        let st = self.state.read();
        PlaybackNumbers {
            position: st.position_secs,
            duration: st.duration_secs,
            paused: st.is_paused,
            volume: st.volume,
        }
    }
}
