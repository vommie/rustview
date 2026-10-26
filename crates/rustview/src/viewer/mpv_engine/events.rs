//! The one listener of a player and the mpv event observer thread. Nothing is polled: every value the player reports is an
//! observed property (`mpv_observe_property`), and the thread sleeps in `mpv_wait_event` until mpv has something to say.

use super::{MpvNotice, MpvPlayer, ab_repeat};
use crate::viewer::types::MpvPlaybackState;
use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The ids of the observed properties (`reply_userdata` of their change events; `ab_repeat` uses `0xab..`).
const TIME_POS: u64 = 1;
const DURATION: u64 = 2;
const PAUSE: u64 = 3;
const VOLUME: u64 = 4;
const MUTE: u64 = 5;
const EOF_REACHED: u64 = 6;
const IDLE_ACTIVE: u64 = 7;
const PARAMS_WIDTH: u64 = 8;
const PARAMS_HEIGHT: u64 = 9;
const PARAMS_ROTATE: u64 = 10;

const OBSERVED: [(&str, Format, u64); 10] = [
    ("time-pos", Format::Double, TIME_POS),
    ("duration", Format::Double, DURATION),
    ("pause", Format::Flag, PAUSE),
    ("volume", Format::Double, VOLUME),
    ("mute", Format::Flag, MUTE),
    ("eof-reached", Format::Flag, EOF_REACHED),
    ("idle-active", Format::Flag, IDLE_ACTIVE),
    ("video-params/dw", Format::Int64, PARAMS_WIDTH),
    ("video-params/dh", Format::Int64, PARAMS_HEIGHT),
    ("video-params/rotate", Format::Int64, PARAMS_ROTATE),
];

/// The display size of the video as mpv reports it (`video-params`), collected from its three observed parts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct VideoParams {
    width: Option<i64>,
    height: Option<i64>,
    rotate: i64,
    /// A file was loaded and its first `video-params` did not come yet: what arrives before belongs to the previous file.
    pub(super) awaiting_start: bool,
}

impl VideoParams {
    pub(super) fn get(self) -> Option<(i64, i64, i64)> {
        Some((self.width?, self.height?, self.rotate))
    }

    /// A new file is on its way: forget the size of the previous one.
    pub(super) fn reset(&mut self, awaiting_start: bool) {
        *self = Self {
            awaiting_start,
            ..Self::default()
        };
    }
}

impl MpvPlayer {
    /// Installs the listener of the player (replacing an earlier one): mute and user pause changes, playback changes, the video's
    /// size, a file that could not be played and the script `ab_repeat.lua`. Called on the observer thread.
    pub fn on_notice<F>(&self, callback: F)
    where
        F: Fn(MpvNotice) + Send + Sync + 'static,
    {
        *self.listener.write() = Some(Arc::new(callback));
    }

    /// Starts observing every property the player reports and the thread that receives the events.
    pub(super) fn spawn_event_observer_thread(&self) {
        for (name, format, id) in OBSERVED {
            if let Err(e) = self.mpv.observe_property(name, format, id) {
                tracing::warn!(property = name, error = ?e, "[MpvPlayer: Observer] A property could not be observed");
            }
        }
        let mut observer = Observer {
            mpv: Arc::clone(&self.mpv),
            state: Arc::clone(&self.state),
            params: Arc::clone(&self.params),
            running: Arc::clone(&self.observer_running),
            is_view_active: Arc::clone(&self.is_view_active),
            is_user_paused: Arc::clone(&self.is_user_paused),
            is_tab_grid_mode: Arc::clone(&self.is_tab_grid_mode),
            listener: Arc::clone(&self.listener),
            current_file: Arc::clone(&self.current_file_path),
            ab: Arc::clone(&self.ab),
            last_error: None,
            last_mute: None,
        };
        let spawned = std::thread::Builder::new()
            .name("mpv-event-observer".into())
            .spawn(move || observer.run());
        match spawned {
            Ok(handle) => *self.observer_handle.lock() = Some(handle),
            Err(e) => {
                tracing::error!(error = %e, "[MpvPlayer: Observer] The event observer thread could not be started");
            }
        }
    }

    /// Wakes the observer thread so it sees that it has to stop (`mpv_wait_event` waits without a timeout).
    pub(super) fn wake_observer(&self) {
        // SAFETY: `ctx` is the live handle owned by `self.mpv`, which outlives the call; `mpv_wakeup` is thread-safe.
        unsafe { libmpv2_sys::mpv_wakeup(self.mpv.ctx.as_ptr()) };
    }
}

fn notify(listener: &RwLock<Option<super::MpvNoticeCallback>>, notice: MpvNotice) {
    let callback = listener.read().clone();
    if let Some(callback) = callback {
        callback(notice);
    }
}

/// The observer thread's view of the player.
struct Observer {
    mpv: Arc<Mpv>,
    state: Arc<RwLock<MpvPlaybackState>>,
    params: Arc<RwLock<VideoParams>>,
    running: Arc<AtomicBool>,
    is_view_active: Arc<AtomicBool>,
    is_user_paused: Arc<AtomicBool>,
    is_tab_grid_mode: Arc<AtomicBool>,
    listener: Arc<RwLock<Option<super::MpvNoticeCallback>>>,
    current_file: Arc<RwLock<Option<PathBuf>>>,
    ab: Arc<RwLock<ab_repeat::AbSnapshot>>,
    /// The last error message of mpv for the current file (the reason shown when the file cannot be played).
    last_error: Option<String>,
    last_mute: Option<bool>,
}

impl Observer {
    fn run(&mut self) {
        tracing::debug!("[MpvPlayer: Observer] Event observer thread started");
        let mpv = Arc::clone(&self.mpv);
        while self.running.load(Ordering::Acquire) {
            // `None`: woken up (`mpv_wakeup`), or a property became unavailable (libmpv2 does not say which).
            let Some(event) = mpv.wait_event(-1.0) else {
                continue;
            };
            match event {
                Ok(Event::Shutdown) => {
                    if self.running.load(Ordering::Acquire) {
                        tracing::warn!("[MpvPlayer: Observer] mpv shut down by itself");
                        self.notify(MpvNotice::Failed("The player stopped unexpectedly.".to_owned()));
                    }
                    break;
                }
                Ok(Event::PropertyChange {
                    change, reply_userdata, ..
                }) => self.on_property(reply_userdata, &change),
                Ok(Event::StartFile) => self.on_start_file(),
                Ok(Event::FileLoaded) => self.on_file_loaded(),
                Ok(Event::EndFile(reason)) => {
                    tracing::debug!(reason, "[MpvPlayer: Observer] The file ended");
                }
                // libmpv2 turns an `end-file` with an error into this error; the other failing events are never requested here.
                Err(libmpv2::Error::Raw(code)) => self.on_load_failed(code),
                Ok(Event::LogMessage {
                    prefix, level, text, ..
                }) => self.on_log(prefix, level, text),
                Ok(_) => {}
                Err(e) => tracing::debug!(error = ?e, "[MpvPlayer: Observer] An event could not be read"),
            }
        }
        tracing::debug!("[MpvPlayer: Observer] Event observer thread terminated");
    }

    fn notify(&self, notice: MpvNotice) {
        notify(&self.listener, notice);
    }

    fn on_property(&mut self, id: u64, change: &PropertyData<'_>) {
        match (id, change) {
            (TIME_POS, PropertyData::Double(pos)) => self.state.write().position_secs = *pos,
            (DURATION, PropertyData::Double(duration)) => self.playback(|st| st.duration_secs = *duration),
            (PAUSE, PropertyData::Flag(paused)) => self.on_pause(*paused),
            (VOLUME, PropertyData::Double(volume)) => self.playback(|st| st.volume = *volume),
            (MUTE, PropertyData::Flag(muted)) => {
                if self.last_mute.replace(*muted).is_some_and(|before| before != *muted) {
                    self.notify(MpvNotice::Mute(*muted));
                }
            }
            (EOF_REACHED, PropertyData::Flag(eof)) => self.playback(|st| st.is_eof = *eof),
            (IDLE_ACTIVE, PropertyData::Flag(idle)) => self.playback(|st| {
                st.is_idle = *idle;
                st.is_playing = !*idle && !st.is_paused;
            }),
            (PARAMS_WIDTH | PARAMS_HEIGHT | PARAMS_ROTATE, PropertyData::Int64(value)) => {
                self.on_video_param(id, *value);
            }
            (_, PropertyData::Str(json)) => {
                if let Some(change) = ab_repeat::on_property(&self.mpv, &self.ab, id, json) {
                    self.notify(MpvNotice::AbRepeat(change));
                }
            }
            _ => {}
        }
    }

    /// Applies a change to the playback state and tells the listener when it really changed something.
    fn playback(&self, apply: impl FnOnce(&mut MpvPlaybackState)) {
        let changed = {
            let mut st = self.state.write();
            let before = st.clone();
            apply(&mut st);
            *st != before
        };
        if changed {
            self.notify(MpvNotice::Playback);
        }
    }

    /// The pause state counts as the user's choice only while the view is active: `set_view_active` stores the flag before it
    /// pauses, so a pause it causes arrives here with the flag already cleared.
    fn on_pause(&self, paused: bool) {
        let before = self.is_user_paused.load(Ordering::SeqCst);
        if self.is_view_active.load(Ordering::SeqCst) && !self.is_tab_grid_mode.load(Ordering::SeqCst) {
            self.is_user_paused.store(paused, Ordering::SeqCst);
        }
        let user_paused = self.is_user_paused.load(Ordering::SeqCst);
        self.playback(|st| {
            st.is_paused = paused;
            st.is_user_paused = user_paused;
            st.is_playing = !st.is_idle && !paused;
        });
        if user_paused != before {
            self.notify(MpvNotice::UserPause(user_paused));
        }
    }

    fn on_video_param(&self, id: u64, value: i64) {
        let complete = {
            let mut params = self.params.write();
            if params.awaiting_start {
                return;
            }
            match id {
                PARAMS_WIDTH => params.width = Some(value),
                PARAMS_HEIGHT => params.height = Some(value),
                _ => params.rotate = value,
            }
            params.get().is_some()
        };
        if complete {
            self.notify(MpvNotice::VideoParams);
        }
    }

    fn on_start_file(&mut self) {
        self.last_error = None;
        self.params.write().reset(false);
        self.playback(|st| {
            st.position_secs = 0.0;
            st.duration_secs = 0.0;
            st.is_eof = false;
        });
    }

    fn on_file_loaded(&self) {
        if let Some(change) = ab_repeat::settle(&self.mpv, &self.ab) {
            self.notify(MpvNotice::AbRepeat(change));
        }
    }

    /// mpv could not play the file: the reason is its error code plus the last error it logged for the file.
    fn on_load_failed(&mut self, code: libmpv2::MpvError) {
        let reason = libmpv2_sys::mpv_error_str(code);
        let message = match self.last_error.take() {
            Some(detail) => format!("{} ({reason})", detail.trim_end_matches('.')),
            None => capitalized(reason),
        };
        let file = self.current_file.read().clone();
        tracing::warn!(
            code,
            reason = %rustview_core::settings::LogObscurer::foreign_text(&message, file.as_deref()),
            "[MpvPlayer: Observer] The file could not be played"
        );
        self.playback(|st| {
            st.is_playing = false;
            st.is_eof = true;
        });
        self.notify(MpvNotice::Failed(message));
    }

    fn on_log(&mut self, prefix: &str, level: &str, text: &str) {
        let file = self.current_file.read().clone();
        if matches!(level, "fatal" | "error") && !text.trim().is_empty() && names_the_file(prefix) {
            self.last_error = Some(text.trim().to_owned());
        }
        super::messages::handle(prefix, level, text, file.as_deref());
    }
}

/// Whether an error of this mpv module can be the reason a file does not play: the decoders and the outputs report problems
/// they recover from (a hardware decoder that falls back to software) and never decide that.
fn names_the_file(prefix: &str) -> bool {
    !["vd", "vo", "ao", "ffmpeg/video", "ffmpeg/audio"]
        .iter()
        .any(|module| prefix == *module || prefix.starts_with(&format!("{module}/")))
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_params_are_complete_with_width_and_height() {
        let mut params = VideoParams::default();
        assert_eq!(params.get(), None);
        params.width = Some(1920);
        assert_eq!(params.get(), None);
        params.height = Some(1080);
        params.rotate = 90;
        assert_eq!(params.get(), Some((1920, 1080, 90)));
        params.reset(true);
        assert_eq!(params.get(), None);
        assert!(params.awaiting_start);
    }

    #[test]
    fn decoder_errors_are_no_reason() {
        assert!(names_the_file("cplayer"));
        assert!(names_the_file("ffmpeg/demuxer"));
        assert!(!names_the_file("ffmpeg/video"));
        assert!(!names_the_file("vo/libmpv"));
    }

    #[test]
    fn reasons_start_with_a_capital() {
        assert_eq!(capitalized("loading failed"), "Loading failed");
        assert_eq!(capitalized(""), "");
    }
}
