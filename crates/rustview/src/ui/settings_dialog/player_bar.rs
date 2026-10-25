//! Settings > Multiview > Player: the control bar of a video. The dialog shows the choices by index, the settings store them
//! by name ("bar"/"pill", "bottom"/"anywhere").

use crate::ui::SettingsWindow;
use rustview_core::settings::schema::{
    FullscreenPlayerSettings, MAX_FULLSCREEN_PLAYER_BAR_HEIGHT_PX, MAX_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT,
    MIN_FULLSCREEN_PLAYER_BAR_HEIGHT_PX, MIN_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT,
};
use rustview_core::util::cast::SaturatingAs;

const STYLES: [&str; 2] = ["bar", "pill"];
const REVEALS: [&str; 2] = ["bottom", "anywhere"];
const SEEKS: [&str; 4] = ["exact", "osc", "keyframes", "mpv"];

fn index_of(names: &[&str], name: &str) -> i32 {
    names
        .iter()
        .position(|n| *n == name)
        .unwrap_or(0)
        .saturating_as::<i32>()
}

fn name_at(names: &[&str], index: i32) -> String {
    usize::try_from(index)
        .ok()
        .and_then(|i| names.get(i))
        .unwrap_or(&names[0])
        .to_string()
}

pub(super) fn populate(window: &SettingsWindow, player: &FullscreenPlayerSettings) {
    window.set_fullscreen_player_style_idx(index_of(&STYLES, &player.bar_style));
    window.set_fullscreen_player_opacity_percent(player.bar_opacity_percent.saturating_as::<i32>());
    window.set_fullscreen_player_height_px(player.bar_height_px.saturating_as::<i32>());
    window.set_fullscreen_player_reveal_idx(index_of(&REVEALS, &player.reveal));
    window.set_fullscreen_player_fade(player.fade);
    window.set_fullscreen_player_show_ms(player.show_ms);
    window.set_fullscreen_player_seek_idx(index_of(&SEEKS, &player.seek));
    window.set_fullscreen_player_limits(crate::ui::PlayerBarLimits {
        min_opacity: MIN_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT.saturating_as::<i32>(),
        max_opacity: MAX_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT.saturating_as::<i32>(),
        min_height: MIN_FULLSCREEN_PLAYER_BAR_HEIGHT_PX.saturating_as::<i32>(),
        max_height: MAX_FULLSCREEN_PLAYER_BAR_HEIGHT_PX.saturating_as::<i32>(),
    });
}

pub(super) fn save(window: &SettingsWindow, player: &mut FullscreenPlayerSettings) {
    player.bar_style = name_at(&STYLES, window.get_fullscreen_player_style_idx());
    player.bar_opacity_percent = u32::try_from(window.get_fullscreen_player_opacity_percent())
        .unwrap_or(MIN_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT)
        .clamp(
            MIN_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT,
            MAX_FULLSCREEN_PLAYER_BAR_OPACITY_PERCENT,
        );
    player.bar_height_px = u32::try_from(window.get_fullscreen_player_height_px())
        .unwrap_or(MIN_FULLSCREEN_PLAYER_BAR_HEIGHT_PX)
        .clamp(MIN_FULLSCREEN_PLAYER_BAR_HEIGHT_PX, MAX_FULLSCREEN_PLAYER_BAR_HEIGHT_PX);
    player.reveal = name_at(&REVEALS, window.get_fullscreen_player_reveal_idx());
    player.fade = window.get_fullscreen_player_fade();
    player.show_ms = window.get_fullscreen_player_show_ms();
    player.seek = name_at(&SEEKS, window.get_fullscreen_player_seek_idx());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_indexes_round_trip_and_unknown_ones_fall_back_to_the_first() {
        for (index, name) in STYLES.iter().enumerate() {
            assert_eq!(index_of(&STYLES, name), index.saturating_as::<i32>());
            assert_eq!(name_at(&STYLES, index.saturating_as::<i32>()), *name);
        }
        assert_eq!(index_of(&REVEALS, "anywhere"), 1);
        assert_eq!(index_of(&REVEALS, "sideways"), 0);
        assert_eq!(name_at(&REVEALS, 5), "bottom");
        assert_eq!(name_at(&REVEALS, -1), "bottom");
        assert_eq!(index_of(&SEEKS, "keyframes"), 2);
        assert_eq!(name_at(&SEEKS, 3), "mpv");
    }
}
