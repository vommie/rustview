//! The sidebars as a dock like `RustCut`'s (ported from `RustCut`'s `src/ui/dock/`, same author, kept alike): the pure layout
//! model, its geometry, its place in `settings.json` (`view.dock`), the rows Slint shows and the bridge that connects them.

mod bridge;
mod install;
pub mod model;
mod model_sync;
pub mod persist;
pub mod plan;
mod view;

#[cfg(test)]
mod tests;

pub use bridge::DockBridge;
pub use install::{install, release, reset_layout, stored_layout};
pub use model::{DockLayout, DropTarget, GroupRef, PanelId, Side};
pub use persist::LegacySidebars;
