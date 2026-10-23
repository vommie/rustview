//! The icon of a viewer tab: one place decides it for the tab bar, the drag ghost and the tab grid. A tab of one medium shows the
//! icon of that medium, a tab of several media the Multiview icon.

use crate::icons::{self, Icon};
use crate::ui::IconId;
use crate::viewer::MediaTabSession;
use crate::viewer::multiview::model::TileKind;

pub fn tab_icon(tab: &MediaTabSession) -> IconId {
    let icon = tab
        .multiview
        .instance
        .lock()
        .solo_tile()
        .map_or(Icon::OpenMultiview, |(_, kind, _)| match kind {
            TileKind::Image => Icon::MediaImage,
            TileKind::Video => Icon::MediaVideo,
        });
    icons::to_slint(Some(icon))
}
