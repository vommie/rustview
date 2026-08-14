use super::commands::PlayerCommand;
use super::error::PlayerError;
use super::events::PlayerEvent;
use super::properties::*;
use super::render::RenderConfig;
use libmpv2::Mpv;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub struct MpvPlayer {
    mpv: Arc<Mutex<Mpv>>,
    event_rx: Mutex<Receiver<PlayerEvent>>,
    _event_thread: Option<JoinHandle<()>>,
}

impl MpvPlayer {
    pub fn new(config: RenderConfig) -> Result<Self, PlayerError> {
        let mpv_instance = Mpv::with_initializer(|init| {
            init.set_property(PROP_HWDEC, config.hwdec.as_str())?;
            init.set_property(PROP_VO, config.video_output.as_str())?;
            init.set_property(PROP_IDLE, "yes")?;
            init.set_property(PROP_KEEP_OPEN, "yes")?;
            init.set_property("audio-device", config.audio_device.as_str())?;
            Ok(())
        })?;

        let mpv = Arc::new(Mutex::new(mpv_instance));
        let (event_tx, event_rx) = channel();

        let event_thread = Self::spawn_event_poller(mpv.clone(), event_tx);

        Ok(Self {
            mpv,
            event_rx: Mutex::new(event_rx),
            _event_thread: Some(event_thread),
        })
    }

    fn spawn_event_poller(mpv: Arc<Mutex<Mpv>>, tx: Sender<PlayerEvent>) -> JoinHandle<()> {
        thread::Builder::new()
            .name("mpv-poller".into())
            .spawn(move || {
                let mut last_pos = -1.0;
                let mut last_paused = false;

                loop {
                    thread::sleep(Duration::from_millis(50));

                    let state_opt = {
                        let lock = mpv.lock();
                        let pos: Option<f64> = lock.get_property(PROP_TIME_POS).ok();
                        let dur: Option<f64> = lock.get_property(PROP_DURATION).ok();
                        let paused: Option<bool> = lock.get_property(PROP_PAUSE).ok();
                        let eof: Option<bool> = lock.get_property(PROP_EOF_REACHED).ok();
                        (pos, dur, paused, eof)
                    };

                    let (pos_opt, dur_opt, paused_opt, eof_opt) = state_opt;

                    if let Some(true) = eof_opt {
                        let _ = tx.send(PlayerEvent::PlaybackEnded);
                        continue;
                    }

                    if let Some(paused) = paused_opt
                        && paused != last_paused
                    {
                        last_paused = paused;
                        let _ = tx.send(PlayerEvent::PlaybackStateChanged { is_paused: paused });
                    }

                    if let Some(pos) = pos_opt
                        && (pos - last_pos).abs() >= 0.25
                    {
                        last_pos = pos;
                        let duration = dur_opt.unwrap_or(0.0);
                        let _ = tx.send(PlayerEvent::PositionChanged {
                            current_time: pos,
                            duration,
                        });
                    }
                }
            })
            .expect("Failed to spawn mpv poller thread")
    }

    pub fn execute(&self, command: PlayerCommand) -> Result<(), PlayerError> {
        let lock = self.mpv.lock();
        match command {
            PlayerCommand::LoadFile { path, autoplay } => {
                let path_str = path.to_string_lossy();
                lock.command("loadfile", &[&path_str, "replace"])?;
                lock.set_property(PROP_PAUSE, !autoplay)?;
            }
            PlayerCommand::Play => {
                lock.set_property(PROP_PAUSE, false)?;
            }
            PlayerCommand::Pause => {
                lock.set_property(PROP_PAUSE, true)?;
            }
            PlayerCommand::TogglePause => {
                let is_paused: bool = lock.get_property(PROP_PAUSE).unwrap_or(false);
                lock.set_property(PROP_PAUSE, !is_paused)?;
            }
            PlayerCommand::Stop => {
                lock.command("stop", &[])?;
            }
            PlayerCommand::SeekAbsolute(seconds) => {
                let sec_str = seconds.to_string();
                lock.command("seek", &[&sec_str, "absolute"])?;
            }
            PlayerCommand::SeekRelative(seconds) => {
                let sec_str = seconds.to_string();
                lock.command("seek", &[&sec_str, "relative"])?;
            }
            PlayerCommand::SetVolume(vol) => {
                let clamped_vol = vol.clamp(0.0, 100.0);
                lock.set_property(PROP_VOLUME, clamped_vol)?;
            }
            PlayerCommand::SetMute(muted) => {
                lock.set_property(PROP_MUTE, muted)?;
            }
            PlayerCommand::ToggleMute => {
                let is_muted: bool = lock.get_property(PROP_MUTE).unwrap_or(false);
                lock.set_property(PROP_MUTE, !is_muted)?;
            }
        }
        Ok(())
    }

    pub fn get_events(&self) -> Vec<PlayerEvent> {
        let mut events = Vec::new();
        let rx = self.event_rx.lock();
        while let Ok(evt) = rx.try_recv() {
            events.push(evt);
        }
        events
    }

    pub fn current_position(&self) -> f64 {
        self.mpv.lock().get_property(PROP_TIME_POS).unwrap_or(0.0)
    }

    pub fn duration(&self) -> f64 {
        self.mpv.lock().get_property(PROP_DURATION).unwrap_or(0.0)
    }

    pub fn is_paused(&self) -> bool {
        self.mpv.lock().get_property(PROP_PAUSE).unwrap_or(true)
    }

    pub fn volume(&self) -> f64 {
        self.mpv.lock().get_property(PROP_VOLUME).unwrap_or(100.0)
    }
}
