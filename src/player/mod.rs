pub mod commands;
pub mod error;
pub mod events;
pub mod mpv;
pub mod properties;
pub mod render;

pub use commands::PlayerCommand;
pub use error::PlayerError;
pub use events::{PlayerEvent, format_playback_time};
pub use mpv::MpvPlayer;
pub use properties::HwDecMode;
pub use render::RenderConfig;

use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Core Player Trait Abstraction
pub trait Player: Send + Sync {
    fn load(&self, path: &Path, autoplay: bool) -> Result<(), PlayerError>;
    fn play(&self) -> Result<(), PlayerError>;
    fn pause(&self) -> Result<(), PlayerError>;
    fn seek(&self, position: f64) -> Result<(), PlayerError>;
    fn set_volume(&self, volume: f64) -> Result<(), PlayerError>;
    fn stop(&self) -> Result<(), PlayerError>;
}

/// High-level thread-safe Player Facade Controller
pub struct PlayerController {
    player: Arc<MpvPlayer>,
    current_file: RwLock<Option<PathBuf>>,
}

impl PlayerController {
    pub fn new(config: RenderConfig) -> Result<Self, PlayerError> {
        let player = Arc::new(MpvPlayer::new(config)?);
        Ok(Self {
            player,
            current_file: RwLock::new(None),
        })
    }

    pub fn current_file(&self) -> Option<PathBuf> {
        self.current_file.read().clone()
    }

    pub fn poll_events(&self) -> Vec<PlayerEvent> {
        self.player.get_events()
    }

    pub fn raw_mpv(&self) -> Arc<MpvPlayer> {
        self.player.clone()
    }
}

impl Player for PlayerController {
    fn load(&self, path: &Path, autoplay: bool) -> Result<(), PlayerError> {
        let buf = path.to_path_buf();
        self.player.execute(PlayerCommand::LoadFile {
            path: buf.clone(),
            autoplay,
        })?;
        *self.current_file.write() = Some(buf);
        Ok(())
    }

    fn play(&self) -> Result<(), PlayerError> {
        self.player.execute(PlayerCommand::Play)
    }

    fn pause(&self) -> Result<(), PlayerError> {
        self.player.execute(PlayerCommand::Pause)
    }

    fn seek(&self, position: f64) -> Result<(), PlayerError> {
        self.player.execute(PlayerCommand::SeekAbsolute(position))
    }

    fn set_volume(&self, volume: f64) -> Result<(), PlayerError> {
        self.player.execute(PlayerCommand::SetVolume(volume))
    }

    fn stop(&self) -> Result<(), PlayerError> {
        self.player.execute(PlayerCommand::Stop)?;
        *self.current_file.write() = None;
        Ok(())
    }
}
