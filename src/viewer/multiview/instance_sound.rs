//! Instance behavior around sound: applying the audio rules to the players, noticing mute changes made inside mpv and the
//! user's mute toggle.

use super::audio::{AudioMode, AudioOrigin, TileAudio};
use super::instance::MultiviewInstance;
use super::follow::TileKey;

impl MultiviewInstance {
    /// Pushes the mute state demanded by the audio rules to every video player whose actual state differs.
    pub(super) fn apply_audio(&mut self) {
        let mut changed = false;
        for tile in &mut self.tiles {
            let (Some(video), Some(audio)) = (tile.video.as_mut(), tile.audio) else {
                continue;
            };
            let target = audio.target_mute(video.native_mute);
            if target == video.last_mute {
                continue;
            }
            match video.player.set_mute(target) {
                Ok(()) => {
                    video.last_mute = target;
                    changed = true;
                    tracing::debug!(
                        media_id = tile.media_id,
                        muted = target,
                        "[Multiview: Audio] Mute state applied"
                    );
                }
                Err(e) => {
                    tracing::warn!(media_id = tile.media_id, error = %e, "[Multiview: Audio] Could not change the mute state")
                }
            }
        }
        if changed {
            self.bump();
        }
    }

    /// The player of tile `key` reports its mute state. A change that did not come from us (key `m`, mpv on-screen
    /// controller) counts as the user's choice and the sound rules leave it alone from then on.
    pub fn on_player_mute_changed(&mut self, key: TileKey, muted: bool) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.key == key) else {
            return;
        };
        let media_id = tile.media_id;
        let (Some(video), Some(audio)) = (tile.video.as_mut(), tile.audio.as_mut()) else {
            return;
        };
        if video.last_mute == muted {
            return;
        }
        video.last_mute = muted;
        *audio = TileAudio {
            mode: if muted { AudioMode::Muted } else { AudioMode::Unmuted },
            origin: AudioOrigin::User,
        };
        tracing::debug!(
            media_id,
            muted,
            "[Multiview: Audio] Mute changed inside mpv; kept as a user choice"
        );
        self.bump();
    }

    /// The player of tile `key` reports a change of the user's pause state (the bar reads the state live; this only makes
    /// the hosts redraw).
    pub fn on_player_pause_changed(&mut self, key: TileKey, paused: bool) {
        if let Some(tile) = self.tiles.iter().find(|t| t.key == key && t.video.is_some()) {
            tracing::debug!(media_id = tile.media_id, paused, "[Multiview: Playback] Pause state changed");
            self.bump();
        }
    }

    /// The user mutes or unmutes tile `index`; either way the choice is theirs and the sound rules leave it alone.
    pub(super) fn toggle_mute_at(&mut self, index: usize) {
        let Some(tile) = self.tiles.get_mut(index) else {
            return;
        };
        let (Some(video), Some(audio)) = (tile.video.as_mut(), tile.audio.as_mut()) else {
            return;
        };
        let next = audio.toggled_by_user();
        let target = next.target_mute(video.native_mute);
        match video.player.set_mute(target) {
            Ok(()) => {
                video.last_mute = target;
                *audio = next;
                tracing::info!(
                    media_id = tile.media_id,
                    muted = target,
                    "[Multiview: Audio] Mute toggled by the user"
                );
            }
            Err(e) => {
                tracing::warn!(media_id = tile.media_id, error = %e, "[Multiview: Audio] Mute toggle failed")
            }
        }
        self.bump();
    }
}
