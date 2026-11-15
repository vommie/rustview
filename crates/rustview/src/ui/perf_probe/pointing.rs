//! Where the probe's pointer and key events go (the main window, or the fullscreen window of a Multiview) and where the
//! things a plan names are: its elements (`@left-resizer`, `@tile:1`, ...) and folders.

use super::driver::Driver;
use super::input;
use super::plan::Anchor;
use super::recorder;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::multiview::probe;
use crate::ui::{MainWindow, MultiviewGutterView, MultiviewTabBus, MultiviewTileView};
use rustview_core::util::cast::FloatToInt;
use slint::{ComponentHandle, Model, ModelRc};

/// The fullscreen of a Multiview shows its media.
pub fn fullscreen_shown() -> bool {
    probe::fullscreen_window().is_some_and(|window| window.get_tiles().row_count() > 0)
}

fn row<T: Clone + 'static>(model: &ModelRc<T>, index: i32) -> Option<T> {
    usize::try_from(index).ok().and_then(|index| model.row_data(index))
}

impl Driver {
    /// Opens the folder with that name like a click in the tree; `false` when the library has none.
    pub(super) fn open_folder(&self, window: &MainWindow, name: &str) -> bool {
        let found = {
            let st = self.ctx.state.read();
            st.folder_hierarchy.as_deref().and_then(|tree| {
                tree.nodes
                    .values()
                    .find(|node| node.name == name && !node.is_deleted)
                    .map(|node| (node.id, node.path.clone()))
            })
        };
        let Some((id, path)) = found else { return false };
        recorder::event("folder_select", id);
        BridgeFolders::select_folder(
            id,
            &path,
            window,
            &self.ctx.library,
            &self.ctx.thumbnails,
            &self.ctx.settings,
            &self.ctx.state,
        );
        true
    }

    /// Runs `f` with the window the steps act on.
    pub(super) fn with_target<R>(&self, window: &MainWindow, f: impl FnOnce(&slint::Window) -> R) -> R {
        match self.fullscreen.then(probe::fullscreen_window).flatten() {
            Some(fullscreen) => f(fullscreen.window()),
            None => f(window.window()),
        }
    }

    /// Moves the pointer of the window the steps act on to where the probe's pointer is. The hosts of a Multiview read the
    /// pointer from winit, which the probe's events do not pass, so they are told too.
    pub(super) fn move_pointer(&self, window: &MainWindow) {
        let point = self.pointer;
        let scale = self.with_target(window, |target| {
            input::move_to(target, point);
            target.scale_factor()
        });
        probe::pointer_moved(
            self.fullscreen,
            ((point.0 * scale).to_int::<i32>(), (point.1 * scale).to_int::<i32>()),
        );
    }

    /// The tiles and gutters of the Multiview the steps act on, and where its canvas is in its window.
    fn multiview(&self, window: &MainWindow) -> Option<Canvas> {
        if self.fullscreen {
            let fullscreen = probe::fullscreen_window()?;
            return Some(Canvas {
                origin: (0.0, 0.0),
                tiles: fullscreen.get_tiles(),
                gutters: fullscreen.get_gutters(),
            });
        }
        let origin = window.invoke_probe_anchor("multiview".into(), 0);
        let bus = window.global::<MultiviewTabBus>();
        Some(Canvas {
            origin: (origin.x, origin.y),
            tiles: bus.get_tiles(),
            gutters: bus.get_gutters(),
        })
    }

    /// The point of the window an element of the plan is at.
    pub(super) fn anchor(&self, window: &MainWindow, anchor: &Anchor) -> Option<(f32, f32)> {
        match anchor.name.as_str() {
            "tile" => {
                let canvas = self.multiview(window)?;
                let tile = row(&canvas.tiles, anchor.index)?;
                Some((
                    canvas.origin.0 + tile.x + tile.width / 2.0,
                    canvas.origin.1 + tile.y + tile.height / 2.0,
                ))
            }
            "gutter" => {
                // The swap button sits in the middle of a gutter: a quarter along it is free.
                let canvas = self.multiview(window)?;
                let gutter = row(&canvas.gutters, anchor.index)?;
                let along = if gutter.vertical { (0.5, 0.25) } else { (0.25, 0.5) };
                Some((
                    canvas.origin.0 + gutter.x + gutter.width * along.0,
                    canvas.origin.1 + gutter.y + gutter.height * along.1,
                ))
            }
            name => {
                let point = window.invoke_probe_anchor(name.into(), anchor.index);
                (point.x >= 0.0 && point.y >= 0.0).then_some((point.x, point.y))
            }
        }
    }
}

struct Canvas {
    origin: (f32, f32),
    tiles: ModelRc<MultiviewTileView>,
    gutters: ModelRc<MultiviewGutterView>,
}
