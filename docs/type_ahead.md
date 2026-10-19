# Type-ahead find (Browse view)

Typing in the grid or the table jumps to the first item whose name starts with the typed text. There is no visible field.

## Behavior (modeled on Dolphin, Windows Explorer, Qt/GTK item views, WAI-ARIA listbox)
- Keys typed less than the search timeout (default 1000 ms, 300-3000; Dolphin 1 s, Qt 400 ms) apart form one search string; the match is case-insensitive and
  starts at the beginning of the name. Folder rows take part, "..." never matches.
- A new search starts AFTER the cursor (typing the same letter again moves on to the next item of that letter) and wraps around; a running search starts
  AT the cursor, so refining the string keeps the item while it still fits.
- A run of one key that matches no name cycles through the names starting with it (`444` keeps cycling `44...` names, as Dolphin does).
- A key that continues no name: typed fast (< the new-search pause, default 500 ms, 0 = never) it is a typo and moves nothing; after a pause it starts a new search.
- Space never starts a search, only continues one. Shift is a capital letter (no backward search). Modifier keys alone do not interrupt a search.
- Backspace removes the last character and Escape cancels the search, both only while a search runs; otherwise they keep their meaning
  (parent folder, clear selection). Every other key (arrows, Enter, Tab, F-keys, Ctrl/Alt/Meta shortcuts) ends the search.
- The search also ends when the cursor is not where the last key left it (click, arrows, rubber band, other folder) or on the timeout. No timer runs.
- Not active: while the filter or tag filter field has focus, a dialog, menu or drag is open, in viewer tabs and the tab grid.
- The jump selects the item at once (tile, thumbnail, inspector) like an arrow key does and centers it when it is not completely visible. Only the
  Multiview that follows the selection waits (`Pace::After`, default 400 ms of quiet), so no video is opened per letter; other selection paths are unaffected.
- When nothing starts with the typed text the status bar shows `No match for "..."` for a moment (option, on by default).

## Options (Settings > Appearance & Grid > Type-Ahead Find, `appearance.type_ahead_*`)
Enable, search timeout, new search after a pause (0 = off), also find folders, repeating a letter cycles, center the found item (else scroll only as far as needed), "No match" message in the status bar (shown for `TYPE_AHEAD_NO_MATCH_MESSAGE_MS`, then the previous message returns), delay of an open Multiview (0 = at once).

## Measured (debug profile = opt-level 3, VM, synthetic records with 24-character names, no match = scans the whole list)
| records | no match, full scan |
|---|---|
| 20 000 | 0.4 ms |
| 200 000 | 4.6 ms |
| 1 000 000 | 21 ms |

A match is found in the time it takes to reach it; extending a string whose item still fits costs one comparison.

## Not covered
Diacritics are not folded (`a` does not find `Aepfel`'s umlaut variant) and names are not Unicode-normalized; Greek final sigma lower-cases per character.
