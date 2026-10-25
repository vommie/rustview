# Support for the mpv script ab_repeat.lua (investigation, 2026-10-25)

The user's mpv script `~/.config/mpv/scripts/ab_repeat.lua` sets A-B repeats and saves, loads and deletes them in
`~/.config/mpv/ab_repeat.json`. RustView's players load the user's mpv config and scripts (`MpvPlayer::new`: `config=yes`,
`load-scripts=yes`, `config-dir` = `$XDG_CONFIG_HOME/mpv`), so the script already runs in every RustView player, but
`input-vo-keyboard=no` means its keys never reach it. This page holds what was tested before the feature is designed; the
design questions are in the questionnaire `2026-10-25-rustview-ab-repeat` (`~/claude/feedback/questionnaires/rustview/`). Nothing
is implemented yet.

Tested with mpv 0.41.0 (libmpv of the VM), the VM's real `~/.config/mpv` (test-only, so as close to production as possible),
native mpv (CLI, `--input-ipc-server`, real X11 keys through `xdotool`) and RustView 0.5.3 in the test environment
(`rv-testenv mpv-user on --ipc SOCKET` links the real scripts into the test profile).

## The script

- Client name `ab_repeat` (from the file name). Bindings (all `mp.add_key_binding`, i.e. weak: `input.conf` beats them, mpv's
  built-in defaults do not): `set-a-point` Home, `set-b-point` End, `reset-points` Del, `save-ab-range` Ctrl+s,
  `load-ab-range-menu` Ctrl+l, `delete-ab-range-menu` Ctrl+Del. They override mpv's own Home (`seek 0`), Del (`osc/visibility`)
  and Ctrl+s (`screenshot window`).
- A and B are Lua locals; nothing is published (no `user-data`, no script messages, no `script-opts`; `CONFIG` is hard-coded).
- The loop is the script's own, not mpv's `ab-loop-a/b`: it observes `time-pos` and seeks to A (`absolute+exact`) as soon as the
  position is at or after B (in the last second of the file 0.1 s before B), plus an `end-file` fallback. Consequences (all
  measured): setting B jumps back to A at once; seeking behind B lands on A; seeking before A plays into the loop; with
  `keep-open=yes` (RustView) and without it the loop wraps before the end of the file.
- `CONFIG` (all honored, measured): `auto_set_b_point` (A alone: B = duration - 1/fps), `auto_set_a_point` (B alone: A = 0),
  `reset_on_new_file` (another file clears A/B; coming back does not restore them), `show_stored_ranges_message` ("N stored AB
  range(s) found for this video" on every load; the script reads the JSON for it on every `file-loaded`), `menu_display_duration`
  10 s (menus close, the name prompt cancels 10 s after the last key), `entries_per_page` 10 (keys 1-9 and a; Page Up/Down),
  `show_osd_messages`, empty name = "Range N".
- While a menu or the name prompt is open (`menu_active`) set A/B, clear, save and the menus do nothing.
- Storage: `$HOME/.config/mpv/ab_repeat.json` (from `HOME`/`APPDATA`, not from `--config-dir`), one array per video:
  `{"<path>": [{"name": "...", "a_point": 10.0, "b_point": 18.0}, ...]}`. The key is mpv's `path` property: mpv 0.41 makes
  relative paths and `file://` URLs absolute, but symbolic links are not resolved (the same file through a link has its own
  list). RustView passes absolute library paths to `loadfile`. Every save/delete reads the whole file, changes it and writes it
  back (`io.open(.., "w")`, not atomic); two players saving at the same time can lose one change.

## Bugs and limits of the script (found by the tests)

- **B before A freezes playback:** A = 30, then B set at 20: the position (20) is at B, the script seeks to A (30), which is
  after B again, and so on; the video stays at 30.0 s until the points are cleared. (A after B cannot happen through the keys:
  the loop pulls the position back before it.)
- The name prompt only binds ASCII letters, digits, space and ASCII punctuation: umlauts, ß and accents are dropped silently
  (the keys U, ä, b, e, r, g, ä, n, g, ß were saved as "Ubergng"; mpv logs `No key binding found for key 'ä'`). The numpad Enter (`KP_ENTER`) does not
  confirm.

## Ctrl+S in native mpv

Not reproducible in the VM with the copied config: Ctrl+S opens the name prompt, Enter saves (real X11 keys and mpv's
`keydown`/`keyup` alike). `input-bindings` shows the script's Ctrl+s with priority 13 over the built-in `screenshot window`
(0); the user's `input.conf` does not bind Ctrl+s. Causes that were reproduced:

1. **A-B set with mpv's own `l` key** (built-in `ab-loop`, OSD "A-B loop start" / "A-B loop: 00:00:20 - 00:00:25"): the script
   knows nothing of mpv's loop, Ctrl+S answers "No valid AB range to save". The script's points are set with Home/End.
2. **Numpad Enter** confirms nothing; the prompt cancels 10 s later ("Save cancelled").
3. More than 10 s without a key in the prompt cancels it.
4. Umlauts in the name are dropped (see above), and Ctrl+L on a path through a symbolic link shows another list.

Not checkable from here: another `input.conf` or another script on the user's PCs that binds Ctrl+s (an `input.conf` binding
or a forced script binding beats the script's weak one). Check on the PC: `mpv --log-file=/tmp/ab.log VIDEO`, press Home, End,
Ctrl+S, type a name, Enter, quit, then `grep -E 'ab_repeat|Run command: (script-binding|screenshot)' /tmp/ab.log`: a
`screenshot` line or a binding of another owner instead of `ab_repeat/save-ab-range` names the conflict.

## Detecting the script

- **`input-bindings`** (read-only, no change notification): entries with `owner == "ab_repeat"` and the six binding names
  above. 330 entries, read in ~5 ms over IPC (less through libmpv). Works with the unchanged script.
- **Timing:** mpv waits for every script's main chunk before it starts playback (`lua.rst`, "the player will wait until the
  script is fully loaded"); an idle mpv answered with all six bindings at its first IPC request, so one read right after
  `mpv_initialize` (or at the first `file-loaded`) per player is enough; no polling.
- `script-message-to ab_repeat <anything>` fails with "error running command" when no client of that name exists (a cheap
  existence probe, but it does not prove it is this script).
- The log message `[ab_repeat] AB Repeat Script initialized` (info) also shows it.
- `user-data` holds only the OSC's entries: the unchanged script publishes nothing.

## Controlling the script

- **`script-binding ab_repeat/<name>`** runs the same function as the key (the documented way to remap script keys): set A,
  set B, clear, and opening the save prompt and the menus. Works the same in native mpv and in RustView's players
  (`input-vo-keyboard=no` does not matter: the command does not come from the video window).
- **The name prompt and the menus need key events:** `keydown NAME` + `keyup NAME` (what a keyboard sends). `keypress` gives a
  "press" event, which the script's prompt ignores (nothing is saved). Tested: save with a typed name, load an entry (also on
  page 3 after two Page Down), delete one entry, delete all (key 0), in native mpv and in RustView.
- **Two traps of key emulation:** (1) keys sent before the script has registered its prompt/menu keys go to mpv's normal
  bindings and are lost; wait until `input-bindings` shows `ab_repeat/input_enter` (prompt), `select_range_1` (menu) or
  `delete_all_ranges`. (2) mpv keeps only 7 queued key events (`--input-key-fifo-size`, default 7) and drops the rest silently:
  typing a name at full speed lost characters and even the Enter in 3 of 5 runs; with a larger FIFO 6 of 6 runs were clean.
- The script's OSD texts (A/B set, the name prompt, the menus, "Saved AB range: ...", "N stored AB range(s) found") are drawn by
  mpv inside RustView's video window (seen in the test screenshots).

## A small API in the script (prototype, scratch copy only)

A scratch copy of the script with ~80 added lines was tested (the user's script is unchanged; extending it needs the user's
OK): `user-data/ab_repeat/api` (version + the `CONFIG` table), `user-data/ab_repeat/state` (`a`, `b`, `looping`, `busy`),
`user-data/ab_repeat/ranges` (the list of the current file, published at `file-loaded` and after every write), and the script
messages `ab-set-a`, `ab-set-b`, `ab-clear`, `ab-save <name>`, `ab-load <index>`, `ab-delete <index>`, which call the same
functions as the keys. Observed over IPC: every change arrives as a `property-change` event whether it came from a native key
(Home) or a message; names with umlauts, ß and en dashes are saved; loading seeks to A;
another file publishes its list. libmpv2 6 cannot hand a node in a property event (`PropertyData` has no node variant), but
observing `user-data/...` as a string gives its JSON (mpv converts nodes to JSON strings), so RustView needs no polling and no
JSON reading of its own.

## RustView-specific observations

- The script's own keys never fire in RustView (`input-vo-keyboard=no`), and its defaults would collide with RustView's keys
  anyway (Delete deletes the medium, Ctrl+S saves an image edit).
- Every tile has its own player and therefore its own script instance and its own A/B; a duplicate of a video loops
  independently.
- The user's `mpv.conf` beats the options `MpvPlayer::new` sets before `mpv_initialize`: with `osc=yes` in it, RustView's players
  run with `osc=true` (seen in the test; RustView sets `osc=no`). The OSC stays invisible because mpv never sees the pointer,
  but it runs. Not part of this feature; noted for a follow-up.
- `MpvPlayer::set_osd_text` sets `osd-border-style=background-box` for the tab grid's time badge, which then also boxes the
  script's OSD texts in that player.

## Test tools (outside the repository)

- `rv-testenv mpv-user on [--ipc SOCKET] | off | status` (test environment): the real mpv config in the test profile.
- `~/claude/rustview-testenv/bin/gen_ab_repeats.py`: 52 sample repeats in 10 test videos, created through the script itself
  (start, end, middle, overlapping, nested, identical, touching, 0.2 s, whole video, 26 for paging, an accented path, the four
  Multiview videos); reruns replace them.
