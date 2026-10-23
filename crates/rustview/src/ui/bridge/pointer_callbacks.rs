//! Rubber band selection and drag and drop callbacks of the Browse view.

use super::UiBridge;
use crate::ui::dnd::{BatchTransferParams, BridgeDnd, DragStartParams};
use crate::ui::selection::rubberband_bridge::{BridgeRubberband, RubberbandDragMoveParams, RubberbandDragStartParams};
use rustview_core::library::models::FileTransferKind;
use rustview_core::util::cast::WrappingAs;
use slint::ComponentHandle;

impl UiBridge {
    pub(super) fn register_rubberband_callbacks(&self) {
        let win_start = self.window.clone_strong();
        let st_start = self.state.clone();
        let scroller_start = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_started(move |cx, cy, vx, vy, ctrl, shift| {
            BridgeRubberband::handle_started(&RubberbandDragStartParams {
                start_content_x: cx,
                start_content_y: cy,
                view_x: vx,
                view_y: vy,
                is_ctrl: ctrl,
                is_shift: shift,
                window: &win_start,
                state: &st_start,
                autoscroller: &scroller_start,
            });
        });

        let win_move = self.window.clone_strong();
        let lib_move = self.library.clone();
        let st_move = self.state.clone();
        let scroller_move = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_moved(move |cx, cy, vx, vy| {
            BridgeRubberband::handle_moved(&RubberbandDragMoveParams {
                content_x: cx,
                content_y: cy,
                view_x: vx,
                view_y: vy,
                window: &win_move,
                library: &lib_move,
                state: &st_move,
                autoscroller: &scroller_move,
            });
        });

        let win_end = self.window.clone_strong();
        let lib_end = self.library.clone();
        let st_end = self.state.clone();
        let scroller_end = self.rubberband_autoscroller.clone();
        self.window.on_rubberband_ended(move || {
            BridgeRubberband::handle_ended(&win_end, &lib_end, &st_end, &scroller_end);
        });
    }

    pub(super) fn register_dnd_callbacks(&self) {
        let win_start = self.window.clone_strong();
        let lib_start = self.library.clone();
        let thumb_start = self.thumbnails.clone();
        let st_start = self.state.clone();
        let autoscroll_start = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_start = self.dnd_browse_scroller.clone();
        let fs_start = self.fullscreen.clone();
        self.window.on_media_drag_started(move |id, x, y| {
            BridgeDnd::handle_drag_started(&DragStartParams {
                media_id: id.wrapping_as::<u64>(),
                start_x: x,
                start_y: y,
                window: &win_start,
                library: &lib_start,
                thumbnails: &thumb_start,
                state: &st_start,
                autoscroller: &autoscroll_start,
                browse_scroller: &browse_scroll_start,
                fullscreen: &fs_start,
            });
        });

        let win_move = self.window.clone_strong();
        let lib_move = self.library.clone();
        let st_move = self.state.clone();
        let autoscroll_move = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_move = self.dnd_browse_scroller.clone();
        self.window.on_media_drag_moved(move |x, y| {
            BridgeDnd::handle_drag_moved(
                x,
                y,
                &win_move,
                &lib_move,
                &st_move,
                &autoscroll_move,
                &browse_scroll_move,
            );
        });

        let win_hover = self.window.clone_strong();
        let st_hover = self.state.clone();
        self.window.on_folder_row_hovered(move |folder_id| {
            BridgeDnd::handle_folder_hovered(folder_id.wrapping_as::<u64>(), &win_hover, &st_hover);
        });

        let win_unhover = self.window.clone_strong();
        self.window.on_folder_row_unhovered(move |folder_id| {
            BridgeDnd::handle_folder_unhovered(folder_id.wrapping_as::<u64>(), &win_unhover);
        });

        let win_end = self.window.clone_strong();
        let lib_end = self.library.clone();
        let st_end = self.state.clone();
        let autoscroll_end = self.dnd_foldertree_autoscroller.clone();
        let browse_scroll_end = self.dnd_browse_scroller.clone();
        self.window.on_media_drag_ended(move || {
            BridgeDnd::handle_drag_ended(&win_end, &lib_end, &st_end, &autoscroll_end, &browse_scroll_end);
        });

        let win_copy = self.window.clone_strong();
        let lib_copy = self.library.clone();
        let thumb_copy = self.thumbnails.clone();
        let set_copy = self.settings.clone();
        let st_copy = self.state.clone();
        let conflict_copy = self.conflict_dialog.clone();
        self.window.on_choice_popup_copy(move |target_fid| {
            win_copy.set_is_choice_popup_open(false);
            BridgeDnd::start_batch_transfer(&BatchTransferParams {
                window: &win_copy,
                library: &lib_copy,
                thumbnails: &thumb_copy,
                settings: &set_copy,
                state: &st_copy,
                conflict_dialog: &conflict_copy,
                target_folder_id: target_fid.wrapping_as::<u64>(),
                transfer_kind: FileTransferKind::Copy,
            });
        });

        let win_mv = self.window.clone_strong();
        let lib_mv = self.library.clone();
        let thumb_mv = self.thumbnails.clone();
        let set_mv = self.settings.clone();
        let st_mv = self.state.clone();
        let conflict_mv = self.conflict_dialog.clone();
        self.window.on_choice_popup_move(move |target_fid| {
            win_mv.set_is_choice_popup_open(false);
            BridgeDnd::start_batch_transfer(&BatchTransferParams {
                window: &win_mv,
                library: &lib_mv,
                thumbnails: &thumb_mv,
                settings: &set_mv,
                state: &st_mv,
                conflict_dialog: &conflict_mv,
                target_folder_id: target_fid.wrapping_as::<u64>(),
                transfer_kind: FileTransferKind::Move,
            });
        });

        let win_cancel = self.window.clone_strong();
        self.window.on_choice_popup_cancel(move || {
            tracing::info!("[UiBridge: ChoicePopup] Floating choice popup cancelled via Cancel button");
            win_cancel.set_is_choice_popup_open(false);
        });

        let win_dism = self.window.clone_strong();
        self.window.on_choice_popup_dismissed(move || {
            tracing::info!("[UiBridge: ChoicePopup] Floating choice popup dismissed via Escape key or backdrop click");
            win_dism.set_is_choice_popup_open(false);
        });
    }
}
