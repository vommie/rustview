//! The options every player of `RustView` starts mpv with. mpv has no window and no input of its own here: the video is drawn
//! through the render API into a texture of the Slint window, and `RustView` handles every key, click and wheel over a video,
//! draws the control bar and its messages, and drives mpv with commands (`mpv_command` never shows mpv's OSD). The user's
//! `mpv.conf` and scripts are loaded (the script `ab_repeat.lua` is part of the player, `docs/mpv_ab_repeat.md`); what in them
//! would collide with `RustView`'s own interface is set back after the configuration was read (`after_init`): mpv shows nothing
//! over a video (no OSD at all, scripts' messages included: the control bar shows what happens) and takes no input
//! (user decisions 2026-10-26, questionnaire `2026-10-26-rustview-mpv-einstellungen`; the cache and everything else of the
//! user's `mpv.conf` stay as configured).

use super::ab_repeat;
use libmpv2::{Mpv, MpvInitializer};

/// mpv's built-in scripts that only act on keys or the pointer (statistics, console, command prompt, selection lists,
/// panning, context menu): mpv never gets either here, so they would only cost start time and memory in every player.
const UNUSED_BUILTIN_SCRIPTS: [&str; 6] = [
    "load-stats-overlay",
    "load-console",
    "load-commands",
    "load-select",
    "load-positioning",
    "load-context-menu",
];

/// What `RustView` always sets, also over the user's `mpv.conf` (set before and again after `mpv_initialize`).
const FORCED: [(&str, &str); 12] = [
    // No OSD at all: no bars, no seek, playing or status messages, no messages of scripts (`show-text` needs level 1).
    ("osd-level", "0"),
    ("osd-bar", "no"),
    ("osd-on-seek", "no"),
    ("osd-playing-msg", ""),
    ("osd-msg1", ""),
    // No input of its own: no built-in or input.conf key bindings (scripts' forced bindings and `script-message` stay), no
    // keyboard, pointer, terminal or media keys.
    ("input-default-bindings", "no"),
    ("input-vo-keyboard", "no"),
    ("input-cursor", "no"),
    ("input-terminal", "no"),
    ("input-media-keys", "no"),
    // No on-screen controller: RustView draws a control bar of its own (`viewer::multiview::video_bar`); `osc=yes` in mpv.conf
    // would load mpv's controller again, which never sees the pointer but runs.
    ("osc", "no"),
    // Local files only: the youtube-dl hook would start a script in every player for nothing.
    ("ytdl", "no"),
];

/// Options set before `mpv_initialize` (the user's `mpv.conf` is read afterwards and may override them).
pub(super) fn before_init(init: &MpvInitializer) {
    let set = |name: &str, value: &str| {
        if let Err(e) = init.set_property(name, value) {
            tracing::warn!(option = name, error = ?e, "[MpvPlayer: Init] mpv did not take an option");
        }
    };
    // Rendering: mpv's render API (the video surface's render thread draws every frame).
    set("vo", "libmpv");
    set("hwdec", crate::viewer::multiview::video_filter::mpv::HWDEC_DEFAULT);
    // Playback lifecycle: a finished video stays on its last frame, a player without a file waits.
    set("keep-open", "yes");
    set("idle", "yes");
    for (name, value) in FORCED {
        set(name, value);
    }
    // Only read at the start: the built-in bindings are never loaded, and the user's input.conf is replaced by an empty one.
    set("input-builtin-bindings", "no");
    set("input-conf", "memory://");
    // The user's configuration and scripts (`ab_repeat.lua`); conditional profiles only load their script when there are any.
    set("config", "yes");
    set("load-scripts", "yes");
    set("load-auto-profiles", "auto");
    for script in UNUSED_BUILTIN_SCRIPTS {
        set(script, "no");
    }

    if let Some(config_dir) = super::self_test::user_config_dir() {
        set("config-dir", config_dir.to_string_lossy().as_ref());
        tracing::info!(
            config_dir = %config_dir.display(),
            "[MpvPlayer: Init] Loaded user MPV configuration and plugin directory"
        );
    }
}

/// Sets back what the user's `mpv.conf` changed and `RustView` cannot allow, once the configuration was read.
pub(super) fn after_init(mpv: &Mpv) {
    let mut overridden = Vec::new();
    for (name, value) in FORCED {
        if mpv.get_property::<String>(name).is_ok_and(|current| current != value) {
            match mpv.set_property(name, value) {
                Ok(()) => overridden.push(name),
                Err(e) => {
                    tracing::warn!(option = name, error = ?e, "[MpvPlayer: Init] mpv did not take an option back");
                }
            }
        }
    }
    if !overridden.is_empty() {
        tracing::info!(
            options = ?overridden,
            "[MpvPlayer: Init] Options of the user's mpv configuration set back to RustView's"
        );
    }
    ab_repeat::mute_script_messages(mpv);
}
