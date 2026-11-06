//! The video filters of a tile in its player (`viewer::multiview::video_filter`): properties are set, `RustView`'s labeled
//! filters are removed from and added to `vf`/`af` by label, so the filters of the user's mpv.conf stay.

use super::MpvPlayer;
use crate::viewer::multiview::video_filter::mpv::MpvSetting;

impl MpvPlayer {
    /// The rotation the file asks for (`video-dec-params/rotate`: without `RustView`'s own rotation), 0 while unknown. Read
    /// only when a filter changes.
    pub fn file_rotation(&self) -> i64 {
        self.mpv.get_property::<i64>("video-dec-params/rotate").unwrap_or(0)
    }

    /// Applies `settings` (the changes of a tile's filters). A failure is logged and the rest goes on.
    pub fn apply_filter_settings(&self, settings: &[MpvSetting]) {
        for setting in settings {
            let result = match setting {
                MpvSetting::Int(name, value) => self.mpv.set_property(name, *value),
                MpvSetting::Text(name, value) => self.mpv.set_property(name, value.as_str()),
                MpvSetting::Chain {
                    chain,
                    label,
                    filter,
                    present,
                } => {
                    if *present && let Err(e) = self.mpv.command(chain, &["remove", label]) {
                        tracing::warn!(chain, error = ?e, "[MpvPlayer: Filters] The old filter could not be removed");
                    }
                    match filter {
                        Some(filter) => self.mpv.command(chain, &["add", &format!("{label}:{filter}")]),
                        None => Ok(()),
                    }
                }
            };
            match result {
                Ok(()) => tracing::debug!(?setting, "[MpvPlayer: Filters] Set"),
                Err(e) => tracing::warn!(?setting, error = ?e, "[MpvPlayer: Filters] mpv did not take a filter"),
            }
        }
    }
}
