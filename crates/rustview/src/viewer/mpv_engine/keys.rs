//! The keys `RustView` takes from the window of a video (`docs/hotkeys.md`).
//!
//! The native video window has the keyboard while the pointer is over it, so every key of the keymap that `RustView` handles
//! itself is bound inside mpv to `script-message rustview-key <chord>` and the application resolves the chord for the host
//! of that player, like a key of its own windows. The bindings live in the input section `rustview_keys` (forced, so they
//! win over mpv's own bindings); the keymap's bridge sets them with [`set_bindings`], which also redefines the section of
//! every running player. Keys that are not in it (the video player keys) stay mpv's until `RustView` controls the videos.

use libmpv2::Mpv;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};

/// The message mpv sends for a key of the keymap: `rustview-key <chord text>`.
pub(super) const KEY_MESSAGE: &str = "rustview-key";

const SECTION: &str = "rustview_keys";

/// The section's contents and every running player that has it.
struct Registry {
    contents: String,
    players: Vec<Weak<Mpv>>,
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry {
    contents: String::new(),
    players: Vec::new(),
});

/// The section for `bindings` (mpv key name, chord text): one `input.conf` line each.
fn section(bindings: &[(String, String)]) -> String {
    let mut text = String::new();
    for (key, chord) in bindings {
        text.push_str(key);
        text.push_str(" no-osd script-message ");
        text.push_str(KEY_MESSAGE);
        text.push(' ');
        text.push_str(chord);
        text.push('\n');
    }
    text
}

fn define(mpv: &Mpv, contents: &str) {
    // An empty section would be removed (and lose its place on the stack); a comment keeps it.
    let contents = if contents.is_empty() { "# no keys\n" } else { contents };
    if let Err(e) = mpv.command("define-section", &[SECTION, contents, "force"]) {
        tracing::warn!(error = ?e, "[MpvPlayer: Keys] Could not define the key section");
    }
}

/// Gives a new player the current keys and keeps it for later changes.
pub(super) fn install(mpv: &Arc<Mpv>) {
    let mut registry = REGISTRY.lock();
    define(mpv, &registry.contents);
    if let Err(e) = mpv.command("enable-section", &[SECTION, "allow-hide-cursor+allow-vo-dragging"]) {
        tracing::warn!(error = ?e, "[MpvPlayer: Keys] Could not enable the key section");
    }
    registry.players.retain(|p| p.strong_count() > 0);
    registry.players.push(Arc::downgrade(mpv));
}

/// The keys every player reports from now on (mpv key name, chord text), applied to the running players at once.
pub fn set_bindings(bindings: &[(String, String)]) {
    let mut registry = REGISTRY.lock();
    registry.contents = section(bindings);
    registry.players.retain(|p| p.strong_count() > 0);
    let players: Vec<Arc<Mpv>> = registry.players.iter().filter_map(Weak::upgrade).collect();
    for mpv in &players {
        define(mpv, &registry.contents);
    }
    tracing::info!(
        keys = bindings.len(),
        players = players.len(),
        "[MpvPlayer: Keys] Keys of the video windows updated"
    );
}

/// The chord of a key message of mpv, `None` when it is another message.
pub(super) fn chord_of_message<'a>(args: &[&'a str]) -> Option<&'a str> {
    match args {
        [KEY_MESSAGE, chord] => Some(chord),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binding_is_one_line_with_the_chord_as_the_message_argument() {
        let text = section(&[
            ("Ctrl+s".to_owned(), "Control+S".to_owned()),
            ("F2".to_owned(), "F2".to_owned()),
        ]);
        assert_eq!(
            text,
            "Ctrl+s no-osd script-message rustview-key Control+S\nF2 no-osd script-message rustview-key F2\n"
        );
    }

    #[test]
    fn only_key_messages_carry_a_chord() {
        assert_eq!(chord_of_message(&["rustview-key", "Control+S"]), Some("Control+S"));
        assert_eq!(chord_of_message(&["rustview-key"]), None);
        assert_eq!(chord_of_message(&["rustview-click"]), None);
    }
}
