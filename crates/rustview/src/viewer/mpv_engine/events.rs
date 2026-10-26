//! Subscriber callbacks and the mpv event observer thread.

use super::{MpvPlayer, ab_repeat};
use crate::viewer::types::MpvPlaybackState;
use rustview_core::settings::schema::{MPV_OBSERVER_ACTIVE_POLL_MS, MPV_OBSERVER_IDLE_POLL_MS};
use rustview_core::util::cast::Az;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// At most this many queued mpv events are taken per turn of the observer loop (the properties are polled in between).
const MAX_EVENTS_PER_TURN: usize = 256;

impl MpvPlayer {
    pub fn subscribe<F>(&self, callback: F)
    where
        F: Fn(&MpvPlaybackState) + Send + Sync + 'static,
    {
        self.callbacks.write().push(Arc::new(callback));
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

    pub(super) fn notify_subscribers(&self) {
        let snapshot = self.state.read().clone();
        let callbacks = self.callbacks.read().clone();
        for cb in callbacks {
            cb(&snapshot);
        }
    }

    /// Background polling and event observer loop running at high frequency without busy waiting.
    pub(super) fn spawn_event_observer_thread(&self) {
        let mpv = self.mpv.clone();
        let state = self.state.clone();
        let running = self.observer_running.clone();
        let is_view_active = self.is_view_active.clone();
        let is_user_paused = self.is_user_paused.clone();
        let is_tab_grid_mode = self.is_tab_grid_mode.clone();
        let callbacks = self.callbacks.clone();
        let mute_callbacks = self.mute_callbacks.clone();
        let pause_callbacks = self.pause_callbacks.clone();
        let current_file = self.current_file_path.clone();
        let ab = self.ab.clone();
        let ab_callbacks = self.ab_callbacks.clone();

        let observer = std::thread::Builder::new()
            .name("mpv-event-observer".into())
            .spawn(move || {
                tracing::debug!("[MpvPlayer: Observer] Event observer thread started");

                let mut last_mute: Option<bool> = None;
                let mut last_user_paused: Option<bool> = None;

                while running.load(Ordering::Relaxed) {
                    // A player that is neither shown nor in the tab grid has nobody to react to: poll it less often.
                    let hidden = !is_view_active.load(Ordering::Relaxed) && !is_tab_grid_mode.load(Ordering::Relaxed);
                    let poll_ms = if hidden {
                        MPV_OBSERVER_IDLE_POLL_MS
                    } else {
                        MPV_OBSERVER_ACTIVE_POLL_MS
                    };
                    // Waits for the first event, then takes what else is queued (mpv's messages come in bursts).
                    let mut timeout = poll_ms.az::<f64>() / 1000.0;
                    for _ in 0..MAX_EVENTS_PER_TURN {
                        let Some(event) = mpv.wait_event(timeout) else {
                            break;
                        };
                        timeout = 0.0;
                        match event {
                            Ok(libmpv2::events::Event::PropertyChange {
                                change: libmpv2::events::PropertyData::Str(json),
                                reply_userdata,
                                ..
                            }) => {
                                if let Some(change) = ab_repeat::on_property(&mpv, &ab, reply_userdata, json) {
                                    for cb in ab_callbacks.read().clone() {
                                        cb(change);
                                    }
                                }
                            }
                            Ok(libmpv2::events::Event::FileLoaded) => {
                                if let Some(change) = ab_repeat::settle(&mpv, &ab) {
                                    for cb in ab_callbacks.read().clone() {
                                        cb(change);
                                    }
                                }
                            }
                            Ok(libmpv2::events::Event::LogMessage {
                                prefix, level, text, ..
                            }) => {
                                let file = current_file.read().clone();
                                super::messages::handle(prefix, level, text, file.as_deref());
                            }
                            _ => {}
                        }
                    }

                    // Sync live playback properties
                    let pos = mpv.get_property::<f64>("time-pos").unwrap_or(0.0);
                    let dur = mpv.get_property::<f64>("duration").unwrap_or(0.0);
                    // The pause state counts as the user's choice only if the view stayed active while it was read.
                    // `set_view_active` stores the flag first and pauses afterwards, so a pause it caused is always
                    // seen with the flag already cleared on one of the two samples.
                    let active_before =
                        is_view_active.load(Ordering::SeqCst) && !is_tab_grid_mode.load(Ordering::SeqCst);
                    let paused = mpv.get_property::<bool>("pause").unwrap_or(false);
                    let active_after =
                        is_view_active.load(Ordering::SeqCst) && !is_tab_grid_mode.load(Ordering::SeqCst);
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
