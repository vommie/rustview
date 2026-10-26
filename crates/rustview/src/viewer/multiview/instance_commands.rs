//! Instance behavior around the keys that act on one medium (`command`): the target is always the selected tile (`focus`),
//! wherever the pointer is and whichever window delivered the key, and a key that does not fit it is refused with the reason
//! the host shows as a hint. A key that reaches the selected tile also shows its marks for a moment while they are hidden with
//! the idle cursor (`instance_focus`). Esc is no command: `escape_crop` cancels a crop tool, the selected tile's first.

use super::command::{MultiviewCommand, ZoomKey};
use super::follow::TileKey;
use super::hint::HintKind;
use super::instance::MultiviewInstance;
use super::media_actions::MediaActionId;
use super::model::TileKind;
use super::route::{HostMode, UiAction};
use rustview_core::library::models::MediaId;
use std::time::Instant;

/// What a key did: the action the application still has to carry out, or why nothing happened.
pub type CommandResult = Result<UiAction, HintKind>;

impl MultiviewInstance {
    fn takes_keys(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
    }

    /// The selected tile a key acts on (the lone tile, or the selected one of several) and its medium; `NoSelection` when
    /// several are shown and none is selected. The marks of the tile show for a moment.
    pub fn command_target(&mut self, now: Instant) -> Result<(TileKey, MediaId), HintKind> {
        let key = self.focus_target().ok_or(HintKind::NoSelection)?;
        let media = self
            .tiles
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.media_id)
            .ok_or(HintKind::NoSelection)?;
        self.reveal_marks(now);
        Ok((key, media))
    }

    /// Runs a key on the selected medium. Rename, delete and saving end in an action for the application; zoom, crop, undo
    /// and redo happen here. Rating and refreshing need the library: the host carries them out after `command_target`, so
    /// here they only check the target. Keys of a host that shows the Multiview only as a picture (the tab grid) do nothing.
    pub fn run_command(&mut self, command: MultiviewCommand) -> CommandResult {
        if !self.takes_keys() {
            tracing::debug!(?command, "[Multiview: Keys] Ignored: the host takes no keys");
            return Ok(UiAction::None);
        }
        let (key, media) = self.command_target(Instant::now())?;
        let index = self.tile_index(key).ok_or(HintKind::NoSelection)?;
        tracing::info!(
            ?command,
            tile = key,
            media_id = media,
            "[Multiview: Keys] Command on the selected medium"
        );
        match command {
            MultiviewCommand::Rate(_) | MultiviewCommand::Refresh => Ok(UiAction::None),
            MultiviewCommand::Rename => {
                self.rename_target = Some(media);
                Ok(UiAction::RequestRename)
            }
            MultiviewCommand::Delete => Ok(UiAction::DeleteTile(media)),
            MultiviewCommand::Bar(action) => self.bar_key(index, action),
            MultiviewCommand::Player(command) => self.player_key(index, command),
            MultiviewCommand::Zoom(zoom) => self.zoom_selected(index, zoom),
            MultiviewCommand::ApplyCrop => {
                if self.apply_crop(key) {
                    Ok(UiAction::None)
                } else {
                    Err(HintKind::NoCropToApply)
                }
            }
            MultiviewCommand::Save | MultiviewCommand::SaveAs => {
                self.edit_ready(index)?;
                if !self.tile_is_dirty(index) {
                    return Err(HintKind::NothingToSave);
                }
                Ok(if command == MultiviewCommand::Save {
                    UiAction::SaveTile(key)
                } else {
                    UiAction::SaveTileAs(key)
                })
            }
            MultiviewCommand::Undo => {
                self.edit_ready(index)?;
                if self.undo_edit(key) {
                    Ok(UiAction::None)
                } else {
                    Err(HintKind::NothingToUndo)
                }
            }
            MultiviewCommand::Redo => {
                self.edit_ready(index)?;
                if self.redo_edit(key) {
                    Ok(UiAction::None)
                } else {
                    Err(HintKind::NothingToRedo)
                }
            }
        }
    }

    /// The picture of tile `index` is not being written right now.
    fn edit_ready(&self, index: usize) -> Result<(), HintKind> {
        let saving = self.tiles[index].edit.as_ref().is_some_and(|e| e.saving);
        if saving { Err(HintKind::SaveInProgress) } else { Ok(()) }
    }

    fn zoom_selected(&mut self, index: usize, zoom: ZoomKey) -> CommandResult {
        if self.zoom_key_at(index, zoom) {
            return Ok(UiAction::None);
        }
        if self.tiles[index].kind == TileKind::Video {
            // A video whose size mpv has not reported yet (no zoom model) zooms itself around the middle; one without a player refuses.
            let Some(video) = self.tiles[index].video.as_ref() else {
                return Err(HintKind::VideoNotZoomable);
            };
            let step = 1.0 + crate::viewer::zoom::prefs::current().wheel_step;
            match zoom {
                ZoomKey::Reset => video.player.reset_video_zoom(),
                ZoomKey::In => video.player.zoom_video_by(step),
                ZoomKey::Out => video.player.zoom_video_by(1.0 / step),
            }
            tracing::info!(
                tile = self.tiles[index].key,
                ?zoom,
                "[Multiview: Zoom] The video zooms itself (mpv)"
            );
            return Ok(UiAction::None);
        }
        Err(HintKind::PictureNotZoomable)
    }

    /// The key of a media bar button: the same as a click on it, if the selected tile offers it. Remove becomes Close on the
    /// last tile, as the bar shows it.
    fn bar_key(&mut self, index: usize, action: MediaActionId) -> CommandResult {
        let offers =
            |mv: &Self, wanted: MediaActionId| mv.action_views(index).iter().any(|a| a.id == wanted && a.enabled);
        let action = if action == MediaActionId::Remove && !offers(self, action) && offers(self, MediaActionId::Close) {
            MediaActionId::Close
        } else {
            action
        };
        if !offers(self, action) {
            return Err(HintKind::NotOffered);
        }
        let key = self.tiles[index].key;
        Ok(self.perform_action(key, action))
    }

    /// Esc: cancels the crop tool of the selected tile, else the one opened last (newest first). Returns whether there was
    /// one; otherwise Esc keeps its usual meaning (close the fullscreen, end a selection of tabs).
    pub fn escape_crop(&mut self) -> bool {
        if let Some(key) = self.focus_target()
            && self.cancel_crop(key)
        {
            tracing::info!(
                tile = key,
                "[ImageEdit: Crop] Esc cancels the crop tool of the selected tile"
            );
            return true;
        }
        self.cancel_newest_crop()
    }
}

#[cfg(test)]
mod tests {
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::command::{MultiviewCommand, ZoomKey};
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::hint::HintKind;
    use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance, WeakInstance};
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::route::UiAction;
    use crate::viewer::multiview::tile::TileSeed;
    use rustview_core::library::models::MediaId;
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    fn seed(media: MediaId, kind: TileKind) -> TileSeed {
        let base = TileSeed::new(
            media,
            kind,
            1.5,
            PathBuf::from(format!("/nowhere/{media}")),
            format!("m{media}"),
        );
        match kind {
            TileKind::Image => TileSeed {
                pixels: Some(SharedPixelBuffer::<Rgb8Pixel>::new(8, 6)),
                orig_size: Some((800, 600)),
                ..base
            },
            TileKind::Video => base,
        }
    }

    fn shown(seeds: Vec<TileSeed>) -> SharedInstance {
        let shared = MultiviewInstance::create(Arc::new(NullSink), seeds);
        shared.lock().set_host(&HostSurface::fullscreen(0, 0, 1920, 1080, 1.0));
        shared
    }

    #[test]
    fn several_media_without_a_selection_refuse_every_key() {
        let shared = shown(vec![seed(1, TileKind::Image), seed(2, TileKind::Image)]);
        let mut mv = shared.lock();
        for command in [
            MultiviewCommand::Rename,
            MultiviewCommand::Delete,
            MultiviewCommand::Save,
            MultiviewCommand::Zoom(ZoomKey::In),
        ] {
            assert_eq!(mv.run_command(command), Err(HintKind::NoSelection));
        }
    }

    #[test]
    fn rename_and_delete_act_on_the_selected_medium() {
        let shared = shown(vec![seed(1, TileKind::Image), seed(2, TileKind::Image)]);
        let mut mv = shared.lock();
        mv.select_tile_at(1);
        assert_eq!(mv.run_command(MultiviewCommand::Delete), Ok(UiAction::DeleteTile(2)));
        assert_eq!(mv.run_command(MultiviewCommand::Rename), Ok(UiAction::RequestRename));
        assert_eq!(mv.rename_target(), Some(2));
    }

    #[test]
    fn a_lone_medium_needs_no_selection() {
        let shared = shown(vec![seed(4, TileKind::Image)]);
        assert_eq!(
            shared.lock().run_command(MultiviewCommand::Delete),
            Ok(UiAction::DeleteTile(4))
        );
    }

    #[test]
    fn keys_that_do_not_fit_the_selected_medium_name_the_reason() {
        let shared = shown(vec![seed(1, TileKind::Image), seed(2, TileKind::Video)]);
        let mut mv = shared.lock();
        mv.select_tile_at(0);
        assert_eq!(mv.run_command(MultiviewCommand::Save), Err(HintKind::NothingToSave));
        assert_eq!(mv.run_command(MultiviewCommand::Undo), Err(HintKind::NothingToUndo));
        assert_eq!(mv.run_command(MultiviewCommand::Redo), Err(HintKind::NothingToRedo));
        assert_eq!(
            mv.run_command(MultiviewCommand::ApplyCrop),
            Err(HintKind::NoCropToApply)
        );
        mv.select_tile_at(1);
        assert_eq!(
            mv.run_command(MultiviewCommand::Zoom(ZoomKey::Reset)),
            Err(HintKind::VideoNotZoomable)
        );
    }

    #[test]
    fn esc_without_a_crop_tool_cancels_nothing() {
        let shared = shown(vec![seed(1, TileKind::Image), seed(2, TileKind::Image)]);
        let mut mv = shared.lock();
        mv.select_tile_at(0);
        assert!(!mv.escape_crop());
    }
}
