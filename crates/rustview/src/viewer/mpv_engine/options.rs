//! The options every player of `RustView` starts mpv with. mpv has no window and no input of its own here: the video is drawn
//! through the render API into a texture of the Slint window, and `RustView` handles every key, click and wheel over a video,
//! draws the control bar and its messages, and drives mpv with commands (`mpv_command` never shows mpv's OSD). The user's
//! `mpv.conf` and scripts are loaded (the script `ab_repeat.lua` is part of the player, `docs/mpv_ab_repeat.md`); what in them
//! would collide with `RustView`'s own interface is set back after the configuration was read (`after_init`).

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

/// Options set before `mpv_initialize` (the user's `mpv.conf` is read afterwards and may override them).
pub(super) fn before_init(init: &MpvInitializer) {
    let set = |name: &str, value: &str| {
        if let Err(e) = init.set_property(name, value) {
            tracing::warn!(option = name, error = ?e, "[MpvPlayer: Init] mpv did not take an option");
        }
    };
    // Rendering: mpv's render API (the video surface's render thread draws every frame).
    set("vo", "libmpv");
    set("hwdec", "auto-safe");
    // Playback lifecycle: a finished video stays on its last frame, a player without a file waits.
    set("keep-open", "yes");
    set("idle", "yes");
    // No on-screen controller: RustView draws a control bar of its own (`viewer::multiview::video_bar`).
    set("osc", "no");
    // The user's configuration and scripts (`ab_repeat.lua`); conditional profiles only load their script when there are any.
    set("config", "yes");
    set("load-scripts", "yes");
    set("load-auto-profiles", "auto");
    for script in UNUSED_BUILTIN_SCRIPTS {
        set(script, "no");
    }
    // Local files only: the youtube-dl hook would start a script in every player for nothing.
    set("ytdl", "no");

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
    // `osc=yes` in mpv.conf loads mpv's controller again, which never sees the pointer but runs; setting it now unloads it.
    if mpv.get_property::<bool>("osc").unwrap_or(false) {
        match mpv.set_property("osc", false) {
            Ok(()) => tracing::info!(
                "[MpvPlayer: Init] mpv's on-screen controller switched off again (the user's mpv.conf had it on)"
            ),
            Err(e) => {
                tracing::warn!(error = ?e, "[MpvPlayer: Init] mpv's on-screen controller could not be switched off");
            }
        }
    }
    ab_repeat::apply_script_opts(mpv);
}
