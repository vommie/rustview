//! The icon of a viewer tab: one place decides it for the tab bar, the drag ghost and the tab grid. A tab that holds a
//! Multiview of one medium shows the icon of that medium.

use crate::icons::{self, Icon};
use crate::ui::IconId;
use crate::viewer::multiview::model::TileKind;
use crate::viewer::{MediaTabSession, TabContentType};

pub fn tab_icon(tab: &MediaTabSession) -> IconId {
    let icon = match tab.kind() {
        TabContentType::Image => Icon::MediaImage,
        TabContentType::Video => Icon::MediaVideo,
        TabContentType::Multiview => tab
            .multiview()
            .and_then(|mv| mv.instance.lock().solo_tile())
            .map_or(Icon::OpenMultiview, |(_, kind, _)| match kind {
                TileKind::Image => Icon::MediaImage,
                TileKind::Video => Icon::MediaVideo,
            }),
    };
    icons::to_slint(Some(icon))
}
