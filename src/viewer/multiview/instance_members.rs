//! Instance behavior around library events: a member was renamed, deleted or refreshed. The application tells the
//! instance from the records it already holds in memory; the drive is never touched.

use super::audio::{self, TileAudio};
use super::instance::MultiviewInstance;
use super::model::{AspectSource, aspect_differs, aspect_from_size};
use super::teardown::teardown_videos;
use crate::library::models::{MediaId, MediaRecord};
use crate::settings::LogObscurer;

impl MultiviewInstance {
    pub fn member_renamed(&mut self, id: MediaId, new_filename: &str) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == id) else {
            return;
        };
        if let Some(parent) = tile.path.parent() {
            tile.path = parent.join(new_filename);
        }
        tile.filename = new_filename.to_string();
        tracing::debug!(
            media_id = id,
            filename = %LogObscurer::filename(new_filename),
            "[Multiview: Members] Member renamed"
        );
    }

    /// The file of a member is gone: its player is shut down, its tile shows an error and the sound moves on if it
    /// carried it.
    pub fn member_deleted(&mut self, id: MediaId) {
        let Some(index) = self.tiles.iter().position(|t| t.media_id == id) else {
            return;
        };
        let video = {
            let tile = &mut self.tiles[index];
            tile.error = Some(format!(
                "Media file not found on disk:\n{}",
                tile.path.display()
            ));
            tile.is_loading = false;
            tile.pixels = None;
            tile.audio = None;
            tile.video.take()
        };
        teardown_videos(video.into_iter().collect());
        self.clear_hover();
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::after_reconcile(&mut slots, &[]);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }
        self.apply_audio();
        self.bump();
        tracing::info!(
            media_id = id,
            "[Multiview: Members] Member deleted; its tile shows an error"
        );
    }

    /// A member's record changed (refresh): keeps the name and, while no player or decoder has measured the real size
    /// yet, the aspect ratio in line with the record.
    pub fn member_refreshed(&mut self, id: MediaId, record: &MediaRecord) {
        if record.is_deleted {
            self.member_deleted(id);
            return;
        }
        let Some(tile) = self.tiles.iter_mut().find(|t| t.media_id == id) else {
            return;
        };
        tile.filename = record.filename.clone();
        let mut relayout = false;
        if tile.aspect_source == AspectSource::Library {
            let aspect = aspect_from_size(record.width, record.height, tile.aspect);
            if aspect_differs(aspect, tile.aspect) {
                tile.aspect = aspect;
                relayout = true;
            }
        }
        if relayout {
            self.apply_layout();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::model::TileKind;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    fn instance(kinds: &[TileKind]) -> SharedInstance {
        let seeds = kinds
            .iter()
            .enumerate()
            .map(|(i, &kind)| TileSeed {
                media_id: i as u64 + 1,
                kind,
                aspect: 1.78,
                path: PathBuf::from(format!("/media/{i}.mp4")),
                filename: format!("{i}.mp4"),
            })
            .collect();
        let shared = MultiviewInstance::detached(seeds);
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn record(id: MediaId, width: u32, height: u32, is_deleted: bool) -> MediaRecord {
        use crate::media::{MediaFormat, MediaType};
        MediaRecord {
            id,
            folder_id: 1,
            filename: format!("refreshed-{id}.mp4"),
            file_hash: [0; 16],
            file_size: 1,
            modification_time: 0,
            media_type: MediaType::Video,
            format: MediaFormat::Mp4,
            width,
            height,
            rating: 0,
            last_seen: 0,
            is_deleted,
            creation_time: 0,
            duration_ms: 0,
            is_corrupted: false,
            source: None,
        }
    }

    #[test]
    fn a_rename_updates_the_name_and_the_path() {
        let shared = instance(&[TileKind::Image, TileKind::Image]);
        let mut mv = shared.lock();
        mv.member_renamed(2, "new.png");
        assert_eq!(mv.tiles[1].filename, "new.png");
        assert_eq!(mv.tiles[1].path, PathBuf::from("/media/new.png"));
        assert_eq!(mv.tiles[0].filename, "0.mp4", "other tiles are untouched");
        mv.member_renamed(99, "unknown.png");
    }

    #[test]
    fn a_deleted_member_keeps_its_slot_and_hands_the_sound_on() {
        let shared = instance(&[TileKind::Video, TileKind::Video, TileKind::Image]);
        let mut mv = shared.lock();
        mv.tiles[0].audio = Some(TileAudio::native());
        mv.tiles[1].audio = Some(TileAudio::muted_by_policy());
        let before = mv.revision();

        mv.member_deleted(1);

        assert_eq!(mv.member_ids(), vec![1, 2, 3]);
        assert!(mv.tiles[0].error.is_some());
        assert_eq!(mv.tiles[0].audio, None);
        assert_eq!(
            mv.tiles[1].audio,
            Some(TileAudio::native()),
            "the next video takes over the sound"
        );
        assert!(mv.tiles[2].error.is_none());
        assert_ne!(mv.revision(), before);
    }

    #[test]
    fn a_refresh_corrects_a_library_aspect_but_not_a_measured_one() {
        let shared = instance(&[TileKind::Video, TileKind::Video]);
        let mut mv = shared.lock();
        mv.tiles[1].aspect_source = AspectSource::Probed;

        mv.member_refreshed(1, &record(1, 1080, 1920, false));
        assert!((mv.tiles[0].aspect - 0.5625).abs() < 1e-3);
        assert_eq!(mv.tiles[0].filename, "refreshed-1.mp4");

        mv.member_refreshed(2, &record(2, 1080, 1920, false));
        assert!(
            (mv.tiles[1].aspect - 1.78).abs() < 1e-3,
            "measured aspects stay"
        );
    }

    #[test]
    fn a_refresh_that_reports_a_deleted_file_marks_the_tile() {
        let shared = instance(&[TileKind::Video, TileKind::Video]);
        let mut mv = shared.lock();
        mv.member_refreshed(2, &record(2, 1920, 1080, true));
        assert!(mv.tiles[1].error.is_some());
    }
}
