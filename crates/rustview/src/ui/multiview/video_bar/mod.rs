//! The control bars of the videos as rows of a Slint model (UI thread only): every host (the fullscreen window, the tab view, each
//! Multiview of the tab grid) has one `VideoBarModel`, and its canvas draws a `MultiviewVideoBar` per row over the video. What a bar
//! shows, where and when is decided by the instance (`viewer::multiview::video_bar`); this module turns its targets into rows.
//!
//! A row belongs to its tile (`TileKey`) for as long as the bar shows: rows are updated in place, never rebuilt, so a drag on the
//! progress bar, an open popup and the text typed into it survive every tick. The bar with an open A-B popup is the last row, so
//! no other bar is drawn over its popup. The events of every bar of a window arrive at its `VideoBarBus` with the tile's key.

mod convert;

use crate::ui::{AbMenuRowView, AbStripSegmentView, VideoBarSpanView};
use crate::ui::{MultiviewVideoBarView, VideoBarEvent as SlintVideoBarEvent};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::video_bar::prefs::VideoBarPrefs;
use crate::viewer::multiview::video_bar::reveal::Phase;
use crate::viewer::multiview::video_bar::view::{VideoBarEvent, VideoBarInput, VideoBarTarget, VideoBarView};
use rustview_core::util::cast::WrappingAs;
use slint::{Model, ModelRc, VecModel};
use std::rc::Rc;

/// What a bar keeps between ticks: the lists inside its row and the view they were made from.
pub(super) struct Slot {
    key: TileKey,
    spans: Rc<VecModel<VideoBarSpanView>>,
    ab_strip: Rc<VecModel<AbStripSegmentView>>,
    ab_rows: Rc<VecModel<AbMenuRowView>>,
    last: Option<VideoBarView>,
}

impl Slot {
    fn new(key: TileKey) -> Self {
        Self {
            key,
            spans: Rc::default(),
            ab_strip: Rc::default(),
            ab_rows: Rc::default(),
            last: None,
        }
    }
}

/// The bars one canvas draws.
#[derive(Default)]
pub struct VideoBarModel {
    rows: Rc<VecModel<MultiviewVideoBarView>>,
    /// One per row, in the same order.
    slots: Vec<Slot>,
    /// An A-B popup closed: its text field had the keyboard, and the host gives it back to its window.
    focus_back: bool,
}

impl VideoBarModel {
    /// The model to hand to the canvas (once; it stays the same).
    pub fn model(&self) -> ModelRc<MultiviewVideoBarView> {
        ModelRc::from(self.rows.clone())
    }

    /// Shows the bars of `targets` (and only those). Returns whether one of them is fully shown.
    pub fn sync(&mut self, targets: &[VideoBarTarget], prefs: &VideoBarPrefs) -> bool {
        let popup_before = self.popup_open();
        // The bar with an open popup last: it is drawn above every other bar.
        let mut ordered: Vec<&VideoBarTarget> = targets.iter().collect();
        ordered.sort_by_key(|target| target.view.ab.as_ref().is_some_and(|ab| ab.popup_size().is_some()));
        for (index, target) in ordered.iter().enumerate() {
            match self.slots.iter().position(|slot| slot.key == target.key) {
                Some(found) if found == index => {}
                Some(found) => {
                    // A row moves by being made again (only when a popup opens or closes, so nothing typed is lost).
                    let slot = self.remove(found);
                    self.insert(index, slot, target, prefs);
                    continue;
                }
                None => {
                    self.insert(index, Slot::new(target.key), target, prefs);
                    tracing::debug!(tile = target.key, "[Multiview: VideoBar] Bar shown");
                    continue;
                }
            }
            let row = convert::row(target, prefs, &mut self.slots[index]);
            if self.rows.row_data(index).as_ref() != Some(&row) {
                self.rows.set_row_data(index, row);
            }
        }
        while self.slots.len() > ordered.len() {
            let slot = self.remove(self.slots.len() - 1);
            tracing::debug!(tile = slot.key, "[Multiview: VideoBar] Bar hidden");
        }
        if popup_before && !self.popup_open() {
            tracing::debug!("[Multiview: VideoBar] A-B popup closed");
            self.focus_back = true;
        }
        targets.iter().any(|target| target.phase == Phase::Shown)
    }

    /// No bar shows any more (the host closes, hides or is locked).
    pub fn clear(&mut self) {
        if !self.slots.is_empty() {
            tracing::debug!(bars = self.slots.len(), "[Multiview: VideoBar] Bars hidden");
        }
        self.focus_back |= self.popup_open();
        self.slots.clear();
        self.rows.set_vec(Vec::new());
    }

    /// The tile whose bar reported `key` (the 32-bit key of the canvas), if it is one of these bars.
    pub fn tile(&self, key: i32) -> Option<TileKey> {
        self.slots
            .iter()
            .map(|slot| slot.key)
            .find(|tile| (*tile).wrapping_as::<i32>() == key)
    }

    /// Whether the keyboard has to go back to the window (once after a popup closed).
    pub fn take_focus_back(&mut self) -> bool {
        std::mem::take(&mut self.focus_back)
    }

    /// Whether an A-B popup of one of these bars is open.
    pub fn popup_open(&self) -> bool {
        self.slots.iter().any(|slot| {
            slot.last
                .as_ref()
                .and_then(|view| view.ab.as_ref())
                .is_some_and(|ab| ab.popup_size().is_some())
        })
    }

    fn insert(&mut self, index: usize, mut slot: Slot, target: &VideoBarTarget, prefs: &VideoBarPrefs) {
        slot.last = None;
        let row = convert::row(target, prefs, &mut slot);
        self.rows.insert(index, row);
        self.slots.insert(index, slot);
    }

    fn remove(&mut self, index: usize) -> Slot {
        self.rows.remove(index);
        self.slots.remove(index)
    }
}

/// What the canvas reported (`VideoBarBus.event`), as an input of the bar (`None`: an unknown code).
pub fn input_of(event: SlintVideoBarEvent) -> Option<VideoBarInput> {
    let decoded = VideoBarInput::from_event(VideoBarEvent {
        kind: event.kind,
        share: event.share,
        phase: event.phase,
        index: event.index,
        text: event.text.into(),
        delta: event.delta,
    });
    if decoded.is_none() {
        tracing::warn!(kind = event.kind, "[Multiview: VideoBar] Unknown event of a bar");
    }
    decoded
}
