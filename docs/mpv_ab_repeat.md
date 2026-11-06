# Support for the mpv script ab_repeat.lua (built in 0.5.4)

The user's mpv script `~/.config/mpv/scripts/ab_repeat.lua` sets A-B repeats and saves, loads and deletes them in
`~/.config/mpv/ab_repeat.json`. RustView's players load the user's mpv config and scripts (`MpvPlayer::new`: `config=yes`,
`load-scripts=yes`, `config-dir` = `$XDG_CONFIG_HOME/mpv`), so the script already runs in every RustView player, but
`input-vo-keyboard=no` means its keys never reach it. The design was decided by the maintainer on 2026-10-25; the decisions, how it is built and what was tested before
follow.

Tested with mpv 0.41.0 (libmpv of the test machine), the test machine's real `~/.config/mpv` (test-only, so as close to production as possible),
native mpv (CLI, `--input-ipc-server`, real X11 keys through `xdotool`) and RustView 0.5.3 in a local test environment
(the real scripts are linked into the isolated test profile).

## Decisions (questionnaire 2026-10-25)

**The script (the user's own file, not part of this repository):**
- RustView talks to an interface in `ab_repeat.lua` (not to emulated keys). The interface is generic (it never names RustView),
  explained in the script and switchable by an option (`api`, default on). The extended script replaces the user's file in
  `~/.config/mpv/scripts/` (backup next to it); it is **not** put into this repository, the user distributes it.
- Identity: mpv has no script registry (the client name is the file name, `mp.get_script_name()`), so the script uses the two
  documented mechanisms: `options.read_options(CONFIG, "ab_repeat")` with a fixed identifier (`script-opts/ab_repeat.conf` and
  `--script-opts=ab_repeat-...` whatever the file is called) and an identity record at the fixed path `user-data/ab_repeat`
  (name, version, interface version, the client name to send messages to). RustView recognizes the script by that record, not by
  its file name.
- An old script without the interface is not supported: the settings card shows it as an error, the switch is disabled, and a
  re-check button looks again without restarting RustView.
- Script fixes: B before A makes the range invalid (no loop, AB inactive; "hanging" is gone), `CONFIG` through `script-opts`. No
  other script changes (umlauts in mpv's prompt, keypad Enter, atomic writes, a cycle action and the prompt timeout stay as
  they are). mpv's `l` stays mpv's own A-B loop. The Ctrl+S problem is no longer relevant (user).
- While a repeat is active, seeking outside A-B is forced back to A by the script itself (RustView does nothing special).
- The AB button switches the loop of the current, valid A-B on and off without clearing it; holding it (hold to confirm) clears
  A and B. So the script gets a loop on/off state of its own.

**Settings (Settings > Multiview, card "mpv script ab_repeat.lua"):** look for the script (default on; status line, error
for an incompatible script, re-check button; extra information in a tooltip, not as text); show the script's OSD messages (one
switch for all of them; on = the script's own `script-opts`, off = RustView turns them off for its players); colors of saved
repeats (each its own color = default, or all grey); playbar interactions (click a lane loads it, hover shows the name, hover
shows the times (off by default), drag the A/B flags; each a switch, the first three of these on); load menu paging (pages of N,
default 10, or a scroll list showing at least ~5). Settings > Debug gets a check. Since 0.5.5 the card is on the Multiview page
(formerly "Fullscreen") with the other player settings; the values stay under `viewer.videos.ab_repeat` of settings.json.

**Control bar (fullscreen and tab view, not the tab grid):** a group in its own capsule between the remaining time and the
sound: A (`brackets-contain-start`), B (`brackets-contain-end`), AB (an own SVG: repeat arrows with "AB" exactly centered), Save
(`playlist-add`), Load (`list-details`). A and B are yellow when set; AB is yellow while the loop plays, blue when a valid A-B
could play, disabled (no hover, crossed out) without a valid A-B. The Load button shows the number of saved repeats as a badge
and is disabled at 0.

**Playbar:** saved repeats as one thin row of colored, slightly translucent segments above the progress bar (overlaps show as
stronger color), shaped like a Chrome tab: only as wide as the progress bar, with curved flanks into the bar; height a third of
the bar, at least 12 px; every repeat is shown. Yellow is reserved for the active A-B and never in the palette; a saved
repeat keeps its color for the whole run (deleting or adding others never reshuffles). The active A-B: A/B flags with times
(label left/right/centered depending on the room), the part A-B bright and the rest darker, a point the script set by itself
(automatic B at the end, automatic A at 0) dashed. Hover over a segment shows a small label with the name (and the times when
switched on) that follows the pointer; a click loads it; the flags can be dragged.

**Save:** a bubble above the Save button with a text field and a save button only; the field has the focus at once and shows
the script's default name ("Range N") as placeholder; Enter saves, Esc, a click outside or a second click on Save cancels; Save
is yellow while the bubble is open; a hint below the field appears only while the name or the range already exists.

**Load menu:** speech-bubble menu like Open With: search field (filters while typing), number, color stripe, name, start -> end,
length, a mini bar per row, the running repeat highlighted, rename and delete (hold to confirm) per row, "delete all" (hold) at
the bottom, pages remembered per video while RustView runs; order and numbers as in the script (save order). Picking an entry
jumps to A and plays.

**Keys:** a hotkey area "MPV ab_repeat.lua" (only while the script is active; conflicts checked with the other keys of a
selected video): set A = Home, set B = End, clear = Ctrl+Delete; save, load menu, jump to A, next/previous saved repeat
without defaults. Keys act on the selected video only.

**Several players:** every video has its own player, script and A-B; the saved list of a file is kept in sync between all
players showing it, event-driven (a change in one player makes RustView ask the others to re-read; no timers); the current A-B
and its on/off state are never touched by that. Another file in the same player clears A-B (script). Renaming or moving a
video in RustView moves its saved repeats along; deleting keeps them; paths through links stay as the script keys them.

**Reveal:** user seeks (keys, clicks) and the A-B keys show the video control bar briefly, like the volume bar; the script's own
jumps of an active loop do not.

**Side finding:** RustView turns mpv's OSC off again after the user's `mpv.conf` was read (a separate commit).

## How it is built (0.5.4)

**The script (2.0, the user's file `~/.config/mpv/scripts/ab_repeat.lua`; the 1.x original is kept as
`~/.config/mpv/ab_repeat.lua.orig-2026-10-25`).** Its keys and menus in mpv are unchanged. New, all described at its top:
`options.read_options(CONFIG, "ab_repeat")` (so `script-opts/ab_repeat.conf` and `--script-opts=ab_repeat-...` work, with
runtime updates), the options `osd_messages` (a master switch for its OSD texts) and `api` (the interface, default on), the
identity `user-data/ab_repeat/info` {name, version, api 1, client, db, config}, the state `user-data/ab_repeat/state` {a, b,
a_auto, b_auto, valid, loop, active, busy}, the saved list `user-data/ab_repeat/ranges` {path, revision, cause, list}, a loop
on/off of its own (`toggle-loop`, `loop on|off`), B not after A = invalid (no loop), seeking outside A-B while it loops is pulled
back to A, and the commands `set-a [t]`, `set-b [t]`, `clear`, `jump-a`, `save [name]`, `load|delete <index> <a> <b>`,
`rename <index> <a> <b> <name>`, `delete-all`, `next-range`, `prev-range`, `reload`, `move-path <old> <new>` and
`ping <token>` (a barrier: `user-data/ab_repeat/pong`). New key bindings without keys: `toggle-loop`, `jump-to-a`,
`next-range`, `prev-range`.

**RustView.** `viewer/ab_repeat/` (protocol, `support` = which script counts as compatible and what was found last, stable
`colors`, `prefs`, `moves`), `viewer/mpv_engine/ab_repeat.rs` (observes the three paths as JSON strings, decides the support
at the first loaded file, sends commands, forwards a saved-list change of one player to every other player of the file as
`reload`, `probe` = a headless mpv for Check again, `move_paths`), the bar (`docs/video_controls.md`), the keys
(`docs/hotkeys.md`), Settings > Multiview (`settings_card_ab_repeat.slint`, `settings_dialog/ab_repeat_card.rs`,
`AbRepeatSettings`), a check on Settings > Debug (`health::ab_repeat_check`) and the move observer of the library
(`file_operations::set_move_observer`): a renamed or moved file whose path has saved repeats has them moved by the script
(through a running player or a headless mpv; RustView reads the script's JSON for that check only, on that explicit action, and
never writes it). The script shows no OSD texts in RustView's players (user decision 2026-10-26, the control bar shows
what happens): mpv's OSD is off (`osd-level=0`) and `ab_repeat-osd_messages=no` is appended to the players' `script-opts`; mpv
itself is not changed. Not covered: renamed folders (their files keep their old keys) and paths
through symbolic links (the script keys them as given).

## Checked with a compositing manager (0.5.6, 2026-10-26)

KWin compositing is on on the test machine since 2026-10-26, so the translucent bar was checked as on the user's PCs (MCP build, two tiles of
`Videos/Clips/hd_720_mkv.mkv` in one tab through Duplicate, the real `~/.config/mpv`):

| Check | Result |
|---|---|
| Save in the left player | the right one shows it at once: Load enabled with badge 1, the segment in the tab; its A and B stay unset |
| Rename in the right player's menu | the left menu shows the new name at once, same color |
| Delete in the left menu (holding) | the right player: Load disabled, no badge, no tab |
| Save again | both enabled again; a new color, the old ones are not reshuffled |
| Name under the pointer | the label follows the pointer over the segment |
| Click on a menu row, hold to delete | works with the real mouse since 0.5.6 (a press inside the popup was tested against the host's coordinates and closed it) |
| Short click on the video | closes the menu (the view's own click, not only the 50 ms poll) |
| Tab and bar | no seam any more; the tab shows only with a saved repeat, A or B |
| Without a compositor (KWin's "Suspend Compositing" for the check) | solid bar, the tab a plain rectangle over the progress bar, menu fine |

Times of the A-B group (flags, menu, label) stay in whole seconds like the rest of the bar (milliseconds with the bar's own
switch); lengths are rounded, not cut off (user decision 2026-10-26: no tenths of a second).

The sync needs no timer: the player that wrote publishes `ranges` with cause `write`, its observer thread sends `reload` to every
other player of the file, their scripts publish again (`reload_others` in `viewer/mpv_engine/ab_repeat.rs`).

## The script (as found, 1.x)

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

Not reproducible on the test machine with the copied config: Ctrl+S opens the name prompt, Enter saves (real X11 keys and mpv's
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

## A small API in the script (the prototype that led to 2.0)

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
  but it runs. Fixed separately: `MpvPlayer::new` switches it off again after the config was read (unloads the script).
- Until 0.5.9 `MpvPlayer::set_osd_text` set `osd-border-style=background-box` for the tab grid's time badge, which then also
  boxed the script's OSD texts in that player. Since 0.5.10 the badge is a Slint element and RustView leaves mpv's OSD style alone.

## Test tools (outside the repository)

- The maintainer's local test environment can link the real mpv config into the isolated test profile.
- A local generator script (not in the repository): 52 sample repeats in 10 test videos, created through the script itself
  (start, end, middle, overlapping, nested, identical, touching, 0.2 s, whole video, 26 for paging, an accented path, the four
  Multiview videos); reruns replace them.
