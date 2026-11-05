//! A bar of the instance (`VideoBarTarget`, physical pixels) as the Slint row the canvas draws (`MultiviewVideoBarView`,
//! logical pixels). The lists inside a row (progress spans, saved repeats, menu rows) are models of the bar's slot, refilled only
//! when they changed, so the canvas keeps their items (a rename field keeps its text and focus).

use super::Slot;
use crate::ui::multiview::views::sync_model;
use crate::ui::{AbMenuRowView, AbStripSegmentView, ContextMenuItemModel, MultiviewVideoBarView, VideoBarSpanView};
use crate::viewer::multiview::video_bar::ab_view::AbBarView;
use crate::viewer::multiview::video_bar::more_menu::{EntryKind, MoreAction, MoreEntry};
use crate::viewer::multiview::video_bar::placement::{AbRoom, ab_popup_height, bubble_height};
use crate::viewer::multiview::video_bar::popups::BarPopup;
use crate::viewer::multiview::video_bar::prefs::{BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::reveal::Phase;
use crate::viewer::multiview::video_bar::view::{VideoBarTarget, VideoBarView};
use rustview_core::util::cast::{Az, SaturatingAs, WrappingAs};
use slint::ModelRc;

/// The row of `target`; the lists inside it are refreshed in `slot` when they changed.
pub(super) fn row(target: &VideoBarTarget, prefs: &VideoBarPrefs, slot: &mut Slot) -> MultiviewVideoBarView {
    let view = &target.view;
    sync_lists(view, slot);
    let scale = target.scale.max(0.5);
    let logical = |px: i32| px.az::<f32>() / scale;
    let area = target.placement.area;
    let room = AbRoom::for_bar(prefs.height);
    let (hover_shown, hover_share, hover_text) = match &view.hover {
        Some((share, text)) => (true, *share, text.as_str().into()),
        None => (false, 0.0, slint::SharedString::default()),
    };
    let mut row = MultiviewVideoBarView {
        key: target.key.wrapping_as::<i32>(),
        x: logical(area.x),
        y: logical(area.y),
        width: logical(area.w),
        height: logical(area.h),
        pill: prefs.style == BarStyle::Pill,
        mini: target.mini,
        bar_opacity: prefs.opacity,
        bar_height: prefs.height,
        bubble_height: bubble_height(prefs.height),
        fade: prefs.fade,
        revealed: target.phase == Phase::Shown,
        playing: view.playing,
        current_text: view.times.current.as_str().into(),
        current_template: view.times.current_template.as_str().into(),
        remaining_text: view.times.remaining.as_str().into(),
        remaining_template: view.times.remaining_template.as_str().into(),
        spans: ModelRc::from(slot.spans.clone()),
        volume_share: view.volume_share,
        volume_icon: view.volume_icon.code(),
        muted: view.muted,
        hover_shown,
        hover_share,
        hover_text,
        ab_strip_height: room.strip,
        ab_strip_flank: room.flank,
        ab_row_height: room.row,
        ab_strip: ModelRc::from(slot.ab_strip.clone()),
        ab_menu_rows: ModelRc::from(slot.ab_rows.clone()),
        ab_menu_renaming: -1,
        fit: view.fit.code(),
        volume_text: view.volume_text.as_str().into(),
        volume_open: view.popup == Some(BarPopup::Volume),
        more_open: matches!(view.popup, Some(BarPopup::More { .. })),
        more_items: ModelRc::from(slot.more.clone()),
        more_sub_items: ModelRc::from(slot.more_sub.clone()),
        ab_bubble: view.ab_bubble,
        // The entry whose submenu is open stays highlighted, like the parent of a context menu's submenu.
        more_active_id: view
            .more
            .entries
            .iter()
            .find(|entry| entry.open)
            .and_then(|entry| entry.action)
            .map_or(-1, MoreAction::code),
        ..MultiviewVideoBarView::default()
    };
    if let Some(ab) = &view.ab {
        fill_ab(&mut row, ab, prefs.height);
    }
    row
}

/// The A-B repeat group of the mpv script `ab_repeat.lua`.
fn fill_ab(row: &mut MultiviewVideoBarView, ab: &AbBarView, bar_height: f32) {
    let prefs = crate::viewer::ab_repeat::prefs::current();
    row.ab_shown = true;
    row.ab_a_look = ab.a.code();
    row.ab_b_look = ab.b.code();
    row.ab_loop_look = ab.loop_look.code();
    row.ab_a_tip = ab.a_tip.as_str().into();
    row.ab_b_tip = ab.b_tip.as_str().into();
    row.ab_loop_tip = ab.loop_tip.as_str().into();
    row.ab_a_key = ab.a_key.as_str().into();
    row.ab_b_key = ab.b_key.as_str().into();
    row.ab_loop_key = ab.loop_key.as_str().into();
    row.ab_clearable = ab.clearable;
    row.ab_save_enabled = ab.save_enabled;
    row.ab_save_tip = ab.save_tip.as_str().into();
    row.ab_load_count = ab.load_count.saturating_as::<i32>();
    row.ab_load_tip = ab.load_tip.as_str().into();
    row.ab_flags_draggable = prefs.drag_flags;
    row.ab_strip_clickable = prefs.click_loads;
    if let Some((from, to, looping)) = ab.span {
        row.ab_span_shown = true;
        row.ab_span_from = from;
        row.ab_span_to = to;
        row.ab_span_looping = looping;
    }
    row.ab_dim = ab.dim;
    if let Some(flag) = &ab.a_flag {
        row.ab_a_flag_shown = true;
        row.ab_a_flag_share = flag.share;
        row.ab_a_flag_text = flag.text.as_str().into();
        row.ab_a_flag_auto = flag.auto;
    }
    if let Some(flag) = &ab.b_flag {
        row.ab_b_flag_shown = true;
        row.ab_b_flag_share = flag.share;
        row.ab_b_flag_text = flag.text.as_str().into();
        row.ab_b_flag_auto = flag.auto;
    }
    if let Some((share, text)) = &ab.hover {
        row.ab_label_shown = true;
        row.ab_label_share = *share;
        row.ab_label_text = text.as_str().into();
    }
    row.ab_popup_height = ab.popup_size().map_or(0.0, |size| ab_popup_height(size, bar_height));
    if let Some(save) = &ab.save {
        row.ab_popup = 1;
        row.ab_save_text = save.text.as_str().into();
        row.ab_save_placeholder = save.placeholder.as_str().into();
        row.ab_save_hint = save.hint.as_deref().unwrap_or_default().into();
    } else if let Some(menu) = &ab.menu {
        row.ab_popup = 2;
        row.ab_menu_query = menu.query.as_str().into();
        if let Some(renaming) = &menu.renaming {
            row.ab_menu_renaming = renaming.index.saturating_as::<i32>();
            row.ab_menu_rename_text = renaming.text.as_str().into();
        }
        row.ab_menu_page = menu.page.saturating_as::<i32>();
        row.ab_menu_pages = menu.pages.saturating_as::<i32>();
        row.ab_menu_scroll = menu.scroll;
        row.ab_menu_visible_rows = menu.visible_rows.saturating_as::<i32>();
        row.ab_menu_status = if menu.matches == menu.total {
            format!("{} saved", menu.total)
        } else {
            format!("{} of {} match", menu.matches, menu.total)
        }
        .into();
    }
}

/// Refills the lists of `slot` whose content differs from the last view.
fn sync_lists(view: &VideoBarView, slot: &mut Slot) {
    let last = slot.last.as_ref();
    if last.is_none_or(|last| last.spans != view.spans) {
        let spans = view
            .spans
            .iter()
            .map(|span| VideoBarSpanView {
                from: span.from,
                to: span.to,
                kind: span.kind.code(),
            })
            .collect();
        sync_model(&slot.spans, spans);
    }
    let ab = view.ab.as_ref();
    let last_ab = last.and_then(|last| last.ab.as_ref());
    let strip = ab.map(|ab| ab.strip.as_slice()).unwrap_or_default();
    if last.is_none() || last_ab.map(|ab| ab.strip.as_slice()).unwrap_or_default() != strip {
        let segments = strip
            .iter()
            .map(|segment| AbStripSegmentView {
                from: segment.from,
                to: segment.to,
                color: segment.color.map_or(-1, i32::from),
            })
            .collect();
        sync_model(&slot.ab_strip, segments);
    }
    let rows = ab
        .and_then(|ab| ab.menu.as_ref())
        .map(|menu| menu.rows.as_slice())
        .unwrap_or_default();
    let last_rows = last_ab
        .and_then(|ab| ab.menu.as_ref())
        .map(|menu| menu.rows.as_slice())
        .unwrap_or_default();
    if last.is_none() || last_rows != rows {
        let entries = rows
            .iter()
            .map(|row| AbMenuRowView {
                index: row.index.saturating_as::<i32>(),
                number: row.number.saturating_as::<i32>(),
                name: row.name.as_str().into(),
                times: row.times.as_str().into(),
                length: row.length.as_str().into(),
                from: row.from,
                to: row.to,
                color: row.color.map_or(-1, i32::from),
                current: row.current,
            })
            .collect();
        sync_model(&slot.ab_rows, entries);
    }
    let last_more = last.map(|last| &last.more);
    if last_more.is_none_or(|more| more.entries != view.more.entries) {
        sync_model(&slot.more, view.more.entries.iter().map(menu_item).collect());
    }
    if last_more.is_none_or(|more| more.sub != view.more.sub) {
        sync_model(&slot.more_sub, view.more.sub.iter().map(menu_item).collect());
    }
    slot.last = Some(view.clone());
}

/// An entry of the "..." menu as a row of `MultiviewBarMenu` (the rows of a context menu).
fn menu_item(entry: &MoreEntry) -> ContextMenuItemModel {
    let (kind, checked) = match entry.kind {
        EntryKind::Action => (0, false),
        EntryKind::Check(checked) => (1, checked),
        EntryKind::Separator => (3, false),
        EntryKind::Header => (4, false),
        EntryKind::Submenu => (5, false),
    };
    ContextMenuItemModel {
        id: entry.action.map_or(-1, MoreAction::code),
        text: entry.text.as_str().into(),
        shortcut: entry.shortcut.as_str().into(),
        icon: crate::icons::to_slint(entry.icon),
        kind,
        is_checked: checked,
        is_enabled: entry.enabled,
        has_submenu: entry.kind == EntryKind::Submenu,
        tooltip: entry.tooltip.as_str().into(),
        ..ContextMenuItemModel::default()
    }
}
