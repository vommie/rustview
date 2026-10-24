//! Subscriber callbacks and the mpv event observer thread.

use super::{CLICK_MESSAGE, DELETE_KEY_MESSAGE, MpvPlayer};
use crate::viewer::types::MpvPlaybackState;
use rustview_core::settings::schema::{MPV_OBSERVER_ACTIVE_POLL_MS, MPV_OBSERVER_IDLE_POLL_MS};
use rustview_core::util::cast::Az;
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl MpvPlayer {
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

    /// A rating key (Ctrl+0..5) was pressed in the player's window; the callback gets the rating.
    pub fn on_rating_requested<F>(&self, callback: F)
    where
        F: Fn(u8) + Send + Sync + 'static,
    {
        let mut cbs = self.rating_callbacks.write();
        cbs.clear();
        cbs.push(Arc::new(callback));
    }

    /// A plain left click on the player's window (outside the tab grid, whose clicks have their own callbacks).
    pub fn on_clicked<F>(&self, callback: F)
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut cbs = self.click_callbacks.write();
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
        let fs_callbacks = self.fullscreen_callbacks.clone();
        let close_callbacks = self.close_tab_callbacks.clone();
        let grid_left_callbacks = self.grid_left_click_callbacks.clone();
        let grid_middle_callbacks = self.grid_middle_click_callbacks.clone();
        let grid_wheel_up_callbacks = self.grid_wheel_up_callbacks.clone();
        let grid_wheel_down_callbacks = self.grid_wheel_down_callbacks.clone();
        let convert_callbacks = self.convert_to_tab_callbacks.clone();
        let rename_callbacks = self.rename_callbacks.clone();
        let delete_callbacks = self.delete_callbacks.clone();
        let rating_callbacks = self.rating_callbacks.clone();
        let click_callbacks = self.click_callbacks.clone();
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
                    let hidden = !is_view_active.load(Ordering::Relaxed) && !is_tab_grid_mode.load(Ordering::Relaxed);
                    let poll_ms = if hidden {
                        MPV_OBSERVER_IDLE_POLL_MS
                    } else {
                        MPV_OBSERVER_ACTIVE_POLL_MS
                    };
                    if let Some(Ok(libmpv2::events::Event::ClientMessage(args))) =
                        mpv.wait_event(poll_ms.az::<f64>() / 1000.0)
                    {
                        if args.first() == Some(&DELETE_KEY_MESSAGE) {
                            let delete_cbs = delete_callbacks.read().clone();
                            for cb in delete_cbs {
                                cb();
                            }
                        } else if args.first() == Some(&CLICK_MESSAGE) {
                            for cb in click_callbacks.read().clone() {
                                cb();
                            }
                        } else if let Some(rating) = super::rating::rating_of_message(&args) {
                            let rating_cbs = rating_callbacks.read().clone();
                            for cb in rating_cbs {
                                cb(rating);
                            }
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
                        let deinterlace_val = mpv.get_property::<bool>("deinterlace").unwrap_or(false);
                        if deinterlace_val != last_deinterlace {
                            last_deinterlace = deinterlace_val;
                            let cbs = grid_wheel_up_callbacks.read().clone();
                            for cb in cbs {
                                cb();
                            }
                        }

                        // Wheel Down in TabGrid Mode (scrolls grid down)
                        let sub_vis_val = mpv.get_property::<bool>("sub-visibility").unwrap_or(true);
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
                    let close_requested = mpv.get_property::<bool>("window-minimized").unwrap_or(false);
                    if close_requested {
                        let _ = mpv.set_property("window-minimized", false);
                        let close_cbs = close_callbacks.read().clone();
                        for cb in close_cbs {
                            cb();
                        }
                    }

                    // 5. Intercept Ctrl+Click / Ctrl+DoubleClick in Fullscreen to convert the fullscreen Multiview to a background tab
                    let convert_requested = mpv.get_property::<bool>("window-maximized").unwrap_or(false);
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
