//! The windows of the video control bars (UI thread only): one per video tile that shows its bar, at the bottom of the video.
//!
//! Like the action bars (`bar_overlay`), a bar is an override-redirect window of its own above the native mpv window, blended
//! by the compositing manager (solid and shaped without one). The windows are a pool: a tile takes one while its bar shows
//! and hands it back afterwards, so a bar follows its tile (`TileKey`) however the layout changes. What a bar shows, where
//! and when is decided by the instance (`viewer::multiview::video_bar`); this module only turns it into window calls.

use super::bar_overlay::BarOwner;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::video_bar::prefs::{self, VideoBarPrefs};
use crate::viewer::multiview::video_bar::reveal::Phase;
use crate::viewer::multiview::video_bar::view::{VideoBarInput, VideoBarTarget};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

mod window;
use window::VideoBarWindow;

/// Called with the tile and what the user did on its bar.
pub type InputHandler = Rc<dyn Fn(TileKey, VideoBarInput)>;

/// The host the bars float over.
pub struct VideoBarHost {
    pub owner: BarOwner,
    /// The window the Multiview is drawn in (the bars are placed relative to it).
    pub host_xid: u64,
    pub on_input: InputHandler,
}

/// More video tiles than this never exist at once (see `MULTIVIEW_MAX_TILES`), so the pool never grows beyond it.
const MAX_BARS: usize = 8;

thread_local! {
    static BARS: RefCell<Vec<Rc<RefCell<VideoBarWindow>>>> = const { RefCell::new(Vec::new()) };
    /// Whether a compositing manager blended the last bar shown (a bar fades only then).
    static COMPOSITED: Cell<bool> = const { Cell::new(true) };
}

/// The preferences of the bars as the instance should apply them: without a compositing manager nothing fades.
pub fn effective_prefs() -> VideoBarPrefs {
    let mut prefs = prefs::current();
    prefs.fade &= COMPOSITED.with(Cell::get);
    prefs
}

/// Shows, updates or hides the bars of `host`: every target gets the window of its tile, every other bar of the host goes.
/// Returns whether a bar is visible.
pub fn update(host: &VideoBarHost, targets: &[VideoBarTarget], prefs: &VideoBarPrefs) -> bool {
    let mut kept: Vec<TileKey> = Vec::new();
    for target in targets {
        let Some(cell) = window_for(host.owner, target.key) else {
            continue;
        };
        let Ok(mut bar) = cell.try_borrow_mut() else {
            tracing::debug!("[Multiview: VideoBar] Skipped a re-entrant call");
            kept.push(target.key);
            continue;
        };
        if bar.show(host, target, prefs) {
            COMPOSITED.with(|c| c.set(bar.translucent()));
            kept.push(target.key);
        }
    }
    hide_except(host.owner, &kept);
    kept.iter()
        .any(|key| targets.iter().any(|t| t.key == *key && t.phase == Phase::Shown))
}

/// Hides every bar `owner` shows (its Multiview closes, hides or is locked).
pub fn hide(owner: BarOwner) {
    hide_except(owner, &[]);
}

fn hide_except(owner: BarOwner, keep: &[TileKey]) {
    for cell in all() {
        let Ok(mut bar) = cell.try_borrow_mut() else {
            continue;
        };
        if bar.assigned.is_some_and(|(o, key)| o == owner && !keep.contains(&key)) {
            bar.hide(if keep.is_empty() {
                "the host shows no bars"
            } else {
                "its tile has no bar now"
            });
            bar.assigned = None;
        }
    }
}

/// The application exits: all bar windows are hidden and dropped.
pub fn shutdown() {
    let taken: Vec<_> = BARS.with(|all| std::mem::take(&mut *all.borrow_mut()));
    for cell in taken {
        if let Ok(mut bar) = cell.try_borrow_mut() {
            bar.hide("the application exits");
        }
    }
    tracing::debug!("[Shutdown: Windows] Video control bar windows destroyed");
}

/// The window of the bar of tile `key` of `owner`: the one it has, else a free one of the pool, else a new one.
fn window_for(owner: BarOwner, key: TileKey) -> Option<Rc<RefCell<VideoBarWindow>>> {
    let bars = all();
    if let Some(cell) = bars
        .iter()
        .find(|cell| cell.try_borrow().is_ok_and(|b| b.assigned == Some((owner, key))))
    {
        return Some(cell.clone());
    }
    let cell = match bars
        .iter()
        .find(|cell| cell.try_borrow().is_ok_and(|b| b.assigned.is_none()))
    {
        Some(cell) => cell.clone(),
        None if bars.len() < MAX_BARS => {
            let cell = Rc::new(RefCell::new(VideoBarWindow::new()?));
            BARS.with(|all| all.borrow_mut().push(cell.clone()));
            cell
        }
        None => return None,
    };
    cell.try_borrow_mut().ok()?.assign(owner, key);
    Some(cell)
}

fn all() -> Vec<Rc<RefCell<VideoBarWindow>>> {
    BARS.with(|all| all.borrow().clone())
}
