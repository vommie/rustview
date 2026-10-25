//! The video player keys on the selected video (`MultiviewCommand::Player`): mpv takes no keys itself, so `RustView` asks the
//! player of the selected tile for what its own keys did (pause, mute, jumps, frame steps, volume, speed).

use super::command::PlayerCommand;
use super::hint::HintKind;
use super::instance::MultiviewInstance;
use super::instance_commands::CommandResult;
use super::model::TileKind;
use super::player_prefs;
use super::route::UiAction;
use super::volume_feedback::{MuteInput, VolumeInput};
use rustview_core::util::cast::Az;
use std::time::Instant;

/// How much louder or quieter one key press makes a video, in percent.
const VOLUME_STEP: f64 = 5.0;

/// How much faster or slower one key press makes a video (mpv's own `[` and `]`).
const SPEED_STEP: f64 = 1.1;

impl MultiviewInstance {
    pub(super) fn player_key(&mut self, index: usize, command: PlayerCommand) -> CommandResult {
        let tile = &self.tiles[index];
        if tile.kind != TileKind::Video || tile.error.is_some() {
            return Err(HintKind::NotAVideo);
        }
        if command == PlayerCommand::Mute {
            self.toggle_mute_at(index);
            self.show_mute_feedback(index, MuteInput::Hotkey, Instant::now());
            return Ok(UiAction::None);
        }
        let Some(video) = tile.video.as_ref() else {
            return Err(HintKind::NotAVideo);
        };
        tracing::info!(
            media_id = tile.media_id,
            ?command,
            "[Multiview: Player] Key on the selected video"
        );
        let prefs = player_prefs::current();
        let result = match command {
            PlayerCommand::PlayPause => video.player.toggle_pause(),
            PlayerCommand::Jump { forward, long } => {
                let secs = if long { prefs.jump_long_secs } else { prefs.jump_secs }.az::<f64>();
                video.player.seek_relative(if forward { secs } else { -secs })
            }
            PlayerCommand::Frame { forward } => video.player.frame_step(forward),
            PlayerCommand::Volume { louder } => {
                video.player.add_volume(if louder { VOLUME_STEP } else { -VOLUME_STEP })
            }
            PlayerCommand::Speed { faster } => {
                video
                    .player
                    .multiply_speed(if faster { SPEED_STEP } else { 1.0 / SPEED_STEP })
            }
            PlayerCommand::Mute => Ok(()),
        };
        match result {
            Ok(()) if matches!(command, PlayerCommand::Volume { .. }) => {
                self.show_volume_feedback(index, VolumeInput::Hotkey, Instant::now());
            }
            Ok(()) => {}
            Err(e) => {
                tracing::warn!(media_id = tile.media_id, ?command, error = %e, "[Multiview: Player] The player key failed");
            }
        }
        Ok(UiAction::None)
    }
}
