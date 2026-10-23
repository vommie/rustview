//! Instance behavior around editing pictures: opening and cancelling the crop tool of a tile, the pointer on it (mapped through
//! the zoom of the tile, so the tool works the same at every zoom), applying a crop, undo, redo and discarding, and what the
//! tile shows meanwhile. Saving is the application's job (`ui::image_edit`); the instance only tells it what to save and
//! hears back when it is done.

use super::events::{DecodedImage, PlayerEvent};
use super::follow::TileKey;
use super::image_decode;
use super::instance::MultiviewInstance;
use super::model::{TileKind, aspect_differs};
use super::route::{HostMode, UiAction};
use crate::viewer::edit::claims::{self, EditClaim};
use crate::viewer::edit::prefs as edit_prefs;
use crate::viewer::edit::tile_edit::{TileEdit, aspect_of, crop_display};
use crate::viewer::zoom::geometry::RectF;
use crate::viewer::zoom::state::ZoomState;
use rustview_core::image_edit::crop_tool::CropTool;
use rustview_core::image_edit::stack::EditOp;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;

/// A crop handle reaches this far from its center on screen (logical pixels).
pub const HANDLE_REACH: f64 = 10.0;

/// What the pointer did on the crop overlay of a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropPhase {
    Down,
    Move,
    Up,
    /// A double click applies the crop.
    Double,
}

impl CropPhase {
    pub fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Down,
            1 => Self::Move,
            2 => Self::Up,
            3 => Self::Double,
            _ => return None,
        })
    }
}

impl MultiviewInstance {
    pub(super) fn edit_host_is_interactive(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
    }

    /// A shown, decoded picture whose file was examined.
    pub(super) fn is_edit_candidate(&self, index: usize) -> bool {
        self.tiles.get(index).is_some_and(|t| {
            t.kind == TileKind::Image && t.error.is_none() && !t.is_loading && t.pixels.is_some() && t.source.is_some()
        })
    }

    /// The crop button of tile `key` was clicked: opens the tool, or cancels it when it is open.
    pub fn toggle_crop(&mut self, key: TileKey) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        if self.tiles[index].edit.as_ref().is_some_and(|e| e.tool.is_some()) {
            return self.cancel_crop(key);
        }
        self.open_crop(index)
    }

    fn open_crop(&mut self, index: usize) -> bool {
        if !self.edit_host_is_interactive() || !self.is_edit_candidate(index) {
            return false;
        }
        let prefs = edit_prefs::current();
        let tile = &mut self.tiles[index];
        let Some(source) = tile.source.as_ref() else {
            return false;
        };
        if source.support().is_err() || tile.edit.as_ref().is_some_and(|e| e.saving) {
            return false;
        }
        if tile.edit.is_none() {
            let Some(claim) = EditClaim::take(tile.media_id, tile.key) else {
                tracing::info!(
                    media_id = tile.media_id,
                    "[ImageEdit: Crop] The picture is edited in another tile"
                );
                return false;
            };
            let Some(base) = tile.pixels.clone() else { return false };
            tile.edit = Some(TileEdit::new(claim, source.size, base));
        }
        let grid = source.crop_grid(prefs.align_jpeg);
        let Some(edit) = tile.edit.as_mut() else { return false };
        let mut tool = CropTool::new(edit.stack.current_size(), edit.stack.original_size(), grid);
        tool.apply_choice(&prefs.ratios);
        edit.tool = Some(tool);
        self.crop_order.opened(tile.key);
        tracing::info!(
            media_id = tile.media_id,
            filename = %LogObscurer::filename(&tile.filename),
            grid = ?grid,
            "[ImageEdit: Crop] Crop tool opened"
        );
        self.bump();
        true
    }

    /// Closes the crop tool of tile `key` without applying it.
    pub fn cancel_crop(&mut self, key: TileKey) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        let Some(edit) = self.tiles[index].edit.as_mut() else {
            return false;
        };
        if edit.tool.take().is_none() {
            return false;
        }
        self.crop_order.closed(key);
        tracing::info!(
            media_id = self.tiles[index].media_id,
            "[ImageEdit: Crop] Crop tool cancelled"
        );
        self.drop_idle_edit(index);
        self.bump();
        true
    }

    /// Esc: cancels the crop tool that was opened first. Returns whether there was one (then Esc means nothing else).
    pub fn cancel_oldest_crop(&mut self) -> bool {
        self.prune_crop_order();
        match self.crop_order.oldest() {
            Some(key) => {
                if !self.cancel_crop(key) {
                    self.crop_order.closed(key);
                }
                true
            }
            None => false,
        }
    }

    pub fn has_open_crop(&self) -> bool {
        self.tiles
            .iter()
            .any(|t| t.edit.as_ref().is_some_and(|e| e.tool.is_some()))
    }

    fn prune_crop_order(&mut self) {
        let open: Vec<TileKey> = self
            .tiles
            .iter()
            .filter(|t| t.edit.as_ref().is_some_and(|e| e.tool.is_some()))
            .map(|t| t.key)
            .collect();
        self.crop_order.retain(|key| open.contains(&key));
    }

    /// An edit with nothing left to do (no tool, no unsaved changes, no save running) goes away and frees the medium.
    pub(super) fn drop_idle_edit(&mut self, index: usize) {
        let tile = &mut self.tiles[index];
        if tile.edit.as_ref().is_some_and(|e| !e.is_live()) {
            tile.edit = None;
        }
    }

    /// The picture of tile `index` in the coordinates of the tile (physical pixels relative to its content rectangle): its
    /// place in the room when it is zoomed, else its fitted place.
    pub(super) fn picture_rect(&self, index: usize) -> Option<RectF> {
        let tile = self.tiles.get(index)?;
        if let Some(zoom) = tile.zoom.as_ref()
            && let (Some(rect), Some(home)) = (zoom.image_rect(), zoom.home())
        {
            return Some(RectF::new(
                rect.x - f64::from(home.x),
                rect.y - f64::from(home.y),
                rect.w,
                rect.h,
            ));
        }
        let content = self.host_content(index)?;
        let (w, h) = tile.edit.as_ref().map_or_else(
            || tile.zoom.as_ref().map_or((1, 1), ZoomState::image),
            |e| e.stack.current_size(),
        );
        let scale = (f64::from(content.w) / f64::from(w.max(1))).min(f64::from(content.h) / f64::from(h.max(1)));
        let (pw, ph) = (f64::from(w) * scale, f64::from(h) * scale);
        Some(RectF::new(
            (f64::from(content.w) - pw) / 2.0,
            (f64::from(content.h) - ph) / 2.0,
            pw,
            ph,
        ))
    }

    /// The pointer on the crop overlay of tile `index` at (`x`, `y`) (physical pixels relative to the tile). Returns whether
    /// anything changed.
    pub fn crop_pointer(&mut self, index: usize, phase: CropPhase, x: f64, y: f64, shift: bool) -> bool {
        if phase == CropPhase::Double {
            let Some(key) = self.tiles.get(index).map(|t| t.key) else {
                return false;
            };
            return self.apply_crop(key);
        }
        let Some(picture) = self.picture_rect(index) else {
            return false;
        };
        let dpi = f64::from(self.host.surface.dpi);
        let prefs = edit_prefs::current();
        let Some(tool) = self.tiles[index].edit.as_mut().and_then(|e| e.tool.as_mut()) else {
            return false;
        };
        let (iw, ih) = tool.image();
        let scale = picture.w / f64::from(iw.max(1));
        if scale <= 0.0 || !scale.is_finite() {
            return false;
        }
        let ix = (x - picture.x) / scale;
        let iy = (y - picture.y) / (picture.h / f64::from(ih.max(1)));
        let changed = match phase {
            CropPhase::Down => {
                let hit = tool.hit_test(ix, iy, HANDLE_REACH * dpi / scale);
                tool.press(ix, iy, hit);
                true
            }
            CropPhase::Move => tool.is_dragging() && tool.drag_to(ix, iy, shift, &prefs.ratios),
            CropPhase::Up => {
                let was = tool.is_dragging();
                tool.release();
                was
            }
            CropPhase::Double => false,
        };
        if changed {
            self.bump();
        }
        changed
    }

    /// The ratio button of the crop overlay of tile `index`: left click = next landscape ratio, right click = next portrait.
    pub fn crop_ratio(&mut self, index: usize, landscape: bool) {
        let prefs = edit_prefs::current();
        if let Some(tool) = self
            .tiles
            .get_mut(index)
            .and_then(|t| t.edit.as_mut())
            .and_then(|e| e.tool.as_mut())
        {
            tool.cycle_ratio(&prefs.ratios, landscape);
            self.bump();
        }
    }

    /// Applies the crop rectangle of tile `key` (double click, Enter): the picture is cropped in memory, nothing is written.
    pub fn apply_crop(&mut self, key: TileKey) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        let Some(edit) = self.tiles[index].edit.as_mut() else {
            return false;
        };
        let Some(tool) = edit.tool.take() else { return false };
        self.crop_order.closed(key);
        let rect = tool.rect();
        let pushed = edit.stack.push(EditOp::Crop(rect));
        tracing::info!(
            media_id = self.tiles[index].media_id,
            x = rect.x,
            y = rect.y,
            width = rect.width,
            height = rect.height,
            applied = pushed,
            "[ImageEdit: Crop] Crop applied"
        );
        if pushed {
            self.edits_changed(index);
        } else {
            self.drop_idle_edit(index);
            self.bump();
        }
        true
    }

    /// Enter: applies the crop of the tile under the pointer, else the one opened first.
    pub fn apply_crop_target(&mut self) -> bool {
        let hovered = self
            .hovered
            .and_then(|i| self.tiles.get(i))
            .filter(|t| t.edit.as_ref().is_some_and(|e| e.tool.is_some()))
            .map(|t| t.key);
        self.prune_crop_order();
        match hovered.or_else(|| self.crop_order.oldest()) {
            Some(key) => self.apply_crop(key),
            None => false,
        }
    }

    pub fn undo_edit(&mut self, key: TileKey) -> bool {
        self.change_stack(key, |edit| edit.stack.undo().is_some(), "Undo")
    }

    pub fn redo_edit(&mut self, key: TileKey) -> bool {
        self.change_stack(key, |edit| edit.stack.redo().is_some(), "Redo")
    }

    /// Drops every unsaved edit of tile `key` (and its open tool): the tile shows the file again.
    pub fn discard_edits(&mut self, key: TileKey) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        if self.tiles[index].edit.as_ref().is_some_and(|e| e.tool.is_some()) {
            self.crop_order.closed(key);
        }
        self.change_stack(
            key,
            |edit| {
                edit.tool = None;
                edit.stack.clear();
                true
            },
            "Discard",
        )
    }

    fn change_stack(&mut self, key: TileKey, change: impl FnOnce(&mut TileEdit) -> bool, what: &str) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        let Some(edit) = self.tiles[index].edit.as_mut().filter(|e| !e.saving) else {
            return false;
        };
        if !change(edit) {
            return false;
        }
        tracing::info!(
            media_id = self.tiles[index].media_id,
            what,
            "[ImageEdit: Edits] Changed"
        );
        self.edits_changed(index);
        true
    }

    /// The edits of tile `index` changed: the tile shows the edited picture at once (cut out of the picture it had), a sharp
    /// one is decoded from the original, the zoom starts over for the new picture and the layout follows a new shape.
    pub(super) fn edits_changed(&mut self, index: usize) {
        let tile = &mut self.tiles[index];
        let Some(edit) = tile.edit.as_mut() else { return };
        edit.revision += 1;
        crate::viewer::edit::touch_unsaved();
        let plan = edit.stack.plan();
        let original = edit.stack.original_size();
        let dirty = edit.stack.is_dirty();
        tile.save_badge = tile.save_badge.after_edit(dirty);
        tile.pixels = Some(match plan.crop {
            Some(crop) => crop_display(&edit.base_pixels, original, crop),
            None => edit.base_pixels.clone(),
        });
        tile.zoom = Some(ZoomState::new(plan.output_size));
        tile.zoom_minimap_shown = false;
        self.zoom_slider.retain(|k| *k != tile.key);
        self.zoom_minimap_bubble.retain(|k| *k != tile.key);
        let aspect = aspect_of(plan.output_size);
        let relayout = aspect_differs(aspect, tile.aspect);
        tile.aspect = aspect;
        let preview = plan.crop.map(|crop| (tile.key, tile.path.clone(), crop, edit.revision));
        self.drop_idle_edit(index);
        if let Some((key, path, crop, revision)) = preview {
            self.start_edit_preview(key, path, crop, revision);
        }
        if relayout {
            self.apply_layout();
        } else {
            self.sync_zoom_frames();
            self.bump();
        }
    }

    /// Decodes the edited state of a tile from the original, sharp at the size the view needs.
    fn start_edit_preview(
        &self,
        key: TileKey,
        path: std::path::PathBuf,
        crop: rustview_core::image_edit::geometry::PixelRect,
        revision: u64,
    ) {
        let area = self.host.surface.rect;
        let (max_w, max_h) = (
            self.container.width.max(area.w).max(1).saturating_as::<u32>(),
            self.container.height.max(area.h).max(1).saturating_as::<u32>(),
        );
        let (sink, me) = (self.sink.clone(), self.me.clone());
        let spawned = std::thread::Builder::new().name("edit-preview".into()).spawn(move || {
            let result = image_decode::load_scaled_cropped(&path, Some(crop), max_w, max_h);
            sink.post(
                me,
                PlayerEvent::EditPreview {
                    tile: key,
                    revision,
                    result,
                },
            );
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[ImageEdit: Preview] The decoder thread could not be started");
        }
    }

    /// The sharp picture of an edited state arrived; it replaces the cut-out one if the edits are still the same.
    pub fn on_edit_preview(&mut self, key: TileKey, revision: u64, result: DecodedImage) {
        let Some(tile) = self.tiles.iter_mut().find(|t| t.key == key) else {
            return;
        };
        let current = tile.edit.as_ref().map(|e| (e.revision, e.stack.current_size()));
        match (result, current) {
            (Ok((w, h, pixels)), Some((rev, size))) if rev == revision && (w, h) == size => {
                tile.pixels = Some(pixels);
                self.bump();
            }
            (Err(e), _) => {
                tracing::warn!(
                    error = %if LogObscurer::is_active() { "<obfuscated>" } else { e.as_str() },
                    "[ImageEdit: Preview] The edited picture could not be decoded; the cut-out picture stays"
                );
            }
            _ => {}
        }
    }

    /// Whether tile `index` shows a picture whose edits are not saved.
    pub fn tile_is_dirty(&self, index: usize) -> bool {
        self.tiles
            .get(index)
            .and_then(|t| t.edit.as_ref())
            .is_some_and(|e| e.stack.is_dirty())
    }

    /// Whether any tile has unsaved edits or a save in progress (the tab bar marks such a tab).
    pub fn has_unsaved_edits(&self) -> bool {
        self.tiles
            .iter()
            .any(|t| t.edit.as_ref().is_some_and(|e| e.stack.is_dirty() || e.saving))
    }

    /// The medium is edited by a tile of this Multiview.
    pub fn edits_medium(&self, media_id: rustview_core::library::models::MediaId) -> bool {
        claims::owner(media_id).is_some_and(|key| self.tile_index(key).is_some())
    }

    /// The pointer on the crop overlay as Slint reports it (logical pixels relative to the tile, the phase as a code).
    pub fn crop_pointer_logical(&mut self, index: usize, phase: i32, x: f32, y: f32, shift: bool) -> bool {
        let Some(phase) = CropPhase::from_code(phase) else {
            return false;
        };
        let dpi = f64::from(self.host.surface.dpi);
        self.crop_pointer(index, phase, f64::from(x) * dpi, f64::from(y) * dpi, shift)
    }

    /// A key of the image editor (0 = Esc, 1 = Enter, 2 = Ctrl+S, 3 = Ctrl+Shift+S, 4 = Ctrl+Z, 5 = Ctrl+Shift+Z / Ctrl+Y).
    /// Returns whether the editor used it (otherwise the key keeps its usual meaning) and what the application has to do.
    pub fn edit_key(&mut self, code: i32) -> (bool, UiAction) {
        if !self.edit_host_is_interactive() {
            return (false, UiAction::None);
        }
        match code {
            0 => (self.cancel_oldest_crop(), UiAction::None),
            1 => (self.apply_crop_target(), UiAction::None),
            2 => match self.save_target() {
                Some(key) => (true, UiAction::SaveTile(key)),
                None => (false, UiAction::None),
            },
            3 => match self.save_target() {
                Some(key) => (true, UiAction::SaveTileAs(key)),
                None => (false, UiAction::None),
            },
            4 => (
                self.undo_target().is_some_and(|key| self.undo_edit(key)),
                UiAction::None,
            ),
            5 => (
                self.undo_target().is_some_and(|key| self.redo_edit(key)),
                UiAction::None,
            ),
            _ => (false, UiAction::None),
        }
    }
}
