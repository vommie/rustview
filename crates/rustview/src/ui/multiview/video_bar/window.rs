//! One control bar window: its content, X11 shape and placement.

use super::{InputHandler, VideoBarHost};
use crate::ui::multiview::bar_overlay::{self, BarOwner};
use crate::ui::multiview::views::sync_model;
use crate::ui::{AbMenuRowView, AbRegionView, AbStripSegmentView, MultiviewVideoBarWindow, VideoBarSpanView};
use crate::viewer::multiview::bar_placement::{rounded_region, tab_region, xrectangle};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::video_bar::ab_state::AbPoint;
use crate::viewer::multiview::video_bar::ab_view::AbBarView;
use crate::viewer::multiview::video_bar::placement::{AbRoom, ab_popup_height};
use crate::viewer::multiview::video_bar::placement::{VideoBarPlacement, bubble_height};
use crate::viewer::multiview::video_bar::prefs::{BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::reveal::Phase;
use crate::viewer::multiview::video_bar::view::{AbButton, AbInput};
use crate::viewer::multiview::video_bar::view::{DragPhase, VideoBarInput, VideoBarTarget, VideoBarView};
use crate::viewer::{OverlayServer, get_raw_window_xid};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What the callbacks of the window need as well.
#[derive(Default)]
struct Shared {
    /// The tile the bar belongs to now.
    key: Cell<TileKey>,
    on_input: RefCell<Option<InputHandler>>,
    x: RefCell<XState>,
}

/// The X side of the window.
#[derive(Default)]
struct XState {
    server: Option<OverlayServer>,
    xid: Option<u64>,
    /// Blended by a compositing manager; otherwise solid and shaped like the bar.
    translucent: bool,
    /// The bar inside the window, physical pixels relative to the window.
    bar: Option<Rect>,
    /// The time bubble above the bar, physical pixels relative to the window.
    bubble: Option<Rect>,
    /// The tab of the A-B group above the bar (physical pixels relative to the window) and the reach of its flanks, shaped like
    /// the strip's path.
    ab_strip: Option<Rect>,
    ab_flank: i32,
    /// What else the A-B group draws above the bar: its flags, label and open popup.
    ab_parts: Vec<Rect>,
    /// A tooltip of the A-B group.
    tooltip: Option<Rect>,
    bar_radius: i32,
    bubble_radius: i32,
    scale: f32,
}

impl XState {
    /// Only the bar, the band of the A-B group and its popup take the pointer (the rest of the room above the bar lets it
    /// through to the video); without a compositing manager the window is cut to what it shows.
    fn apply_shape(&self) {
        let (Some(server), Some(xid), Some(bar)) = (self.server.as_ref(), self.xid, self.bar) else {
            return;
        };
        let mut input = vec![xrectangle(bar)];
        input.extend(self.ab_strip.iter().chain(&self.ab_parts).map(|r| xrectangle(*r)));
        server.set_input_region(xid, &input);
        if self.translucent {
            server.set_bounding_region(xid, None);
            return;
        }
        let mut region = rounded_region(bar, self.bar_radius);
        for extra in [self.bubble, self.tooltip].into_iter().flatten() {
            region.extend(rounded_region(extra, self.bubble_radius));
        }
        if let Some(strip) = self.ab_strip {
            // The corners are half the strip high, like the path's.
            region.extend(tab_region(strip, self.ab_flank, strip.h / 2));
        }
        region.extend(self.ab_parts.iter().map(|r| xrectangle(*r)));
        server.set_bounding_region(xid, Some(&region));
    }

    /// Whether the screen point (`x`, `y`) is on a part of the window that takes the pointer (`origin` = the window's place).
    fn takes_pointer_at(&self, origin: (i32, i32), x: i32, y: i32) -> bool {
        let (x, y) = (x - origin.0, y - origin.1);
        self.bar.is_some_and(|r| r.contains(x, y))
            || self.ab_strip.iter().chain(&self.ab_parts).any(|r| r.contains(x, y))
    }
}

pub(super) struct VideoBarWindow {
    /// The host and the tile this window serves (`None`: free).
    pub(super) assigned: Option<(BarOwner, TileKey)>,
    window: MultiviewVideoBarWindow,
    spans: Rc<VecModel<VideoBarSpanView>>,
    ab_strip: Rc<VecModel<AbStripSegmentView>>,
    ab_rows: Rc<VecModel<AbMenuRowView>>,
    /// The popup of the A-B group that is open (1 = save, 2 = load), and the row of the menu being renamed.
    ab_popup: i32,
    ab_renaming: Option<usize>,
    /// The duration of hold to confirm the window has.
    hold_ms: Option<u32>,
    /// The height of the bar (logical pixels, Settings > Fullscreen > Player): the popups are sized from it.
    bar_height: f32,
    shared: Rc<Shared>,
    shown: bool,
    /// Where the window is on the screen.
    placed: Option<VideoBarPlacement>,
    last_view: Option<VideoBarView>,
}

impl VideoBarWindow {
    pub(super) fn new() -> Option<Self> {
        let window = match bar_overlay::create_override_redirect(MultiviewVideoBarWindow::new) {
            Ok(window) => window,
            Err(e) => {
                tracing::error!(error = %e, "[Multiview: VideoBar] Failed to create a control bar window");
                return None;
            }
        };
        let server = OverlayServer::new()
            .map_err(|e| tracing::warn!(error = %e, "[Multiview: VideoBar] No X connection; the bar cannot be placed"))
            .ok();
        let shared = Rc::new(Shared::default());
        shared.x.borrow_mut().server = server;
        let spans = Rc::new(VecModel::default());
        window.set_spans(ModelRc::from(spans.clone()));
        let ab_strip = Rc::new(VecModel::default());
        window.set_ab_strip(ModelRc::from(ab_strip.clone()));
        let ab_rows = Rc::new(VecModel::default());
        window.set_ab_menu_rows(ModelRc::from(ab_rows.clone()));
        connect(&window, &shared);
        tracing::info!("[Multiview: VideoBar] Control bar window created");
        Some(Self {
            assigned: None,
            window,
            spans,
            ab_strip,
            ab_rows,
            ab_popup: 0,
            ab_renaming: None,
            hold_ms: None,
            bar_height: 30.0,
            shared,
            shown: false,
            placed: None,
            last_view: None,
        })
    }

    /// Gives the window to the bar of tile `key` of `owner`.
    pub(super) fn assign(&mut self, owner: BarOwner, key: TileKey) {
        self.hide("the window serves another tile");
        self.assigned = Some((owner, key));
        self.shared.key.set(key);
        self.last_view = None;
    }

    pub(super) fn translucent(&self) -> bool {
        self.shared.x.borrow().translucent
    }

    /// Shows the bar of `target` (or lets it fade out). Returns whether the window is visible.
    pub(super) fn show(&mut self, host: &VideoBarHost, target: &VideoBarTarget, prefs: &VideoBarPrefs) -> bool {
        *self.shared.on_input.borrow_mut() = Some(host.on_input.clone());
        // Before the check below: with the pointer on the bar, the bar's own window is the one in front.
        if self.shown {
            self.adopt_native_window();
        }
        let Some((origin, in_front)) = self.host_state(host.host_xid) else {
            self.hide("the host window is not known to X");
            return false;
        };
        // A bar never floats over another application (a drag that left the host keeps it, though).
        if !in_front && !target.held {
            self.hide("another window is in front at the pointer");
            return false;
        }
        if !self.shown {
            self.refresh_glass();
        }
        self.sync_prefs(prefs);
        self.window.set_mini(target.mini);
        self.sync_view(&target.view);
        self.place(target.placement.translated(origin.0, origin.1), prefs);
        if !self.shown {
            if let Err(e) = self.window.show() {
                tracing::warn!(error = %e, "[Multiview: VideoBar] A control bar could not be shown");
                return false;
            }
            self.shown = true;
            tracing::debug!(tile = target.key, "[Multiview: VideoBar] Bar shown");
        }
        self.adopt_native_window();
        self.window.set_revealed(target.phase == Phase::Shown);
        self.follow_popup(host, origin);
        true
    }

    /// A popup of the A-B group that opened gets the keyboard (its window is override-redirect, so the window manager never
    /// gives it the focus), one that closed gives it back to the host; a press outside the window's parts closes it.
    fn follow_popup(&mut self, host: &VideoBarHost, origin: (i32, i32)) {
        let open = self.window.get_ab_popup();
        let state = self.shared.x.borrow();
        let (Some(server), Some(xid)) = (state.server.as_ref(), state.xid) else {
            return;
        };
        if open != 0 && open != self.ab_popup {
            server.focus(xid);
            self.window.invoke_focus_ab_popup();
            tracing::debug!(popup = open, "[Multiview: VideoBar] A-B popup has the keyboard");
        } else if open == 0
            && self.ab_popup != 0
            && let Some(top) = server.top_level_of(host.host_xid)
        {
            server.focus(top);
            tracing::debug!("[Multiview: VideoBar] Keyboard back to the host window");
        }
        let pressed_outside = open != 0
            && server
                .pointer_and_buttons()
                .is_some_and(|((x, y), pressed)| pressed && !state.takes_pointer_at(origin, x, y));
        drop(state);
        self.ab_popup = open;
        if pressed_outside {
            let handler = self.shared.on_input.borrow().clone();
            if let Some(handler) = handler {
                handler(self.shared.key.get(), VideoBarInput::Ab(AbInput::ClosePopup));
            }
        }
    }

    pub(super) fn hide(&mut self, reason: &'static str) {
        if !self.shown {
            return;
        }
        self.window.set_revealed(false);
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = %e, "[Multiview: VideoBar] A control bar could not be hidden");
        }
        self.shown = false;
        self.placed = None;
        {
            let mut state = self.shared.x.borrow_mut();
            state.bubble = None;
            state.tooltip = None;
        }
        if self.ab_popup != 0 {
            self.window.set_ab_popup(0);
            self.ab_popup = 0;
        }
        tracing::debug!(reason, "[Multiview: VideoBar] Bar hidden");
    }

    fn sync_prefs(&mut self, prefs: &VideoBarPrefs) {
        let hold = crate::ui::hold_confirm::current_ms();
        if self.hold_ms != Some(hold) {
            crate::ui::hold_confirm::install(&self.window, hold);
            self.hold_ms = Some(hold);
        }
        self.bar_height = prefs.height;
        let w = &self.window;
        let room = AbRoom::for_bar(prefs.height);
        w.set_ab_strip_height(room.strip);
        w.set_ab_strip_flank(room.flank);
        w.set_ab_row_height(room.row);
        w.set_pill(prefs.style == BarStyle::Pill);
        w.set_bar_opacity(prefs.opacity);
        w.set_bar_height(prefs.height);
        w.set_bubble_height(bubble_height(prefs.height));
        w.set_fade(prefs.fade);
    }

    fn sync_view(&mut self, view: &VideoBarView) {
        if self.last_view.as_ref() == Some(view) {
            return;
        }
        let w = &self.window;
        w.set_playing(view.playing);
        w.set_current_text(view.times.current.as_str().into());
        w.set_current_template(view.times.current_template.as_str().into());
        w.set_remaining_text(view.times.remaining.as_str().into());
        w.set_remaining_template(view.times.remaining_template.as_str().into());
        w.set_volume_share(view.volume_share);
        w.set_volume_icon(view.volume_icon.code());
        w.set_muted(view.muted);
        match &view.hover {
            Some((share, text)) => {
                w.set_hover_share(*share);
                w.set_hover_text(text.as_str().into());
                w.set_hover_shown(true);
            }
            None => w.set_hover_shown(false),
        }
        let spans: Vec<VideoBarSpanView> = view
            .spans
            .iter()
            .map(|span| VideoBarSpanView {
                from: span.from,
                to: span.to,
                kind: span.kind.code(),
            })
            .collect();
        if self.last_view.as_ref().is_none_or(|last| last.spans != view.spans) {
            sync_model(&self.spans, spans);
        }
        self.sync_ab(view.ab.as_ref());
        self.last_view = Some(view.clone());
    }

    /// The A-B repeat group (hidden without one). Texts the user types are only set when their popup or rename opens.
    fn sync_ab(&mut self, ab: Option<&AbBarView>) {
        let w = &self.window;
        let Some(ab) = ab else {
            w.set_ab_shown(false);
            w.set_ab_popup(0);
            sync_model(&self.ab_strip, Vec::new());
            sync_model(&self.ab_rows, Vec::new());
            self.ab_renaming = None;
            return;
        };
        let prefs = crate::viewer::ab_repeat::prefs::current();
        w.set_ab_shown(true);
        w.set_ab_a_look(ab.a.code());
        w.set_ab_b_look(ab.b.code());
        w.set_ab_loop_look(ab.loop_look.code());
        w.set_ab_a_tip(ab.a_tip.as_str().into());
        w.set_ab_b_tip(ab.b_tip.as_str().into());
        w.set_ab_loop_tip(ab.loop_tip.as_str().into());
        w.set_ab_save_enabled(ab.save_enabled);
        w.set_ab_save_tip(ab.save_tip.as_str().into());
        w.set_ab_load_count(ab.load_count.saturating_as::<i32>());
        w.set_ab_load_tip(ab.load_tip.as_str().into());
        w.set_ab_flags_draggable(prefs.drag_flags);
        w.set_ab_strip_clickable(prefs.click_loads);
        sync_model(
            &self.ab_strip,
            ab.strip
                .iter()
                .map(|segment| AbStripSegmentView {
                    from: segment.from,
                    to: segment.to,
                    color: segment.color.map_or(-1, i32::from),
                })
                .collect(),
        );
        match ab.span {
            Some((from, to, looping)) => {
                w.set_ab_span_from(from);
                w.set_ab_span_to(to);
                w.set_ab_span_looping(looping);
                w.set_ab_span_shown(true);
            }
            None => w.set_ab_span_shown(false),
        }
        w.set_ab_dim(ab.dim);
        w.set_ab_a_flag_shown(ab.a_flag.is_some());
        if let Some(flag) = &ab.a_flag {
            w.set_ab_a_flag_share(flag.share);
            w.set_ab_a_flag_text(flag.text.as_str().into());
            w.set_ab_a_flag_auto(flag.auto);
        }
        w.set_ab_b_flag_shown(ab.b_flag.is_some());
        if let Some(flag) = &ab.b_flag {
            w.set_ab_b_flag_share(flag.share);
            w.set_ab_b_flag_text(flag.text.as_str().into());
            w.set_ab_b_flag_auto(flag.auto);
        }
        match &ab.hover {
            Some((share, text)) => {
                w.set_ab_label_share(*share);
                w.set_ab_label_text(text.as_str().into());
                w.set_ab_label_shown(true);
            }
            None => w.set_ab_label_shown(false),
        }
        let popup = if ab.save.is_some() {
            1
        } else if ab.menu.is_some() {
            2
        } else {
            0
        };
        let opened = popup != w.get_ab_popup();
        w.set_ab_popup_height(
            ab.popup_size()
                .map_or(0.0, |size| ab_popup_height(size, self.bar_height)),
        );
        if let Some(save) = &ab.save {
            if opened {
                w.set_ab_save_text(save.text.as_str().into());
            }
            w.set_ab_save_placeholder(save.placeholder.as_str().into());
            w.set_ab_save_hint(save.hint.as_deref().unwrap_or_default().into());
        }
        if let Some(menu) = &ab.menu {
            if opened {
                w.set_ab_menu_query(menu.query.as_str().into());
            }
            let renaming = menu.renaming.as_ref().map(|r| r.index);
            if renaming != self.ab_renaming
                && let Some(r) = &menu.renaming
            {
                w.set_ab_menu_rename_text(r.text.as_str().into());
            }
            self.ab_renaming = renaming;
            w.set_ab_menu_renaming(renaming.map_or(-1, SaturatingAs::saturating_as::<i32>));
            w.set_ab_menu_page(menu.page.saturating_as::<i32>());
            w.set_ab_menu_pages(menu.pages.saturating_as::<i32>());
            w.set_ab_menu_scroll(menu.scroll);
            w.set_ab_menu_visible_rows(menu.visible_rows.saturating_as::<i32>());
            w.set_ab_menu_status(
                if menu.matches == menu.total {
                    format!("{} saved", menu.total)
                } else {
                    format!("{} of {} match", menu.matches, menu.total)
                }
                .into(),
            );
            sync_model(
                &self.ab_rows,
                menu.rows
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
                    .collect(),
            );
        } else {
            self.ab_renaming = None;
        }
        w.set_ab_popup(popup);
    }

    fn place(&mut self, placement: VideoBarPlacement, prefs: &VideoBarPrefs) {
        let moved = self.placed != Some(placement);
        if moved {
            let window = self.window.window();
            window.set_size(PhysicalSize::new(
                placement.window.w.max(1).saturating_as::<u32>(),
                placement.window.h.max(1).saturating_as::<u32>(),
            ));
            window.set_position(PhysicalPosition::new(placement.window.x, placement.window.y));
            self.placed = Some(placement);
        }
        let scale = self.window.window().scale_factor().max(0.5);
        let mut state = self.shared.x.borrow_mut();
        let bar = Rect::new(
            placement.bar.x - placement.window.x,
            placement.bar.y - placement.window.y,
            placement.bar.w,
            placement.bar.h,
        );
        let radius = match prefs.style {
            BarStyle::Pill => bar.h / 2,
            BarStyle::Bar => 0,
        };
        let changed =
            state.bar != Some(bar) || state.bar_radius != radius || (state.scale - scale).abs() > f32::EPSILON;
        state.bar = Some(bar);
        state.bar_radius = radius;
        state.bubble_radius = (4.0 * prefs.height / 30.0 * scale).round().to_int::<i32>();
        state.scale = scale;
        if (changed || moved)
            && let (Some(server), Some(xid)) = (state.server.as_ref(), state.xid)
        {
            server.raise(xid);
            state.apply_shape();
        }
    }

    /// Slint creates the native window in its event loop, so it may not exist yet right after the first `show`: as soon as it
    /// does, its X id is known, it counts as part of the Multiview, comes to the front and gets its shape.
    fn adopt_native_window(&self) {
        let mut state = self.shared.x.borrow_mut();
        if state.xid.is_some() {
            return;
        }
        state.xid = get_raw_window_xid(self.window.window());
        let Some(xid) = state.xid else {
            return;
        };
        bar_overlay::register_overlay_xid(xid);
        if let Some(server) = state.server.as_ref() {
            server.raise(xid);
        }
        state.apply_shape();
        drop(state);
        // The depth of the window decides whether it can be glass; it is known only now.
        self.refresh_glass();
    }

    /// Where the host window is on the screen, and whether it (or a bar over it) is in front at the pointer.
    fn host_state(&self, host_xid: u64) -> Option<((i32, i32), bool)> {
        let state = self.shared.x.borrow();
        let server = state.server.as_ref()?;
        let origin = server.origin_in_root(host_xid)?;
        let host_top = server.top_level_of(host_xid)?;
        let in_front = server
            .top_level_at_pointer()
            .is_some_and(|front| front == host_top || bar_overlay::is_overlay_xid(front));
        Some((origin, in_front))
    }

    /// Glass if a compositing manager blends the window and the window has an alpha channel, solid otherwise.
    fn refresh_glass(&self) {
        let translucent = {
            let state = self.shared.x.borrow();
            state.server.as_ref().is_some_and(|server| {
                server.compositor_running() && state.xid.is_none_or(|xid| server.window_depth(xid) == Some(32))
            })
        };
        let changed = std::mem::replace(&mut self.shared.x.borrow_mut().translucent, translucent) != translucent;
        if changed {
            tracing::info!(translucent, "[Multiview: VideoBar] Bar style");
        }
        self.window.set_translucent(translucent);
    }
}

/// Hands every click, drag and wheel of the window to the input handler of its host, with the tile it belongs to.
fn connect(window: &MultiviewVideoBarWindow, shared: &Rc<Shared>) {
    let send = {
        let shared = shared.clone();
        move |input: VideoBarInput| {
            let handler = shared.on_input.borrow().clone();
            if let Some(handler) = handler {
                handler(shared.key.get(), input);
            }
        }
    };
    let send = Rc::new(send);
    let s = send.clone();
    window.on_play_pause(move || s(VideoBarInput::PlayPause));
    let s = send.clone();
    window.on_toggle_ms(move || s(VideoBarInput::ToggleMs));
    let s = send.clone();
    window.on_speaker(move || s(VideoBarInput::Speaker));
    let s = send.clone();
    window.on_seek(move |share, phase| {
        if let Some(phase) = DragPhase::from_code(phase) {
            s(VideoBarInput::Seek { share, phase });
        }
    });
    let s = send.clone();
    window.on_hover(move |inside, share| {
        s(VideoBarInput::Hover {
            share: inside.then_some(share),
        });
    });
    let s = send.clone();
    window.on_volume(move |share, phase| {
        if let Some(phase) = DragPhase::from_code(phase) {
            s(VideoBarInput::Volume { share, phase });
        }
    });
    let s = send.clone();
    window.on_wheel(move |delta_y| s(VideoBarInput::Wheel { delta_y }));
    connect_ab(window, &send);
    let for_bubble = shared.clone();
    window.on_bubble_changed(move |visible, x, y, w, h| {
        let Ok(mut state) = for_bubble.x.try_borrow_mut() else {
            return;
        };
        let rect = logical_rect(state.scale, visible, (x, y, w, h));
        state.bubble = rect;
        state.apply_shape();
    });
    let for_tooltip = shared.clone();
    window.on_tooltip_changed(move |visible, x, y, w, h| {
        let Ok(mut state) = for_tooltip.x.try_borrow_mut() else {
            return;
        };
        let rect = logical_rect(state.scale, visible, (x, y, w, h));
        state.tooltip = rect;
        state.apply_shape();
    });
    let for_region = shared.clone();
    window.on_ab_region_changed(move |region: AbRegionView| {
        let Ok(mut state) = for_region.x.try_borrow_mut() else {
            return;
        };
        let scale = state.scale;
        state.ab_strip = logical_rect(
            scale,
            true,
            (region.strip_x, region.strip_y, region.strip_w, region.strip_h),
        );
        state.ab_flank = (region.strip_flank * scale.max(0.5)).round().to_int::<i32>();
        state.ab_parts = [
            (region.a_x, region.a_y, region.a_w, region.a_h),
            (region.b_x, region.b_y, region.b_w, region.b_h),
            (region.label_x, region.label_y, region.label_w, region.label_h),
            (region.popup_x, region.popup_y, region.popup_w, region.popup_h),
        ]
        .into_iter()
        .filter_map(|rect| logical_rect(scale, true, rect))
        .collect();
        state.apply_shape();
    });
}

/// A rectangle the window reported in logical pixels, in physical ones (`None`: hidden or empty).
fn logical_rect(scale: f32, visible: bool, (x, y, w, h): (f32, f32, f32, f32)) -> Option<Rect> {
    let scale = scale.max(0.5);
    let px = |logical: f32| (logical * scale).round().to_int::<i32>();
    (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)))
}

/// The callbacks of the A-B repeat group.
fn connect_ab(window: &MultiviewVideoBarWindow, send: &Rc<impl Fn(VideoBarInput) + 'static>) {
    let ab = {
        let send = send.clone();
        Rc::new(move |input: AbInput| send(VideoBarInput::Ab(input)))
    };
    let s = ab.clone();
    window.on_ab_button(move |code| {
        let button = match code {
            0 => AbButton::SetA,
            1 => AbButton::SetB,
            2 => AbButton::Loop,
            3 => AbButton::Clear,
            4 => AbButton::Save,
            5 => AbButton::Load,
            _ => return,
        };
        s(AbInput::Button(button));
    });
    let s = ab.clone();
    window.on_ab_save_edited(move |text| s(AbInput::SaveText(text.into())));
    let s = ab.clone();
    window.on_ab_save_commit(move |text| s(AbInput::SaveCommit(text.into())));
    let s = ab.clone();
    window.on_ab_close_popup(move || s(AbInput::ClosePopup));
    let s = ab.clone();
    window.on_ab_menu_query_edited(move |text| s(AbInput::MenuQuery(text.into())));
    let s = ab.clone();
    window.on_ab_menu_page_step(move |delta| s(AbInput::MenuPage(delta)));
    let s = ab.clone();
    window.on_ab_menu_pick(move |index| s(AbInput::MenuPick(index.saturating_as::<usize>())));
    let s = ab.clone();
    window.on_ab_menu_delete(move |index| s(AbInput::MenuDelete(index.saturating_as::<usize>())));
    let s = ab.clone();
    window.on_ab_menu_delete_all(move || s(AbInput::MenuDeleteAll));
    let s = ab.clone();
    window.on_ab_menu_rename(move |index| s(AbInput::MenuRename(index.saturating_as::<usize>())));
    let s = ab.clone();
    window.on_ab_menu_rename_edited(move |text| s(AbInput::MenuRenameText(text.into())));
    let s = ab.clone();
    window.on_ab_menu_rename_commit(move |text| s(AbInput::MenuRenameCommit(text.into())));
    let s = ab.clone();
    window.on_ab_menu_rename_cancel(move || s(AbInput::MenuRenameCancel));
    let s = ab.clone();
    window.on_ab_strip_hover(move |inside, share| s(AbInput::StripHover(inside.then_some(share))));
    let s = ab.clone();
    window.on_ab_strip_click(move |share| s(AbInput::StripClick(share)));
    window.on_ab_flag(move |point, share, phase| {
        let point = if point == 0 { AbPoint::A } else { AbPoint::B };
        if let Some(phase) = DragPhase::from_code(phase) {
            ab(AbInput::Flag { point, share, phase });
        }
    });
}
