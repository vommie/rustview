//! What editing shows and what the application needs from it: the edit buttons of the bar, the crop overlay of a tile, the
//! pictures with unsaved edits (for the question before closing), and the hand-over of a picture to save and its result.

use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::media_actions::EditBarState;
use super::view::CropView;
use crate::viewer::edit::claims;
use crate::viewer::edit::prefs as edit_prefs;
use crate::viewer::edit::save_badge::SaveBadge;
use crate::viewer::edit::tile_edit::EditSource;
use crate::viewer::edit::unsaved::UnsavedEntry;
use rustview_core::image_edit::conflict::FileIdentity;
use rustview_core::image_edit::formats::SaveSupport;
use rustview_core::image_edit::options::CropGrid;
use rustview_core::image_edit::stack::EditPlan;
use rustview_core::library::models::MediaId;
use std::path::PathBuf;
use std::time::Instant;

/// Everything the application needs to save the edits of one tile.
#[derive(Debug, Clone)]
pub struct SaveRequest {
    pub tile: TileKey,
    pub media_id: MediaId,
    pub path: PathBuf,
    pub filename: String,
    pub source: EditSource,
    pub plan: EditPlan,
}

impl MultiviewInstance {
    /// The edit buttons of tile `index`, for a picture that is shown.
    pub(super) fn edit_bar_state(&self, index: usize) -> Option<EditBarState> {
        if !self.edit_host_is_interactive() || !self.is_edit_candidate(index) {
            return None;
        }
        let tile = &self.tiles[index];
        let source = tile.source.as_ref()?;
        let saving = tile.edit.as_ref().is_some_and(|e| e.saving);
        let crop = match source.support() {
            Err(refusal) => Err(refusal.text()),
            Ok(_) if saving => Err("Crop (disabled: the picture is being saved)"),
            Ok(_) => match claims::owner(tile.media_id) {
                Some(owner) if owner != tile.key => {
                    Err("Crop (disabled: this picture is being edited in another view; save or discard it there first)")
                }
                _ => Ok(tile.edit.as_ref().is_some_and(|e| e.tool.is_some())),
            },
        };
        let (can_undo, can_redo) = tile
            .edit
            .as_ref()
            .filter(|_| !saving)
            .map_or((false, false), |e| (e.stack.can_undo(), e.stack.can_redo()));
        Some(EditBarState {
            crop,
            save: tile.save_badge.look(Instant::now()),
            can_undo,
            can_redo,
        })
    }

    /// The crop overlay of tile `index`, while its tool is open.
    pub(super) fn crop_view(&self, index: usize) -> Option<CropView> {
        let tile = self.tiles.get(index)?;
        let tool = tile.edit.as_ref()?.tool.as_ref()?;
        let picture = self.picture_rect(index)?;
        let content = self.host_content(index)?;
        // The overlay covers the room of the picture (a zoomed picture is drawn there), else the tile.
        let clip = match tile.zoom.as_ref().and_then(|z| z.home().zip(z.room())) {
            Some((home, (w, h))) => Rect::new(-home.x, -home.y, w, h),
            None => Rect::new(0, 0, content.w, content.h),
        };
        let (iw, ih) = tool.image();
        let (sx, sy) = (picture.w / f64::from(iw.max(1)), picture.h / f64::from(ih.max(1)));
        let (ox, oy) = (picture.x - f64::from(clip.x), picture.y - f64::from(clip.y));
        let rect = tool.rect();
        let prefs = edit_prefs::current();
        let grid = match prefs.grid {
            CropGrid::Always => true,
            CropGrid::Dragging => tool.is_dragging(),
            CropGrid::Never => false,
        };
        Some(CropView {
            clip,
            picture: (ox, oy, picture.w, picture.h),
            rect: (
                ox + f64::from(rect.x) * sx,
                oy + f64::from(rect.y) * sy,
                f64::from(rect.width) * sx,
                f64::from(rect.height) * sy,
            ),
            size_label: tool.size_label(),
            ratio_label: tool.ratio_label(&prefs.ratios).to_string(),
            grid,
            dragging: tool.is_dragging(),
        })
    }

    /// The pictures of this Multiview with unsaved edits (checked; the dialog lets the user decide per picture).
    pub fn unsaved_entries(&self) -> Vec<UnsavedEntry> {
        self.tiles
            .iter()
            .filter_map(|tile| {
                let edit = tile.edit.as_ref().filter(|e| e.stack.is_dirty())?;
                let overwritable = matches!(
                    tile.source.as_ref().map(EditSource::support),
                    Some(Ok(SaveSupport::InPlace(_)))
                );
                Some(UnsavedEntry {
                    tile: tile.key,
                    media_id: tile.media_id,
                    filename: tile.filename.clone(),
                    folder: tile.path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
                    description: edit.stack.describe(),
                    overwritable,
                    save: true,
                })
            })
            .collect()
    }

    /// The unsaved pictures among the tiles `keys` (a tile that leaves the view).
    pub fn unsaved_entries_of(&self, keys: &[TileKey]) -> Vec<UnsavedEntry> {
        self.unsaved_entries()
            .into_iter()
            .filter(|e| keys.contains(&e.tile))
            .collect()
    }

    /// What saving tile `key` needs; `None` when it has nothing to save or is being saved.
    pub fn save_request(&self, key: TileKey) -> Option<SaveRequest> {
        let index = self.tile_index(key)?;
        let tile = &self.tiles[index];
        let edit = tile.edit.as_ref().filter(|e| e.stack.is_dirty() && !e.saving)?;
        Some(SaveRequest {
            tile: key,
            media_id: tile.media_id,
            path: tile.path.clone(),
            filename: tile.filename.clone(),
            source: tile.source.clone()?,
            plan: edit.stack.plan(),
        })
    }

    /// The tile under the pointer if it has unsaved edits, else the only tile that has some (Ctrl+S).
    pub fn save_target(&self) -> Option<TileKey> {
        let dirty = |i: usize| self.tile_is_dirty(i);
        if let Some(index) = self.hovered.filter(|i| dirty(*i)) {
            return Some(self.tiles[index].key);
        }
        let mut keys = (0..self.tiles.len()).filter(|i| dirty(*i)).map(|i| self.tiles[i].key);
        match (keys.next(), keys.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    }

    /// The tile under the pointer if it has something to undo or redo, else the only such tile (Ctrl+Z, Ctrl+Shift+Z).
    pub fn undo_target(&self) -> Option<TileKey> {
        let editable = |i: usize| {
            self.tiles
                .get(i)
                .and_then(|t| t.edit.as_ref())
                .is_some_and(|e| !e.saving)
        };
        if let Some(index) = self.hovered.filter(|i| editable(*i)) {
            return Some(self.tiles[index].key);
        }
        let mut keys = (0..self.tiles.len())
            .filter(|i| editable(*i))
            .map(|i| self.tiles[i].key);
        match (keys.next(), keys.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    }

    /// The file of tile `key` is being written (`true`) or the attempt ended without writing (`false`).
    pub fn set_saving(&mut self, key: TileKey, saving: bool) {
        let Some(index) = self.tile_index(key) else { return };
        let tile = &mut self.tiles[index];
        let Some(edit) = tile.edit.as_mut() else { return };
        edit.saving = saving;
        crate::viewer::edit::touch_unsaved();
        if saving {
            if edit.tool.take().is_some() {
                self.crop_order.closed(key);
            }
            tile.save_badge = SaveBadge::Saving;
        } else {
            tile.save_badge = tile.save_badge.after_edit(edit.stack.is_dirty());
        }
        self.bump();
    }

    /// The edits of tile `key` were written to its file (which now has `identity` and the size of the edited picture): the
    /// edits are done, the Save button shows the success and fades away, and the tile keeps showing the picture it shows
    /// (which is what the file holds now) until the refreshed file is decoded.
    pub fn save_finished(&mut self, key: TileKey, identity: FileIdentity, size: (u32, u32)) {
        let Some(index) = self.tile_index(key) else { return };
        let tile = &mut self.tiles[index];
        if let Some(source) = tile.source.as_mut() {
            source.identity = Some(identity);
            source.size = size;
        }
        tile.edit = None;
        tile.save_badge = SaveBadge::Saved(Instant::now());
        crate::viewer::edit::touch_unsaved();
        self.bump();
    }

    /// The edits of tile `key` are done without changing its file (saved as a copy elsewhere, or the user chose to drop them
    /// and see the file again).
    pub fn finish_edits_without_save(&mut self, key: TileKey) {
        let Some(index) = self.tile_index(key) else { return };
        if self.tiles[index].edit.as_ref().is_some_and(|e| e.tool.is_some()) {
            self.crop_order.closed(key);
        }
        let tile = &mut self.tiles[index];
        let Some(edit) = tile.edit.as_mut() else { return };
        edit.saving = false;
        edit.tool = None;
        edit.stack.clear();
        self.edits_changed(index);
    }

    /// The next moment an edit button changes by itself (the Save button fading after a save).
    pub fn edit_wakeup(&self, now: Instant) -> Option<Instant> {
        self.tiles.iter().filter_map(|t| t.save_badge.wakeup(now)).min()
    }

    /// Lets the Save buttons that are done go away; returns whether something changed.
    pub fn edit_tick(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for tile in &mut self.tiles {
            if matches!(tile.save_badge, SaveBadge::Saved(_)) {
                // Every wakeup is a change of the look: the fade starts, or the button goes.
                if tile.save_badge.look(now).is_none() {
                    tile.save_badge = SaveBadge::Hidden;
                }
                changed = true;
            }
        }
        if changed {
            self.bump();
        }
        changed
    }
}
