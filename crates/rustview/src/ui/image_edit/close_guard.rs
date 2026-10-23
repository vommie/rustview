//! Nothing that holds pictures with unsaved edits closes unasked: closing the fullscreen, closing tabs, removing a tile, paging
//! to another picture and following another selection first ask what happens to them (`unsaved_dialog`). Checked pictures are
//! saved one after the other (through the export dialog where their format cannot be written), the others are discarded,
//! and only when every save succeeded does the close go on. Cancelling, or a save that fails or is cancelled, calls it off.
//!
//! Call sites only ask "is there something unsaved?" (`defer_*` returns `true` when the question took over); the guard then
//! carries out the close itself once the user agreed.

use super::EditContext;
use super::save_flow::{SaveIntent, save_tile};
use super::unsaved_dialog::UnsavedDialog;
use crate::ui::multiview::handle::MultiviewHandle;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::viewer::TabId;
use crate::viewer::edit::unsaved::{CloseReason, UnsavedEntry, prompt};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

thread_local! {
    /// A question is open: further close requests wait for its answer instead of asking again.
    static ASKING: Cell<bool> = const { Cell::new(false) };
}

/// Whether a question about unsaved pictures is open right now.
pub fn is_asking() -> bool {
    ASKING.with(Cell::get)
}

/// The tiles of `instance` that leave (`None` = all of them).
pub type Leaving = (SharedInstance, Option<Vec<TileKey>>);

/// Asks about the unsaved pictures among `leaving`. Without any, `proceed` runs at once and the result is `true`; otherwise the
/// question is shown, the result is `false`, and `proceed` runs once every checked picture was saved and the others discarded.
pub fn confirm(
    context: &EditContext,
    leaving: Vec<Leaving>,
    reason: CloseReason,
    proceed: impl FnOnce() + 'static,
) -> bool {
    let mut entries: Vec<UnsavedEntry> = Vec::new();
    let mut owners: HashMap<TileKey, WeakInstance> = HashMap::new();
    for (instance, keys) in leaving {
        let mv = instance.lock();
        let found = match keys {
            Some(keys) => mv.unsaved_entries_of(&keys),
            None => mv.unsaved_entries(),
        };
        for entry in found {
            owners.insert(entry.tile, Arc::downgrade(&instance));
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        proceed();
        return true;
    }
    if is_asking() {
        tracing::info!("[ImageEdit: Close] A question about unsaved pictures is open already; this close waits");
        return false;
    }
    let Some(window) = context.window.upgrade() else {
        return false;
    };
    ASKING.with(|a| a.set(true));
    tracing::info!(
        pictures = entries.len(),
        ?reason,
        "[ImageEdit: Close] Unsaved pictures would be lost; asking"
    );
    let context = context.clone();
    UnsavedDialog::ask(&window, prompt(reason, entries), move |decision| {
        ASKING.with(|a| a.set(false));
        let Some(decision) = decision else {
            tracing::info!("[ImageEdit: Close] Cancelled; nothing closes");
            return;
        };
        for key in &decision.discard {
            if let Some(instance) = owners.get(key).and_then(WeakInstance::upgrade) {
                instance.lock().discard_edits(*key);
                context.handle.refresh_instance(&instance);
            }
        }
        let queue: VecDeque<(WeakInstance, TileKey)> = decision
            .overwrite
            .iter()
            .chain(&decision.export)
            .filter_map(|key| owners.get(key).map(|owner| (owner.clone(), *key)))
            .collect();
        save_queue(context, queue, Box::new(proceed));
    });
    false
}

/// Saves the pictures of `queue` one after the other, then runs `proceed`; a picture that is not saved stops the queue.
fn save_queue(context: EditContext, mut queue: VecDeque<(WeakInstance, TileKey)>, proceed: Box<dyn FnOnce()>) {
    let Some((owner, key)) = queue.pop_front() else {
        proceed();
        return;
    };
    let Some(instance) = owner.upgrade() else {
        save_queue(context, queue, proceed);
        return;
    };
    let next = context.clone();
    save_tile(
        &context,
        &instance,
        key,
        SaveIntent::Save,
        Some(Box::new(move |done| {
            if done {
                save_queue(next, queue, proceed);
            } else {
                tracing::info!("[ImageEdit: Close] A picture was not saved; the close is called off");
            }
        })),
    );
}

/// Closing the tabs `ids`: `true` when the question took over (the guard closes the tabs itself once the user agreed);
/// `false` when nothing is unsaved and the caller closes them as usual.
pub fn defer_tab_close(handle: &MultiviewHandle, ids: Vec<TabId>) -> bool {
    let Some((app, window)) = handle.app_and_window_for_edit() else {
        return false;
    };
    let leaving: Vec<Leaving> = ids
        .iter()
        .filter_map(|id| app.tab_manager.get_tab(*id))
        .map(|tab| (tab.multiview.instance.clone(), None))
        .filter(|(instance, _)| instance.lock().has_unsaved_edits())
        .collect();
    if leaving.is_empty() {
        return false;
    }
    let context = handle.edit_context(app.clone(), &window);
    let (handle, weak) = (handle.clone(), slint::ComponentHandle::as_weak(&window));
    let count = ids.len();
    !confirm(&context, leaving, CloseReason::Tabs(count), move || {
        let Some(window) = weak.upgrade() else { return };
        let (closed, _) = app.tab_manager.close_tabs(&ids);
        tracing::info!(closed, "[ImageEdit: Close] Tabs closed after the question");
        BridgeViewerTabs::sync_ui(&window, &app.tab_manager, &app.library, &app.settings, &app.state);
        BridgeViewerTabs::sync_video_embedding(&window, &app.tab_manager, &handle, &app.settings);
        handle.sync_tab(&app.tab_manager);
        if window.get_active_view_mode() == 2 {
            BridgeTabGrid::sync_tab_grid_ui(&window, &app.tab_manager, &app.library, &app.settings, &app.state);
        }
    })
}

/// The tabs that "Close Tabs to the Left/Right", "Close Other Tabs" and "Close All Tabs" close, in the order of the tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabRange {
    Left,
    Right,
    Others,
    All,
}

pub fn tabs_in_range(order: &[TabId], range: TabRange, tab: TabId) -> Vec<TabId> {
    let position = order.iter().position(|id| *id == tab);
    match (range, position) {
        (TabRange::All, _) => order.to_vec(),
        (TabRange::Others, _) => order.iter().copied().filter(|id| *id != tab).collect(),
        (TabRange::Left, Some(p)) => order[..p].to_vec(),
        (TabRange::Right, Some(p)) => order[p + 1..].to_vec(),
        (_, None) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ranges_of_the_tab_menu_name_the_tabs_they_close() {
        let order = [4, 9, 2, 7];
        assert_eq!(tabs_in_range(&order, TabRange::Left, 2), [4, 9]);
        assert_eq!(tabs_in_range(&order, TabRange::Right, 9), [2, 7]);
        assert_eq!(tabs_in_range(&order, TabRange::Others, 9), [4, 2, 7]);
        assert_eq!(tabs_in_range(&order, TabRange::All, 9), order);
        assert_eq!(tabs_in_range(&order, TabRange::Left, 4), Vec::<TabId>::new());
        assert_eq!(tabs_in_range(&order, TabRange::Right, 5), Vec::<TabId>::new());
    }
}
