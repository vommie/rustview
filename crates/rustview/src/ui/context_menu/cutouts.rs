//! Cutouts of native video windows below open menus.

pub use crate::ui::context_menu::favorite::FavoriteContextMenu;
pub use crate::ui::context_menu::folder::FolderContextMenu;
pub use crate::ui::context_menu::media::{MediaActionContext, MediaContextMenu, MediaSelectionMeta};
pub use crate::ui::context_menu::presenter::CtxMenuPresenter;
pub use crate::ui::context_menu::tab::{TabActionContext, TabContextMenu};
pub use crate::ui::context_menu::tag::TagContextMenu;
pub use crate::ui::file_dialogs::conflict::FileOpsConflictDialogController;
pub use crate::ui::file_dialogs::delete::DeleteFileDialogController;
pub use crate::ui::file_dialogs::missing::{MissingFileDialogController, MissingFileItem};
pub use crate::ui::file_dialogs::remove_orphaned::RemoveOrphanedFileDialogController;
pub use crate::ui::file_dialogs::{FileDialogContext, FileRenameContext, FileRenameDialogController};
use crate::ui::MainWindow;
use crate::viewer::XRectangle;
use slint::{ComponentHandle, Model};

/// Computes MPV cutouts for all currently mapped menu level rectangles.
pub fn calculate_all_active_menu_cutouts(
    window: &MainWindow,
) -> Vec<XRectangle> {
    let scale = window.window().scale_factor();
    let levels = window.get_context_menu_levels();
    let count = levels.row_count();

    if count == 0 {
        return Vec::new();
    }

    let mut all_cutouts = Vec::new();

    for i in 0..count {
        if let Some(lvl) = levels.row_data(i) {
            let h = lvl.max_height;
            let cutouts = make_rounded_cutout_rectangles(lvl.x, lvl.y, lvl.width, h, scale);
            all_cutouts.extend(cutouts);
        }
    }

    all_cutouts
}

pub fn make_rounded_cutout_rectangles(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    scale: f32,
) -> Vec<XRectangle> {
    let px = (x * scale).round() as i16;
    let py = (y * scale).round() as i16;
    let pw = (w * scale).round() as u16;
    let ph = (h * scale).round() as u16;
    let pr = ((6.0 * scale).round() as i16).max(2).min(ph as i16 / 3);

    vec![
        XRectangle {
            x: px,
            y: py + pr,
            width: pw,
            height: ph.saturating_sub((pr * 2) as u16).max(1),
        },
        XRectangle {
            x: px + 1,
            y: py + 2,
            width: pw.saturating_sub(2).max(1),
            height: ph.saturating_sub(4).max(1),
        },
        XRectangle {
            x: px + 2,
            y: py + 1,
            width: pw.saturating_sub(4).max(1),
            height: ph.saturating_sub(2).max(1),
        },
        XRectangle {
            x: px + pr,
            y: py,
            width: pw.saturating_sub((pr * 2) as u16).max(1),
            height: ph,
        },
    ]
}
