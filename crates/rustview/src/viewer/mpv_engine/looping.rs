//! How a video repeats (user decision 2026-10-26): always with `loop-file`, never with `loop-playlist`, whatever the user's
//! `mpv.conf` says. Every player of `RustView` plays one file, and `loop-playlist` repeats it by loading it anew at its end
//! (`end-file`, `start-file`, `file-loaded`: the duration and the script's state go and come back, the saved A-B repeats on the
//! progress bar with them), while `loop-file` only seeks to its start. `loop-file=inf` alone would already win over
//! `loop-playlist`; switching the latter off keeps a finite `loop-file=N` from loading the file again after its N repeats.
//!
//! The script `ab_repeat.lua` sets `loop-file` itself: `no` while an A-B loops, and back to the value it read when it started
//! once the points are cleared. mpv starts the scripts at the end of `mpv_initialize`, so that value may be the one of the
//! `mpv.conf` or this one; after every state the script publishes, a video that no A-B loops gets `RustView`'s value again.

use libmpv2::Mpv;
use std::sync::Arc;

/// The `loop-file` value a player uses for the configured `loop-file` and `loop-playlist` (`None`: the video does not repeat).
pub(super) fn wanted(loop_file: &str, loop_playlist: &str) -> Option<String> {
    let repeats = |value: &str| !matches!(value, "" | "no" | "0");
    if repeats(loop_file) {
        return Some(loop_file.to_owned());
    }
    // `force` only differs from `inf` for playlists of files that cannot be played.
    repeats(loop_playlist).then(|| if loop_playlist == "force" { "inf" } else { loop_playlist }.to_owned())
}

/// Turns the repeat of the user's configuration into `loop-file` (once the configuration was read). Returns the value.
pub(super) fn apply(mpv: &Mpv) -> Option<Arc<str>> {
    let read = |name: &str| mpv.get_property::<String>(name).unwrap_or_default();
    let (loop_file, loop_playlist) = (read("loop-file"), read("loop-playlist"));
    let wanted = wanted(&loop_file, &loop_playlist);
    if loop_playlist != "no"
        && let Err(e) = mpv.set_property("loop-playlist", "no")
    {
        tracing::warn!(error = ?e, "[MpvPlayer: Loop] loop-playlist could not be switched off");
    }
    if let Some(value) = wanted.as_deref()
        && value != loop_file
        && let Err(e) = mpv.set_property("loop-file", value)
    {
        tracing::warn!(error = ?e, "[MpvPlayer: Loop] loop-file could not be set");
    }
    tracing::info!(
        configured_loop_file = %loop_file,
        configured_loop_playlist = %loop_playlist,
        loop_file = wanted.as_deref().unwrap_or("no"),
        "[MpvPlayer: Loop] Videos repeat with loop-file"
    );
    wanted.map(Arc::from)
}

/// Whether `loop-file` has to be set back to `wanted`: no A-B loops (the script switches it off for its loop) and it differs.
pub(super) fn needs_restore(wanted: Option<&str>, current: &str, ab_looping: bool) -> bool {
    wanted.is_some_and(|wanted| !ab_looping && current != wanted)
}

/// The script published its state: a video that no A-B loops repeats as `RustView` set it up again.
pub(super) fn keep(mpv: &Mpv, wanted: Option<&str>, ab_looping: bool) {
    let Some(value) = wanted else {
        return;
    };
    let current = mpv.get_property::<String>("loop-file").unwrap_or_default();
    if !needs_restore(Some(value), &current, ab_looping) {
        return;
    }
    match mpv.set_property("loop-file", value) {
        Ok(()) => {
            tracing::info!(from = %current, to = value, "[MpvPlayer: Loop] loop-file set back after the A-B script");
        }
        Err(e) => tracing::warn!(error = ?e, "[MpvPlayer: Loop] loop-file could not be set back"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_playlist_becomes_loop_file() {
        assert_eq!(wanted("no", "inf").as_deref(), Some("inf"));
        assert_eq!(wanted("no", "force").as_deref(), Some("inf"));
        assert_eq!(wanted("no", "3").as_deref(), Some("3"));
        assert_eq!(wanted("2", "inf").as_deref(), Some("2"), "a loop-file of the user wins");
        assert_eq!(wanted("inf", "no").as_deref(), Some("inf"));
        assert_eq!(wanted("no", "no"), None, "a video that does not repeat stays so");
    }

    #[test]
    fn the_loop_comes_back_once_no_a_b_loops() {
        assert!(needs_restore(Some("inf"), "no", false), "the script cleared the points");
        assert!(
            !needs_restore(Some("inf"), "no", true),
            "an A-B loops: the script's own no"
        );
        assert!(!needs_restore(Some("inf"), "inf", false));
        assert!(!needs_restore(None, "no", false), "no repeat configured");
    }
}
