//! The time badges of the videos of one Multiview in the tab grid as rows of a Slint model (UI thread only): its grid item's
//! canvas draws a badge per row. A row belongs to its tile and is updated in place, only when its text or place changed.

use crate::ui::MultiviewTimeBadgeView;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance_grid_bar::TimeBadgeTarget;
use rustview_core::settings::schema::{MULTIVIEW_TIME_BADGE_FONT_LOGICAL_PX, MULTIVIEW_TIME_BADGE_MARGIN_LOGICAL_PX};
use rustview_core::util::cast::{Az, WrappingAs};
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;

/// The badges one canvas draws.
#[derive(Default)]
pub struct TimeBadgeModel {
    rows: Rc<VecModel<MultiviewTimeBadgeView>>,
    keys: Vec<TileKey>,
}

impl TimeBadgeModel {
    /// The model to hand to the canvas (once; it stays the same).
    pub fn model(&self) -> ModelRc<MultiviewTimeBadgeView> {
        ModelRc::from(self.rows.clone())
    }

    /// Shows the badges of `targets` (and only those); `dpi` = physical pixels per logical pixel of the host.
    pub fn sync(&mut self, targets: &[TimeBadgeTarget], dpi: f32) {
        if targets.iter().map(|t| t.key).ne(self.keys.iter().copied()) {
            self.keys = targets.iter().map(|t| t.key).collect();
            self.rows
                .set_vec(targets.iter().map(|t| row(t, dpi)).collect::<Vec<_>>());
            return;
        }
        for (index, target) in targets.iter().enumerate() {
            let row = row(target, dpi);
            if self.rows.row_data(index).as_ref() != Some(&row) {
                self.rows.set_row_data(index, row);
            }
        }
    }

    /// No badge shows any more (the grid went, or the Multiview left it).
    pub fn clear(&mut self) {
        if !self.keys.is_empty() {
            self.keys.clear();
            self.rows.set_vec(Vec::new());
        }
    }
}

fn row(target: &TimeBadgeTarget, dpi: f32) -> MultiviewTimeBadgeView {
    let dpi = dpi.max(0.01);
    let logical = |px: i32| px.az::<f32>() / dpi;
    MultiviewTimeBadgeView {
        key: target.key.wrapping_as::<i32>(),
        x: logical(target.rect.x),
        y: logical(target.rect.y),
        width: logical(target.rect.w),
        height: logical(target.rect.h),
        text: target.text.as_str().into(),
        font_size: MULTIVIEW_TIME_BADGE_FONT_LOGICAL_PX,
        margin: MULTIVIEW_TIME_BADGE_MARGIN_LOGICAL_PX,
    }
}
