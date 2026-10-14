//! Instance behavior around sound: applying the audio rules to the players, noticing mute changes made inside mpv and the
//! user's mute toggle.

use super::audio::{AudioMode, AudioOrigin, TileAudio};
use super::instance::MultiviewInstance;

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

    /// A mute change that did not come from us (key `m`, mpv on-screen controller) counts as the user's choice.
    /// Returns `true` if any tile changed.
    pub(super) fn poll_mute(&mut self) -> bool {
        let mut changed = false;
        for tile in &mut self.tiles {
            let (Some(video), Some(audio)) = (tile.video.as_mut(), tile.audio.as_mut()) else {
                continue;
            };
            let now = video.player.get_mute();
            if now == video.last_mute {
                continue;
            }
            video.last_mute = now;
            let mode = if now {
                AudioMode::Muted
            } else {
                AudioMode::Unmuted
            };
            *audio = TileAudio {
                mode,
                origin: AudioOrigin::User,
            };
            changed = true;
            tracing::debug!(
                media_id = tile.media_id,
                muted = now,
                "[Multiview: Audio] Mute changed inside mpv; kept as a user choice"
            );
        }
        changed
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
