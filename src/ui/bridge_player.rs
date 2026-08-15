use super::MainWindow;
use crate::player::events::format_playback_time;
use crate::player::{Player, PlayerController, PlayerEvent};
use slint::Timer;
use std::sync::Arc;

/// Subsystem managing video player timer polling and playback control callbacks.
pub struct BridgePlayer;

impl BridgePlayer {
    /// Spawns timer polling player events from the native libmpv background thread.
    pub fn start_event_timer(
        player: Arc<PlayerController>,
        window_weak: slint::Weak<MainWindow>,
    ) -> Timer {
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(50),
            move || {
                for event in player.poll_events() {
                    if let Some(w) = window_weak.upgrade() {
                        match event {
                            PlayerEvent::PositionChanged {
                                current_time,
                                duration,
                            } => {
                                w.set_player_current_time(current_time as f32);
                                w.set_player_duration(duration as f32);
                                w.set_player_time_str(
                                    format!(
                                        "{} / {}",
                                        format_playback_time(current_time),
                                        format_playback_time(duration)
                                    )
                                    .into(),
                                );
                            }
                            PlayerEvent::PlaybackStateChanged { is_paused } => {
                                w.set_player_is_playing(!is_paused);
                            }
                            PlayerEvent::PlaybackEnded => {
                                w.set_player_is_playing(false);
                            }
                            _ => {}
                        }
                    }
                }
            },
        );
        timer
    }

    /// Registers player user action callbacks on MainWindow.
    pub fn register_callbacks(window: &MainWindow, player: Arc<PlayerController>) {
        let p_play = player.clone();
        window.on_player_toggle_play(move || {
            let is_paused = p_play.raw_mpv().is_paused();
            if is_paused {
                let _ = p_play.play();
            } else {
                let _ = p_play.pause();
            }
        });

        let p_seek = player.clone();
        window.on_player_seek(move |pos| {
            let _ = p_seek.seek(pos as f64);
        });

        let p_vol = player.clone();
        window.on_player_set_volume(move |vol| {
            let _ = p_vol.set_volume(vol as f64);
        });

        let p_stop = player;
        window.on_player_stop(move || {
            let _ = p_stop.stop();
        });
    }
}
