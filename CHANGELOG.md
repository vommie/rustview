# Changelog

All notable changes to RustView are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/):
until 1.0, the patch number rises with every state that can be tested, the minor number with a milestone.

The entries before 0.3.82 were reconstructed from the git history (first to last commit); the versions are the ones set in
`Cargo.toml` at the time. They were never published as releases, but each one is tagged (`v0.1.0` to `v0.3.81`).

## [Unreleased]

### Added

- Settings > Multiview > Player: Normalize volume, the normalization every video starts with. The filter menu of a video starts
  from it; a choice there overrides it for that video in its view only (a duplicated tile takes it over), and its reset button
  goes back to the default.

### Changed

- The filter menu shows which filter is on: every filter keeps its icon, a filter that is on shows its icon and "On" (or its
  value) in orange, and every filter has a reset button at the right that is enabled only while the filter is on. The rotations
  are three entries (90 degrees right, 180 degrees, 90 degrees left); a click on the one that is on turns the picture back.
  Mirror and Deinterlace have icons. Nothing in the menu moves when a filter changes.
- The filter menu and the "..." menu are as wide as their entries need (up to a generous limit), each submenu with its own
  width.

### Fixed

- Mirror did nothing with hardware decoding: like in RustCut the flip is a filter on decoded frames, so while a video is
  mirrored its player copies the frames back (`hwdec=auto-copy`); without a mirror the faster decoding stays. The mirror now
  always swaps left and right on screen, also on a rotated video.

## [0.6.11] - 2026-11-06

### Added

- The control bar of a video adapts to narrow tiles (several videos in the Multiview): step by step the volume slider becomes a
  speaker with a vertical volume bar, the remaining time goes, the A-B repeat group becomes one button that shows the group in a
  bubble, and the sound and the A-B repeat move into a new "..." menu (with an A-B submenu); the narrowest bar keeps only play,
  the progress bar and "...". The progress bar always keeps a usable width, and a bar at a boundary does not switch back and forth.
  Esc closes the volume bar and the menu. Whether the A-B bubble is open is remembered.
- A filter button on the control bar of a video: rotate right, left or by 180 degrees (combinable with mirroring), mirror,
  deinterlace, sliders for gamma, brightness, contrast and saturation, and volume normalization (Loudness, Dynamic, Speech,
  Compressor, as in RustCut), plus All Filters Off. The button turns orange and shows how many filters are on; on a narrow bar the
  filters are a submenu of "..." (Filters > Image / Sound) and "..." shows the number. Filters apply only while the video is shown
  and are not remembered; a duplicated tile takes them over. Hotkeys in the Video player area: R rotates and goes round (90, 180,
  270, back to 0 degrees); rotate left, mirror, gamma up by 10 (wraps to 0), deinterlace and all filters off have no key by default.
- The load menu of the A-B repeats has a refresh button: it reads the saved repeats again, for example after they were changed in
  mpv itself.

## [0.6.9] - 2026-11-02

### Added

- `rustview api move-media --from OLD --to NEW` moves a media record to the new path of its file, after another program (RustCut)
  moved the file: the record keeps its tags, rating, hash, source, thumbnail and video preview. A different record at the new path
  is refused unless `--replace` is given (its file was overwritten). A second call reports `already_moved`; aliases `move`, `relink`.

- `ATTRIBUTION.md` and `LICENSES/` hold the notices the licenses of the bundled crates, libraries and icons require when a built
  RustView is passed on; the package installs them to `/usr/share/licenses/rustview/`.

### Changed

- License: RustView's own source code stays MIT, the built program as a whole is under the GPL-3.0-only, because it links the
  system's libmpv and FFmpeg libraries, which Arch and Manjaro build under the GPL; Slint is now used under its GPL-3.0 option. The
  package says `MIT GPL-3.0-only`, the About card in Settings > General and the README say so and point to the notices.

## [0.6.7] - 2026-10-28

### Changed
- **Rating buttons in the Tag & Rate panel have a new look**, the same as in RustCut 0.0.75: a dark button with a thin
  border in the rating's color; under the pointer it fills with its color and the digit turns dark. The chosen rating
  is filled completely (no white border any more) and gets a little lighter under the pointer; a press darkens it
  briefly. A rating that only some of the selected media have is filled faintly. The bar is lower (20 px instead of 28 px).

## [0.6.6] - 2026-10-27

### Added
- **Compact Mode for the tag tree**: dense rows that share one line, in a smaller font, with the same marks,
  expand buttons and indentation, a slightly darker row background and a lighter hover; text, lines and symbols sit on
  whole pixels and stay sharp. Right-click the tree's free space and choose "Compact Mode" ("Normal Mode" switches back),
  or use Settings > Tags & Ratings > Tag Tree Appearance > Compact tag tree. The choice is kept (new setting
  `tags.compact_tree`, off by default; older settings files load unchanged, no migration). Normal Mode looks exactly as
  before. The same mode exists in RustCut 0.0.71.

## [0.6.5] - 2026-10-26

### Changed
- **mpv shows and takes nothing of its own in RustView's videos**: no OSD bars, seek or status messages of mpv (also when your
  mpv.conf asks for them, for example `osd-level=3`), and no key bindings of mpv or your input.conf (RustView handles every key
  and click over a video anyway). Your mpv.conf otherwise applies as before (languages, cache, decoding).
- **Videos no longer ask mpv for their state 50 times a second**: every player is told by mpv when its time, pause state,
  volume, mute or picture size changes and otherwise sleeps. A paused video tab now costs about 0.3 % of a CPU core instead of
  5 %.
- **The views with videos and pictures no longer wake up every 50 ms**: the fullscreen, the tab view and the tab grid react to the
  pointer, keys and the players at once and only wake up when something changes by itself (a bar hides, the cursor hides, a
  zoomed picture is sharpened). Only while the control bar of a playing video (or, in the tab grid, a playing video) is on
  screen does its time move every 50 ms.
- mpv's built-in helper scripts that only react to its own keys (statistics, console, command prompt, selection lists, panning,
  context menu) and the youtube-dl hook are no longer started in every video player; mpv gets no keys in RustView. Your
  script-opts other than the A-B script's message switch are kept now.
- A fresh crop tool covers the whole picture: dragging inside it now draws a new rectangle (before, nothing happened).

- **A-B repeat in the control bar**: the current A-B is drawn in the progress bar itself (yellow, the progress shows through)
  instead of in the strip of saved repeats, so a loaded saved repeat stays visible there. A right click on Set A or Set B clears
  that point, a right click on the AB button clears both (holding it still works); the tooltips say so and name the keys. A blue
  (not looping) marker turns white where it crosses the played part of the progress bar. The strip of saved repeats keeps a
  margin of 7 points to the ends of the progress bar, "Delete all" in the menu of saved repeats is only as wide as its label, and
  names too long for the menu are shortened in the middle.
- **Videos repeat with mpv's `loop-file`, never with `loop-playlist`**, whatever your mpv.conf says: `loop-playlist=inf` (or a
  number) is turned into `loop-file`, so a repeating video jumps back to its start instead of being loaded anew at its end.
  The saved A-B repeats on the progress bar no longer vanish for a moment at every repeat, and a video repeats again when an
  A-B loop of the script is switched off (the script leaves `loop-file` off then).
- Tooltips of elements at the bottom of a window (the control bar of a video) open above the element instead of covering it.

### Removed
- The switch "Show the script's messages in the video" (Settings > Multiview > mpv script ab_repeat.lua): the script shows no
  messages in RustView's videos any more, the control bar shows what happens. mpv itself is not changed.

### Fixed
- After a video was loaded again by mpv, its progress bar could keep a duration of zero (mpv reported no change because the
  duration was the same), which squeezed the saved A-B repeats into the start of the bar until something else redrew it.
- The line under the column headers of the Browse view runs through; before, it showed only as light bits in the gaps between
  the headers.
- A click on the progress bar of a playing video no longer makes the picture jump back and forth for a moment: the click seeks
  once instead of once on press and again on release.
- **A video that cannot be played shows why**: instead of an empty black tile, the tile shows a warning icon and mpv's reason
  where the video would be (for example "avformat_open_input() failed (unrecognized file format)"); also when mpv stops by itself.
  A refresh (F5) tries the file again.
- The log line of F11 in the tab view no longer claims a middle click in the tab grid.

## [0.6.0] - 2026-10-26

### Added
- **A-B repeats of the mpv script ab_repeat.lua in the control bar of videos**: when your mpv script ab_repeat.lua (version 2.0
  with its interface) runs in a video's player, the control bar gets an A-B group in a capsule after the remaining time: set A,
  set B, AB (a click switches the repeat on and off, holding it clears A and B), save (a small bubble for the name, Enter saves,
  with a hint when the name or the range is saved already) and the menu of saved repeats (search, pages or a scroll list,
  a mini bar per entry showing where it lies, rename, delete and delete all by holding, the running one highlighted; a badge on
  the button counts them). Above the progress bar a tab grows out of the bar with every saved repeat as a thin colored line
  (overlaps show stronger, the colors stay while RustView runs), the current A-B in yellow, flags with the times of A and B
  (dragging one moves its point, a point the script set by itself is dashed), and the name of the repeat under the pointer;
  a click on a line loads that repeat and plays it. While a repeat loops, the rest of the progress bar is darker. Everything goes
  through the script, so a repeat behaves exactly as in mpv and the saved repeats are the same in both. Several players of the
  same file keep their lists in step, and renaming or moving a video in RustView takes its saved repeats along.
- **Keys for A-B repeats** (Settings > Hotkeys > MPV ab_repeat.lua, only while the script runs): set A (Home), set B (End), clear
  (Ctrl+Delete), and without a default key the save bubble, the menu of saved repeats, jump to A and the next or previous saved
  repeat.
- **Settings > Multiview, card "mpv script ab_repeat.lua"**: look for the script (on by default; an incompatible script is
  named as an error, Check again looks again without restarting RustView), whether the script shows its own messages in the
  video, the colors of the saved repeats, what clicks, the pointer and dragging do in the playbar, and how the menu pages.
  Settings > Debug names the script too.
- **Jumping with a key shows the control bar for a moment**: the jump and frame keys of the video player and the A-B repeat keys
  show the bar of the video for 1.5 s, so the new position is visible.
- **A volume bar shows when the mouse wheel or a key changes the volume of a video**: a flat, semi-transparent bar with a speaker icon
  near the bottom of that video (at most half its width), in the fullscreen and in tabs, with one or several media. It fades out like the
  rating icon, and every new change shows it again. Dragging the volume slider of the control bar shows no bar.
- **The mute key shows a speaker icon** in the middle of the video (crossed out when it is muted), like the rating icon. The mute buttons
  show none.

### Changed
- **Videos are drawn by RustView itself** (branch video-in-slint, first step): mpv renders every video on a thread of its own into
  textures that the RustView window shows like a picture, instead of a native mpv window over it. Context menus and tooltips of the
  main window now simply lie over a video (no more holes cut into the video), and moving a video between a tab and the fullscreen
  keeps it playing. mpv itself, your mpv.conf and your scripts are unchanged; the control bars still float in windows of their own
  for now. Settings > Debug: the video self-test renders through the same path (no test window opens any more) and says how videos
  are drawn and whether the picture had content.
- **The control bar of videos is drawn by RustView's own window** (branch video-in-slint, second step), together with its A-B group:
  it no longer floats over the video in a window of its own. It fades in and out also without a compositing manager, and nothing of
  it can stay behind over another application. The small bar of a video in the Tab-Grid is drawn the same way.
- **The action bars of the Multiview are drawn by RustView's own window** (branch video-in-slint, third step): the media bar of each
  medium, the primary bar, the "Open With" and Save drop-downs, the zoom slider below the bar and the hint ("Select a medium first")
  no longer float in windows of their own. Nothing of them can stay over another application, they look the same with and without a
  compositing manager, tooltips and menus of the window lie over them, and a drop-down closes on a click anywhere else in the view
  (that click does nothing else). RustView no longer asks the X server where the pointer is every 50 ms: the bars follow the pointer
  events of their window, so they also appear with native Wayland.
- **Everything over a video is drawn by RustView's own window** (branch video-in-slint, fourth step): the rating and save icon, the
  speaker of the mute key, the volume bar of the wheel and the keys and the corner marks of the selected medium look exactly as over
  a picture; mpv draws nothing over the picture any more. In the Tab-Grid the time badge of a video ("0:03 / 0:08") is drawn by
  RustView too, and the pause badge of a single video moved to the bottom left corner, so the two no longer cover each other.
  Messages of mpv scripts no longer get the dark box the time badge used to give them.
- **A zoomed video is moved and scaled by RustView like a picture** (branch video-in-slint, fifth step): mpv always renders the whole
  video, and RustView draws it where the zoom puts it. Dragging a zoomed video follows the pointer at once instead of a frame later,
  zooming never shows the picture in the wrong place for a moment, and the minimap of a zoomed video plays along with it (before it
  showed a still that was renewed every 2 s). The volume bar lies on the part of the video that shows. Subtitles
  now grow with a zoomed video like the picture itself.
- **A video moves between the fullscreen and its tab without a black picture** (branch video-in-slint, sixth step): while it
  changes windows, the new window shows the video's last picture until the video runs there. In the tab grid the videos scroll
  with their items without any lag, and the small control bar of the video under the mouse follows the mouse directly (before it
  was polled from the X server 20 times a second).
- **The save bubble and the menu of saved A-B repeats open right above their button**: the caret points at the button that opened
  them (also on a narrow video and with the pill style), the bubble touches the bar (no gap any more), and opening or closing them
  no longer makes the whole bar flicker. A menu that is wider than its video may use the room next to it. A click anywhere else in
  the view closes them, and the keys work again right after.
- **The windows are drawn with Slint's skia renderer** (before: femtovg). It uses EGL, which mpv's hardware decoding needs to hand
  decoded frames over without copying them back. Text and shapes look the same. `RUSTVIEW_RENDERER=femtovg` starts with the old
  renderer once (for a check or a driver problem).
- **The settings page "Fullscreen" is now called "Multiview"** and starts with a short explanation of what the Multiview is (the
  view of one to four media, in the fullscreen and in tabs). Everything about the video player lives there now, because videos play
  only in the Multiview: the jump distances of the jump keys moved from Settings > Viewer > Videos to the new card Multiview > Jump
  Keys, and the card "mpv script ab_repeat.lua" sits next to Player and Sound. Settings > Viewer keeps how images and videos open
  from the media view, the rating badge of tabs and the tab background. Only the place in the settings window changed: your settings
  are kept as they are (no migration).
- **In a Wayland session RustView now runs as a native Wayland program** (branch video-in-slint, seventh step; before it ran through
  XWayland). Videos with all bars, menus, zoom and the Multiview work there, since everything over a video is drawn by RustView. Not
  yet on native Wayland: dragging files to other programs, dialogs tied to their window and the pointer lock of the minimap drag
  (Settings > Debug names them). `RUSTVIEW_WINDOW_SYSTEM=x11` starts RustView through XWayland as before. An X11 session is unchanged.
- **The A-B tab above the progress bar only shows when it has something in it** (a saved repeat, A or B); without them the bar
  looks like any other. Without a compositing manager the tab is now a plain rectangle over the progress bar instead of a window
  cut to the curved outline.

### Fixed
- **The Multiview fullscreen opens on native Wayland**: it waited for an X11 window and closed itself again; it now goes into
  Wayland's fullscreen on the monitor that is set (or the one the window opened on).
- **The Multiview fullscreen no longer opens as a merely maximized window now and then** (with the panel visible, on X11 and on
  native Wayland, where it happened every second time): when the window manager did not take the fullscreen state, RustView asks for
  it again.
- **The buttons of the A-B group and of the menu of saved repeats show their tooltips** (they never appeared).
- **The menu of saved A-B repeats and the save bubble work with the mouse**: a press inside them was taken for a press outside
  and closed them, so a click on an entry could get lost and deleting by holding never worked; a click on the video now closes
  them reliably, also a very short one.
- **No darker line between the A-B tab and the control bar** (the two translucent fills overlapped by one pixel).
- **The length of a saved A-B repeat is rounded** in the menu and the label under the pointer (a repeat of 1.9 s reads "0:02",
  not "0:01").
- **Settings > Multiview no longer says that only pictures zoom**: the Image Zoom card and the tooltip of the Zoom button now name videos
  too, which zoom the same way.
- **mpv's own on-screen controller no longer runs inside RustView's players** when the user's `mpv.conf` switches it on (`osc=yes`): the
  mpv configuration is read after RustView's own options, so the controller was loaded again. It was never visible (RustView takes the
  mouse over videos), but it ran in every player.
- **Zooming a video no longer flickers when it leaves 100 % or comes back to it**: for one frame the video showed up stretched or at the
  wrong place. In the fullscreen and in tabs the video now always draws into the whole area of its tile and is only cut to its place while
  it is not zoomed, so starting or ending a zoom never resizes the video's own window. The rating icon now stays centered on the video
  while it is zoomed, too.

## [0.5.1] - 2026-10-25

### Fixed
- **The corner marks of the selected medium in a Multiview frame its view now, not the medium** (your decision): they sit in the corners
  of the tile's area (its column at the full height of the row) for pictures and videos alike, in the fullscreen and in tabs. Before, the
  marks of a video were placed by its picture and landed in wrong places as soon as the video was zoomed; a zoomed picture had them at its
  visible part. The brightened area is the same rectangle, and Settings > Fullscreen > Selected Medium and hiding the marks with the mouse
  cursor work as before.

## [0.5.0] - 2026-10-25

### Added
- **Settings > Debug checks what RustView needs and how it is doing** (click the check button; nothing is checked on its own). The card
  **Dependencies** lists the programs and libraries RustView uses (libmpv and FFmpeg with their versions, xclip or wl-clipboard for copying
  pictures, kscreen-doctor/xrandr for the monitors, the desktop portal for the file dialogs, gio for Open With, xdg-open, the file manager, the X11
  extensions and the compositor) with the exact `sudo pacman -S ...` command for everything that is missing, plus one command for all of them; a
  closed group **Developer** has samply and `perf_event_paranoid`. The card **Health** shows the library database (size, media, folders, tags), the
  folder tree check, the thumbnails, the settings file, the free space, the drive of the root folder (from the mount table, the drive is not
  touched), the window system, the graphics (with a warning for software rendering), how videos draw and the RAM budget. Problems come first, what
  passed is one row; the cards sit side by side in a wide window and one below the other in a narrow one. `rustview check` prints the same checks in
  the terminal.
- **The log is kept:** the last 5,000 lines stay in memory, and Settings > Debug > Log can write log files (off by default): `rustview.log` as it
  happened and `rustview-private.log` with file names, paths, folders, tags and searches hidden, to share; a new pair per start, the last 5 are kept,
  in `~/.local/state/rustview/`. The log level (Errors, Warnings, Info, Debug, Trace) applies with Save without a restart; `RUST_LOG` still wins.
  mpv's warnings and errors are in the log now, and a switch writes mpv's detailed log to `mpv.log`.
- **A log window** (the list button of the Log card) follows the log live and can stay open next to the main window: every line, warnings and
  errors, or errors only, with names shown or hidden, follow the newest line, copy the lines shown.
- **A diagnostic report** for bug reports (copy or save): version, system, window system, graphics, the checks, the settings you changed, the
  startup and the last 300 log lines; names and paths are hidden while "Obfuscate" is on.
- **Debug tools:** a video self-test (mpv plays its own test picture for three seconds in a small window and tells how it was drawn), the folders
  of RustView (open them, copy their paths), the startup timeline of this session, live runtime values (memory, thumbnail cache, players, window
  system, graphics, environment overrides), resetting the window size and position, and resetting the layout (held).
- **Videos zoom like pictures** (tab and fullscreen): Ctrl + mouse wheel zooms toward the pointer, Ctrl++ / Ctrl+- / Ctrl+Backspace, dragging moves
  the picture, the Zoom button of the media bar opens the slider, and the minimap shows a still of the video with the visible part framed (drag it,
  middle click resets, right click opens its slider). A zoomed video uses the free room around its tile, as a zoomed picture does.
- **The Unsaved Changes dialog shows each picture:** a small thumbnail at the left of every row, in a square, as the picture looks with its
  changes (the cropped part), so you can tell the pictures apart before deciding which to save.
- **A selected medium in the Multiview** (with two or more media, fullscreen and tab): a plain click on a medium, on a video too, or on a button of its media
  bar selects it. The selected medium is marked by blue corner marks inside its corners and a slightly lighter area around it (in its column); a video gets
  the corner marks from the player itself. The selection stays until another medium is clicked or the medium leaves the view; a Multiview of one medium
  needs none.
- **A short hint over the view** ("Select a medium first") when a key needs a medium and none is selected: a disc like the rating icon with one line below
  it, also over a video. It fades by itself.
- **Every key that acts on a medium in a Multiview goes to the selected one** (with two or more media), wherever the pointer is: F2 (rename), DEL (delete),
  Ctrl+Backspace / Ctrl++ / Ctrl+- (zoom), Enter (apply the crop), Ctrl+S / Ctrl+Shift+S (save, save as), Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y (undo, redo) and F5
  in a tab (refresh). When a key does not fit the selected medium, the hint says why ("Nothing to save", "No crop to apply", "Videos cannot be zoomed
  here", ...). While the pointer rests over a video, the keys the player knows itself (for example Ctrl+S, Ctrl++) still go to the player for now.
- **DEL and the zoom keys work in a Multiview tab** too, not only in the fullscreen; DEL also with several media (it deletes the selected one).
- **The marks of the selected medium hide with the mouse cursor** while the resting cursor is hidden; a key that acts on the medium shows them for a moment.
  Settings > Fullscreen > Primary Action Bar > Mouse cursor has the switch "Hide the selection marks too" (on by default); the tooltip of the cursor button
  of the primary bar says when the marks go with the cursor.
- **Settings > Fullscreen > Selected Medium:** how much the area around the selected medium is lightened (Off, Weak, Medium, Strong) and how long its corner
  marks are (Short, Medium, Long); Medium and Medium by default.
- **The sound can follow the selected video:** Settings > Fullscreen > Sound > "The sound belongs to" has the new choice "The selected video" (the leftmost
  video has the sound until you select one; a mute you set yourself stays).
- **The panels of a Multiview tab follow the selected medium:** the inspector, the Tag & Rate panel (rating buttons and tags) and the tag history act on the
  selected medium. With several media and none selected the Tag & Rate panel greys out and says "Select a medium in the view", and its buttons show the
  hint over the view. The refresh button of the tab still reads all media of the tab.
- **Settings > Hotkeys: every key can be changed.** One section per area (General, Browse, Multiview in a Tab, Multiview in the Fullscreen,
  Video Player), each collapsible; a filter (action, section or key) and an "Only changed" switch. The keyboard button of a row waits for a key (Esc
  cancels); a key that another action of the same area already has is named in the row and moves over when you press it a second time; a key's cross
  removes it and the arrow button brings the defaults back. Browse refuses keys that type a character (they belong to type-ahead find); fixed keys (Esc,
  the cursor keys of Browse, type-ahead find) are listed with a lock whose tooltip says why. "Every key back to its default" is held for a moment. Only the
  keys that differ from the defaults are stored (`hotkeys` in `settings.json`; the page's filter, switch and collapsed sections in
  `view.hotkeys_page`); no migration, older settings files load as before.
- **Hold to confirm:** buttons and menu entries can act only when held (a red line runs around them, a short click shows the tooltip with "Hold 1 s").
  Settings > General > Confirmations > "Hold to confirm" sets the time (1 s by default, 0 = a click is enough). The first user is "Every key back to its
  default".
- **New keys:** F1 opens Settings > Hotkeys, Ctrl+Comma the settings; Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+Page Down / Ctrl+Page Up switch tabs; Alt+1 /
  Alt+2 show Browse / the tab grid; in Browse F11 opens the selection in the fullscreen, Ctrl+Enter in background tabs, Ctrl+T switches between thumbnails
  and the table; in a tab F11 or F shows it in the fullscreen, and in the fullscreen F11, F and Ctrl+W close it; F5 also works in the fullscreen
  (the selected medium is read again); plain Backspace resets the zoom in a Multiview (besides Ctrl+Backspace).
- **The zoom keys work on a selected video:** mpv zooms the video inside its tile (Ctrl++ / Ctrl+-, Backspace or Ctrl+Backspace for the whole picture
  again); Ctrl + mouse wheel over a video zooms toward the pointer as before. The next video in the same tile starts unzoomed.
- **RustView controls the videos with the keyboard:** the keys of Settings > Hotkeys > Video Player act on the selected video in a tab and in the
  fullscreen: Space play/pause, M mute, Left/Right jump, Shift+Left/Right jump far, Comma/Period frame by frame, Ctrl+Up/Down louder/quieter, [ and ]
  slower/faster. Settings > Viewer > Videos sets how far the jumps go (5 and 60 seconds by default). With a picture selected the hint says
  "Select a video".
- The Rate Selection submenu shows the rating keys.
- **Keys for the buttons of the media bar and the primary bar:** Settings > Hotkeys lists Sticky, Duplicate, Open as Tab, Crop, Remove, Send to Tab,
  Mute All, Play/Pause All, Reset Layout, Hide Cursor and Pin (without a key at first); a key acts like a click on the button of the selected medium.
- **A control bar of RustView's own for every video** in a tab and in the fullscreen, at the bottom of the video: play/pause, the current time, a wide
  progress bar, the remaining time and the volume (a speaker button and a volume slider). A click or a drag on the progress bar goes exactly there; a
  bubble above it shows the time under the pointer. A click on a time shows milliseconds for that video. Mute stays a state of its own: the slider sets
  the level of a muted video without unmuting it (the level shows dimmed), the speaker shows "muted" also at level 0, and a click on it brings a silent
  video back to its last level. With several videos each has its own bar. **Settings > Fullscreen > Player:** a translucent bar (default) or a floating
  pill, its opacity (70 %), its height (30 px; everything in it grows with it), when it shows (pointer near the bottom of the video, or any movement over
  the video), a short fade, whether times start with milliseconds, and how the progress bar seeks (exact by default; like mpv's controller, by keyframes,
  or as mpv is configured). No migration: older settings files load with these defaults.
- **The mouse over a video belongs to RustView** (tab and fullscreen): the wheel changes the volume, Ctrl + wheel zooms the video, a right click plays or
  pauses, a click selects the video and a double or middle click works as over a picture.
- **Video items in the Tab-Grid work like picture items:** the hover frame shows over the video too, a right click opens the tab's context menu,
  Shift+click selects a range, and the wheel scrolls the grid smoothly. A small control bar (play/pause and the progress bar) appears while the pointer
  is over a video; the videos stay paused otherwise. Every other video shows its time ("0:03 / 0:08") in a badge at its bottom right.
- **Keys for the Tab-Grid** (Settings > Hotkeys > Tab-Grid): the arrow keys mark an item (the first press marks the tab you came from), Enter opens it,
  Del or Ctrl+W closes its tab, F11 shows it in the fullscreen.

### Changed
- **Esc** cancels the crop tool of the selected picture first, then the other crop tools (newest first), then it closes the fullscreen as before.
- **Esc over a video in a tab** no longer opens the fullscreen; it behaves like Esc anywhere else in the tab.
- **F5 in a Multiview tab** refreshes the selected medium (the refresh button of the tab still refreshes all of them).
- **Ctrl+0..5 in a Multiview rate the selected medium only**, wherever the pointer is (also when pressed over a video). Before, they rated every medium the
  view showed; with several media and none selected nothing is rated now and the hint appears. A Multiview of one medium rates it as before.
- A click on a video in a Multiview is no longer ignored: it selects the video (it still never pauses it).
- **Menus, tooltips, status messages and setting descriptions name your keys** (Settings > Hotkeys) instead of fixed ones.
- Keys are written with the short names of keyboard legends: Enter, Esc, Del, Left, Right, Up, Down, PgUp, PgDown.
- **The video player no longer takes any key:** every key pressed while the pointer rests over a video goes to RustView (Ctrl+S no longer takes a
  screenshot there, Ctrl++ no longer changes the audio delay, keys of your own `input.conf` of mpv no longer apply).
- **The rating keys need Ctrl in a tab too** (the plain digits 0 to 5 no longer rate).
- **A tab no longer pages with the arrow keys**, and Space no longer pages in the fullscreen; only a single picture of a fullscreen opened from Browse
  pages (Left, Up, Page Up back; Right, Down, Page Down forward).
- Shortcuts that also accepted the Meta key instead of Ctrl (Ctrl+A, Ctrl+W, ...), and Ctrl+F4 or Meta+W over a video, no longer do; add such keys on the
  Hotkeys page if you want them.

- **A click outside an open context menu also acts on what it hits**, as in KDE applications: it closes the menu and puts the cursor into a text field,
  selects the medium, presses the button or switches to the tab under the pointer; a right click opens the next menu there. Before, that click only closed
  the menu.
- **In a Wayland session RustView runs through XWayland**, so videos play and everything works as on X11 (before, a Wayland session showed no video at
  all). Without XWayland it still starts as a native Wayland program with those limits. `RUSTVIEW_WINDOW_SYSTEM=wayland` starts it as a native Wayland
  program on purpose. Videos also no longer try mpv's Wayland drawing (`gpu-context`) where they cannot be embedded; X11 entries of your own `mpv.conf`
  are kept.

### Removed
- **The Prev and Next buttons of a tab's toolbar:** a tab always shows the media it was opened with (paging stays in a fullscreen opened from Browse).
- **mpv's own on-screen controller** over videos in tabs and the fullscreen: RustView's control bar replaces it (mouse bindings of your own `input.conf`
  of mpv no longer apply there either).

### Fixed
- **The keys that work while typing did nothing in a filter field** (Browse filter, tag filter): Ctrl+W, Ctrl+Tab, Alt+1, Alt+2,
  Ctrl+Comma, F1 and Ctrl+F act there now, and Alt+2 no longer types "2" into the field.
- **Opening the Tab-Grid froze RustView** as soon as a tab was open (since 0.4.0): the app no longer reacted and had to be killed.
- **F5 in Browse** did nothing although the menu named it; it refreshes the selected files now.
- The tag menu no longer names F2 for "Rename Tag..." (F2 never renamed a tag).
- **A tooltip stayed on screen after a right-click menu** (for example "Tag: Forest" after right-clicking a tag and pressing Esc): the menu's
  heading showed it and it did not go away when the menu closed. Closing a menu now hides it, and while a menu is open only its own entries show
  tooltips. The heading of a menu shows a tooltip only when the name in it is cut short.
- **The tag filter ("Filter tags...") took no typing after a context menu** (the Browse filter neither): the click into the field only closed the menu,
  the field did not get the cursor, and what you typed went to type-ahead find instead. The click now puts the cursor into the field. While a menu is open
  the keys belong to it: Esc closes the menu instead of clearing a filter field behind it, and letters no longer land in that field.

## [0.4.0] - 2026-10-24

### Added
- **Crop pictures** in every Multiview (fullscreen and tab): the media bar of an image has a **Crop** button. The crop tool draws a rectangle over the picture with
  corner and edge handles, the picture outside it dimmed and rule-of-thirds lines while you drag. Top left it shows the size of the crop in pixels of the picture
  (e.g. "893 x 1116 px"), top right a small **ratio button**: a left click goes through the landscape ratios, a right click through the portrait ones, **O** is the
  picture's own ratio, "Free" means no ratio. A new ratio keeps the size (area) of the rectangle you made and its place: an edge of the picture it touches
  stays touched, a rectangle in a corner stays in that corner, otherwise its center stays. **Shift** while resizing keeps the ratio; drag inside to move, drag
  outside to draw a new rectangle. It works at every
  zoom (a zoomed picture starts with the part you see; the middle button pans, Ctrl + wheel and the minimap keep working). **Double click or Enter** applies the crop;
  nothing is written yet. **Esc** cancels the crop tool that was opened last (with several open, newest to oldest) before it does anything else, also from a
  video's window. Animated and multi-page pictures cannot be cropped (the button says why); a picture can only be edited in one view at a time.
- **Undo, Redo and Save** appear in the media bar while a picture has unsaved changes (Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y, Ctrl+S), and a dot marks its tab. The
  **Save** button is emphasized; its arrow offers **Save As...** (Ctrl+Shift+S) and **Discard Changes**. After saving it turns grey and fades away, and a save
  icon pops up in the middle of the picture (also when the media bar is hidden), with the same animation as the icon of a rating key.
- **Saving keeps quality and data safe.** Save overwrites the original in its own format: a JPEG crop that starts on the 8/16 pixel blocks is cut out without
  any loss (like jpegtran), any other is encoded with the file's own quantization tables; PNG keeps its palette and bit depth, WebP stays lossless or lossy,
  a still GIF keeps its colors, TIFF its 16 bits. EXIF, ICC profile and XMP are kept (EXIF gets the new size, its stale preview is dropped). The file is written
  completely to a temporary file, read back and only then put in place; if it changed on disk since it was opened you are asked (Overwrite Anyway, Save As
  Copy, Discard & Refresh, Cancel). Read-only files are never overwritten. Afterwards the library record, the hash and the thumbnail are refreshed and every view
  of the picture shows the new file.
- **Save As / export dialog:** it starts with the original's name, folder and format, so saving without renaming replaces the original (the button then
  says Replace); type another name or folder for a copy. The cursor waits at the end of the name. The folder is a text field (a missing folder is created when
  you save, an unusable path is explained and disables Save) with a Browse button. Format (JPEG, PNG, WebP, AVIF, TIFF, BMP) and its few options (quality, chroma
  subsampling, progressive, compression, lossless, AVIF speed), with a warning when a format loses something (transparency, 16 bits, metadata). **Same as
  original** (when the picture stays in its own format) saves like Save: without loss where possible, otherwise with the original's quality; the options are
  greyed out then. **Keep rating and tags** (shown when the original has some, on by default) gives them to the new file. Formats RustView reads but cannot
  write (HEIC, SVG, PSD, JPEG XL, JPEG 2000, EXR, ...) are saved through it. The saved file becomes what the view shows (fullscreen, tab, tab fullscreen),
  without loading it again; a fullscreen opened from the Browse view selects it there in place of the original (not for a file outside the library's root
  folder). The new file joins the library at once, also outside the root folder (it appears when the root covers it). The dialog can be made wider and
  remembers the last options.
- **No unsaved picture is lost unasked:** closing the fullscreen, closing tabs (also left/right/others/all/selected and from the tab grid), removing a tile, paging
  to another picture (fullscreen and the Prev/Next of a tab) or following another selection first lists the pictures with unsaved changes, one checkbox each
  (save or discard): **Save** saves the checked ones and goes on, **Don't Save** discards every change and goes on, **Cancel** keeps everything open. Quitting
  the application does not ask (the files are never touched by unsaved edits).
- **Settings > Image Edit:** the crop ratios (chips with their orientation, add, remove, defaults), the rule-of-thirds lines, "Keep JPEG crops lossless" (the
  rectangle snaps to the JPEG blocks), how the original is replaced (**safe**: a backup until the new file is in place, default; **to the trash**; **directly**)
  and the presets of JPEG, PNG, WebP, AVIF and TIFF. Settings > Fullscreen has a switch for the Crop button.

### Changed
- **Every dialog belongs to the window it was opened from** (the main window, the fullscreen, the settings window or another dialog): it opens centered
  on that window and stays above it, and only above it, so a dialog no longer floats over other programs. While it is open its window is dimmed and
  locked; clicking that window (or switching to it in the taskbar) brings the dialog to the front instead of closing it. This covers every dialog,
  including Save As and Unsaved Changes of the image editor, the color picker of the settings (it stays above the settings without locking them) and the
  folder pickers (root folder in Settings, Browse in Save As). **The settings window works the same way:** it belongs to the main window, opens centered
  on it and no longer closes when you click the main window; that click brings it to the front (close it with Cancel, Save or Esc). With a compositor,
  KDE's "Dialog Parent" effect also dims the window and its videos. On Wayland a dialog stays an ordinary window (the toolkit offers no parent there); a
  click on the locked main window closes the dialogs and the settings as before.
- **Dialogs keep their layout when they are resized:** the content stays at the top at its own height (single-line fields never stretch, extra height stays
  empty below), a dialog made lower than its content gets a scroll bar instead of squeezing it, and a dialog grows when a note appears while it showed
  everything. All dialogs follow this.

### Fixed
- **A dialog opened from the fullscreen locks it:** behind Save As, Unsaved Changes or Delete the fullscreen still reacted to clicks, the wheel and a
  double click on a video (it could page, close itself or ask again). The videos of a locked window no longer take clicks either.
- **Clicking the main window behind New Folder, Save As, Unsaved Changes, Open Many Files, Merge Tabs or the delete dialog for several items** unlocked
  the main window while the dialog stayed open; the click brings the dialog to the front now.

No migration: the new settings are additive and older settings files keep working.

## [0.3.86] - 2026-10-23

### Fixed
- **Selecting a second tab with Ctrl+click now selects two tabs:** a selection that starts with a Ctrl or Shift click in the tab view includes the tab you are looking at, so the
  menu of the clicked tab is the one for several tabs (Merge/Close Selected Tabs) and not the single-tab menu. In the Browse view and the tab grid nothing is added implicitly.
- **Menus over a video no longer leave a black hole:** the video was cut out in the full maximum height of a menu (70 % of the window) instead of its real height, so a short menu
  showed a black area below it (also in 0.3.84 and 0.3.85).
- **A menu over a video closes with a click on the video** (and anywhere else): while a menu is open the native video windows let the pointer through to the main window, where
  the click closes the menu. Before, the video swallowed the click and only a click outside the video closed it.

No migration.

## [0.3.85] - 2026-10-23

### Added
- **Merge tabs.** Select several tabs with **Ctrl+click** (toggles one tab, the shown tab does not change) or **Shift+click** (the range from the last clicked tab; Ctrl+Shift
  adds the range) in the tab bar and in the tab grid; selected tabs get an accent-colored border. A click on a tab without a modifier, a click on free space or Esc ends the
  selection. Right-click a selected tab: **Merge Selected Tabs** puts the media of all selected tabs into **one new Multiview tab** (at the place of the first of them, named
  "Multiview"), **Close Selected Tabs** closes them. The merged tabs are closed; the new tab is shown (the tab grid stays open). Nothing is restarted: running videos keep
  playing in their mpv players and windows, decoded images keep their pixels, pause and mute choices of the user stay; the sound follows the Settings > Fullscreen > Sound
  rules (one video audible, the leftmost that was). Widths dragged in a gutter and the zoom of an image start from the automatic arrangement again.
- **Merge With Left / Merge With Right** in the menu of a single tab (**Merge** with a submenu of Left, Right and **Left & Right** when it has tabs on both sides): the tab and
  its adjacent neighbor(s) become one Multiview tab. The entries share the group of "Move Tab...".
- **Limit check.** A Multiview shows at most four media. When the tabs hold more (a medium that several tabs show counts once per tab), nothing is merged and a warning
  dialog says how many media have to be removed from the tabs first, with the media of each tab.
- Ctrl+click on a video in a tab-grid item selects the tab like a click on the rest of the item.

### Changed
- A Multiview that was never shown starts only the tiles that are not running (needed for merged tabs); a tab that was merged away never leaves a player behind.

No migration: settings, library and thumbnail store are untouched (the selection of tabs is not persisted, tabs do not survive a restart either).

## [0.3.84] - 2026-10-23

### Added
- **Rating hotkeys Ctrl+0 (no rating) to Ctrl+5** in the Browse view (grid and table: the selected media, never folder rows), in every Multiview (fullscreen
  from the Browse view or from a tab, a Multiview in a viewer tab, a tab shown fullscreen) and in image and video tabs. In a Multiview the rating applies to
  **every medium that is shown**, also sticky or pinned tiles the Browse view has not selected; a medium shown by several tiles is rated once. The keys also work
  while the focus is in a video's own window (mpv), and on the numpad. Key repeat does not rate again; nothing is rated while a dialog or the settings window is
  open, while a text field has the focus, or during a drag.
- **Rating animation:** the icon of the new rating (about 50x50 px) appears in the middle of every tile of a rated medium at once, stays for 0.75 s and fades out
  in the next 0.25 s (1 s in all). The digit is filled white (the icon sits on a white disc), so the picture never shines through it. A new rating cancels the running animation immediately and shows its icon at full strength, with no cross-fade. On a video
  tile the icon is drawn by mpv itself, so it needs no compositor and works on Wayland too. Settings > Fullscreen > Rating switches the animation off
  (the rating itself is always saved).

### Changed
- **Ctrl+0 no longer resets the image zoom** in a Multiview (it rates now); **Ctrl+Backspace** resets it. The plain digits 0-5 keep rating in a viewer tab.

## [0.3.83] - 2026-10-23

### Changed
- **Dates are shown in local time:** the modified and created dates in the tables, the folder tree and the conflict dialog were UTC and now match what the
  file manager shows. The time stamps stored in the library and returned by the API stay UTC.

- **"Open With > Other Application..." asks the desktop portal directly** (the application chooser of your desktop, KDE's own dialog on Plasma). It no longer starts
  a Python script (PyQt6) or `kdialog`, so it works on every system that has `xdg-desktop-portal`; cancelling the dialog does nothing, as it should.
- **Dropping files on other programs no longer needs Python:** the X11 drag and drop handshake (XDND) that hands the files to Dolphin, a browser or any other
  program is part of RustView now (it used to start a Python script with `libX11`). Without `python3` on the system the drop used to do nothing without a message.
- **The drag ghost is drawn by RustView itself:** the small card with the thumbnail, the file name and the file count that follows the pointer while you drag files
  out of RustView no longer needs Python with GTK or Qt. It appears at once (no interpreter start), and it is opaque with square corners when no compositor runs.

### Removed
- The hidden `--reset-folder-tree` option, a temporary debugging aid for the folder tree.

## [0.3.82] - 2026-10-23

### Added
- **A real app icon:** a thumbnail grid with an eye over it. The colored icon (scalable and PNG sizes from 16 to 512 px) works on light and
  dark panels; a monochrome symbolic icon follows the KDE color scheme. The main, settings and fullscreen windows carry it as their window icon.
- **About card** in Settings > General with the version, the license and the Slint attribution.
- The package ships the AppStream metadata (software centers), the README and this changelog.

### Changed
- Updated to **Slint 1.18** and the current compatible versions of all dependencies (no behavior change; the thumbnail badges were adapted to the new
  flexbox layout API).
- **The command line is parsed by `clap`:** `rustview --help` lists everything, `rustview api --help` (and `rustview <command> --help`) documents the API commands.
  Every spelling that PyCutRv and other tools used keeps working (`api <command>` and `<command>`, the aliases, `--option=value`, `--api-<command>`).
  Options of the XnView MP migration are refused when `--migrate-xnview` is missing, and the last of two opposing flags wins.
- The data and settings folders, the `~` in typed paths, the mount table, memory and process information, signal handling and file URIs now come from
  established libraries instead of hand-written code (no change in behavior, except the fixes below).
- The X11 window handling (video windows, action bar windows, maximize/fullscreen state, pointer lock) runs on the pure Rust `x11rb` instead of
  hand-written Xlib calls (no change in behavior; X errors are logged instead of ending the process).
- The whole code base passes `clippy::pedantic` and is formatted with rustfmt. Numeric conversions no longer wrap or truncate silently: sizes,
  indexes and pixel values that do not fit their target type are clamped instead (ids that pass through Slint keep their old wrapping behavior). No
  change in behavior for any value that occurred before. The messages of the XnView MP migration show paths without quotes.
- Removed an unused thumbnail memory arena that wrote through a shared reference (undefined behavior if it had ever been used); every remaining
  `unsafe` block now carries a safety comment.
- **The application id is now `io.github.vommie.rustview`** (it was `org.rustview.app`) and matches the desktop file, so the desktop shows the
  launcher icon for the window. If you pinned RustView to the taskbar, pin it again once after updating. Settings and library are untouched, no migration.

### Fixed
- "Open With" passed unencoded `file://` URIs to applications whose command line uses `%u`/`%U`, so files with spaces or `#` in their names failed to open.
- The name of the migration audit log file used UTC although it is documented as local time.

## [0.3.81] - 2026-10-23

### Changed
- **One tab view: every tab is a Multiview.** A medium opened as a tab (double click, Enter, "Open in new tab", the open-in-background
  entries) is now a Multiview of one tile, exactly like a fullscreen that was turned into a tab. The separate image and video tab views
  are gone; a tab keeps its title.

## [0.3.80] - 2026-10-21

### Changed
- Media bars belong to their tile: open bars follow their tiles when media join or leave the Multiview, in a row and in the 2x2 layout.

## [0.3.79] - 2026-10-21

### Added
- A media action bar and a zoom slider bubble **per tile**; any number can be open at once.

### Fixed
- The zoom slider no longer jumps or disappears on a click.

## [0.3.78] - 2026-10-21

### Changed
- Dragging the image zoom minimap locks the pointer (hidden, held in place, only its movement counts), so a fine drag is not stopped
  by the screen edge.

## [0.3.77] - 2026-10-21

### Changed
- Dragging the image zoom minimap is relative with a zoom-dependent gain (fine when slow, direct when fast) and goes on outside the minimap.

## [0.3.76] - 2026-10-21

### Fixed
- Panning a zoomed image follows the pointer exactly, in whole device pixels, with stable rounding (no jitter).

## [0.3.75] - 2026-10-21

### Changed
- The media action bar always sits at the top right of the canvas of its medium, at the height of the primary bar.

## [0.3.74] - 2026-10-21

### Changed
- The image zoom slider is one logarithmic range with ranked, thinned marks (reset, 100 % lines, 50 % lines, 25/75 % dots) that
  also act as gravity points.
- Panning shows no blurry preview any more: the sharp render covers the room plus an overscan margin.
- The media bar stays at the corner of its medium.

## [0.3.73] - 2026-10-21

### Added
- Image zoom option "maximum zoom out" (0 makes the slider one-sided), a minimap timeout, a right-click slider bubble on the minimap and
  a middle click that resets the zoom.

### Changed
- A zoomed picture uses the whole free room around its tile, not only the tile.
- The minimap has hard corners; bars that went away with the focus come back with it. New defaults: smooth zoom, 15 % step.

## [0.3.72] - 2026-10-20

### Added
- **Image zoom** in every Multiview host (fullscreen and tab view): a Zoom button in the media bar opens a slider, Ctrl + wheel and
  Ctrl + 0/+/- zoom, dragging pans, and a minimap shows the visible part. Sharp Lanczos3 (or Catmull-Rom, Bilinear, Nearest)
  re-render in the background.
- Settings page Fullscreen > Image Zoom.

### Changed
- Action bars never float over anything but their own window (not while another application is active).

## [0.3.71] - 2026-10-20

### Added
- **Hover previews for animated pictures** (GIF, APNG, animated WebP, AVIF sequences) and an `ANIM` badge (Settings > Appearance > Show
  ANIM badge). The frames are sampled once by the thumbnail pipeline and stored like video previews, so hovering never opens the file;
  pictures indexed earlier are upgraded by a refresh you start.

## [0.3.70] - 2026-10-20

### Added
- **File formats:** one table of 47 formats (ids, extensions, decoder, MIME type) and a Settings > File Formats page with a switch per
  format, a search, counts of indexed files and a notice for hidden files. Switched-off formats are skipped by scans and hidden from lists,
  queries and counts, but never removed from the library.
- HEIC/HEIF through FFmpeg, including tiled pictures (HEIC photos, AVIF grids) and the stored rotation; SVG through resvg; GXF and
  WTV/DVR-MS videos; raw H.264/HEVC thumbnails. See `docs/file_formats.md`.

### Changed
- The content decides whether a file is an image or a video where containers are shared (an AVIF named `.mp4`); `.ts` files are only taken when
  they are transport streams.
- Phone videos are rotated from the display matrix (the old rotate tag no longer exists in current files).
- The refresh status line names skipped files; renaming a file into a switched-off format hides it at once and moves the folder counts;
  deleting a folder only deducts what its ancestors list; auto-extension appends the extension without a counter when the name is free.

### Fixed
- Thumbnails of streams whose first keyframe lies after 0 (FLV) no longer start with a seek that loses the stream.

## [0.3.69] - 2026-10-19

### Added
- The tab view says so (with a placeholder over its whole inner area) while its tab is in the fullscreen and its media are over there.

## [0.3.68] - 2026-10-19

### Added
- The tab grid shows a "Playing in fullscreen" badge instead of an empty black area for videos whose player is in the fullscreen.

## [0.3.67] - 2026-10-19

### Changed
- The default limit of the open confirmation is 5 files.

## [0.3.66] - 2026-10-19

### Added
- **Confirmation before opening many files at once** (Open, Open With, Open in File Manager, Open in (Background) Tabs) from N files
  on (Settings > General > Confirmations). Open in Fullscreen keeps its own tile limit and never asks.

### Changed
- The General settings page is regrouped into Root Directory, Session and Confirmations.

## [0.3.65] - 2026-10-19

### Changed
- A lone medium in the tab view fills the whole tab area (no bars around it); two or more media, the fullscreen and the tab grid keep
  the display-shaped arrangement.

## [0.3.64] - 2026-10-19

### Changed
- Escape in the filter field and in the tag filter field clears it and gives the keyboard focus back to the views, so arrow keys and
  type-ahead work again without a click. The selection stays.

## [0.3.63] - 2026-10-19

### Added
- A Multiview that follows the selection waits (default 400 ms of quiet, configurable, 0 = at once) after an item selected by typing.
- The status bar shows "No match for ..." for a moment when type-ahead finds nothing (option, on by default).

## [0.3.62] - 2026-10-19

### Added
- **Type-ahead options** in Settings > Appearance & Grid: enable, search timeout, new search after a pause (0 = off), also find folders,
  repeating a letter cycles, center the found item. Applied live.
- F3 puts the cursor into the filter field of the Browse view, like Ctrl+F.

## [0.3.61] - 2026-10-19

### Added
- **Type-ahead find** in the Browse view (grid and table): printable keys jump invisibly to the first item whose name starts with what was
  typed, folder rows included; a repeated key cycles, Backspace/Escape work while a search runs, shortcuts and other keys end it.

## [0.3.60] - 2026-10-19

### Added
- **Duplicate button** in the media action bar: shows a medium a second time in the Multiview, in the slot next to it, with its own
  player (muted by the sound policy) or the shared decoded pixels. Disabled with the reason as tooltip while the view shows the maximum
  of 4 media or the tile is broken.

## [0.3.59] - 2026-10-19

### Fixed
- The action bars are hidden while a dialog locks the main window, so none can be clicked above a dialog. A tab's fullscreen locks the main
  window on its own, so closing a dialog no longer releases that lock.

## [0.3.58] - 2026-10-19

### Added
- **Cut, copy and paste of media** through the system clipboard (Ctrl+X/C/V and the context menu). The files go on the X11 clipboard in the
  formats of Dolphin and the other file managers and are read from it; paste uses the existing conflict dialog, flags records of files
  that vanished and scans the target folder.
- Action bars for image and video tabs opened directly (mute, Open With, Delete and Close on the media bar; Close, play/pause and idle
  cursor on the primary bar).

## [0.3.57] - 2026-10-19

### Fixed
- Dialog heights: every dialog has a fixed width and its height follows the content, so wrapped text no longer clips the buttons.
- A freeze (deadlock) when deleting or renaming a favorite with Del/F2.
- Drops: nothing lights up and nothing moves while a scan or refresh runs; the target folder must exist and be writable and a failure names
  the reason; moving out of a read-only folder leaves no half-done copy; symbolic links are moved as links and loops are harmless.

## [0.3.56] - 2026-10-19

### Changed
- Folder tiles use a palette color for their preview area. The delete dialogs no longer show file sizes (nothing reads the drive for a size).

## [0.3.55] - 2026-10-19

### Changed
- The preview area of a folder tile (and of the "..." entry) is framed and rounded like the one of a media tile.

## [0.3.54] - 2026-10-19

### Added
- **Thumbnail badges for folders:** MISSING, HIDDEN, EMPTY, FOLDER and "N MISSING", governed by the media type / status option. The Flags
  column of the table lists the same states.

## [0.3.53] - 2026-10-19

### Changed
- The delete dialogs say how many of the items are already missing on disk (only when there are any).

### Fixed
- Deleting or purging a folder no longer leaves the records of its missing subfolders behind.

## [0.3.52] - 2026-10-19

### Added
- **Combined "Delete Files & Folders" dialog** for selections that hold folders: what is deleted on disk, what is only cleaned from the
  database, notes for covered and restored items. A file record is purged only after the file is really gone; a folder that is back on disk
  is restored, never purged.

### Fixed
- The folder snapshot is refreshed after a restore, a filter-pruned folder selection updates the Inspector, and the folder media count uses
  the tree counters.

## [0.3.51] - 2026-10-18

### Added
- **Folders join the Browse selection** (click, Ctrl, Shift, rubber band, Select All, keys) and ranges and arrow keys cross folders and media.
- Keys: Backspace / Alt+Up (parent folder), Alt+Left/Right (history), Ctrl+Shift+N (New Folder), Enter (opens the selected folder or medium).
- A context menu for the free space, a reduced menu for mixed selections, auto-scroll while dragging media near the top or bottom edge,
  folder dates in the table and the Inspector, a name filter for folder rows, and the first element is selected after going up.

## [0.3.50] - 2026-10-18

### Added
- **Drop media onto folder rows** (and onto "...", the parent): the row lights up and the same Copy / Move / Cancel flow as in the folder
  tree opens. Folder rows open the tree's folder context menu; F2 and Del act on the highlighted row.

### Changed
- The parent entry is labeled "...".

## [0.3.49] - 2026-10-18

### Added
- **New Folder** dialog (folder tree context menu and Browse toolbar): relative paths work like `mkdir -p` and the tree is patched in place.
- **Show Folders** toggle: lists the folders (and a virtual "up" row) in grid and table; no folders in queries or the recursive view.

## [0.3.48] - 2026-10-18

### Added
- The drop-down menus of the media action bar (Open With) are speech bubbles that stay open until you click elsewhere.

### Fixed
- Videos stayed black in software-GL environments: libmpv now gets `gpu-sw`, and a failing video output switch is logged.

## [0.3.47] - 2026-10-18

### Added
- **Open With** drop-down and a **delete button** in the media action bar; Del on a single medium in the fullscreen opens the regular delete
  dialog and confirmed deletions drop the tiles from the Multiview (the last medium closes it).

### Changed
- **Browse performance work** (measured in `docs/perf/RESULTS.md`): records shrank from ~1.2 KB to ~300 bytes, large lists load, filter
  and sort on a worker thread, no O(N) work per user event, thumbnails decode through libwebp on demand with several loader threads.

### Fixed
- The selection highlight and the middle click broke after deleting media until a folder change.

## [0.3.46] - 2026-10-16

### Changed
- **Browse view rebuilt for scrolling speed:** a ring pool of tiles in one stable model, a single byte-budgeted thumbnail RAM cache with
  on-demand loading (no preload of all thumbnails), an id-indexed media store and a status bar that costs O(selection). Thumbnails reach the
  UI through a wake-up instead of a 16 ms polling timer.
- A **RAM budget** setting (Settings > Performance, 0 = automatic) replaces the thumbnail cache slot count. No database or on-disk format
  change, no migration.

## [0.3.45] - 2026-10-15

### Changed
- **Faster, calmer start:** the window opens already maximized (no windowed flash) with its remembered maximized size, the two action bar
  windows are created when a Multiview opens, no subprocess runs before the window, the saved position is validated against the X screen
  size, and the folder audit and monitor diagnostics start after the first rendered frame.
- A startup timeline logs the elapsed time per launch stage and the first frame.

## [0.3.44] - 2026-10-14

### Fixed
- **Orderly shutdown:** closing the main window, `SIGINT`, `SIGTERM` and `SIGHUP` take one path that stops background work, closes every
  window, shuts players down before their windows are destroyed, flushes the settings and quits; a second signal or a watchdog ends a hang.
  Settings are written by one serialized writer, so background savers can no longer overwrite newer data.
- Closing a playing video tab or a Multiview no longer ends the application with exit status 1 (`BadWindow`).

## [0.3.43] - 2026-10-14

No user-visible changes (version bump while merging branches).

## [0.3.42] - 2026-10-14

### Added
- **The Multiview is the only fullscreen mode:** sticky tiles, per-tile width pins with draggable gutters, a follow planner that keeps
  the view on the Browse selection, previous/next for a single image, and a thumbnail placeholder while an image decodes.
- **Media action bar** (hover bar of a medium: sticky, open as tab, remove or close, mute) and a **primary action bar** (view-wide:
  send to a background tab, close, mute all, play/pause all, reset the arrangement, hide the idle cursor, pin the fullscreen), drawn as
  translucent windows of their own over native video windows.
- **Settings > Fullscreen:** bars (a switch each, one per button, reveal distance), sound policy (who gets the sound, pass it on),
  background color (theme, like the viewer or custom) and a reusable **color picker**.
- Every medium of a Multiview tab gets the bar; a Multiview tab of one medium is titled after its file and follows renames.
- In a 2x2 Multiview each column gets its own swap button.

### Changed
- Browse stays usable while a Multiview follows its selection; only a Multiview that belongs to a tab is isolated.

### Fixed
- Black check marks on the action bar buttons, and a created tab that stayed invisible until the view changed.

## [0.3.41] - 2026-10-14

### Added
- **Multiview:** "Open in Fullscreen" shows up to four media side by side in one frameless fullscreen window (a row, or 2x2 for four
  widescreen media) with draggable gutters, swap buttons and a sound policy (only one video is audible, new videos start muted).
- Ctrl+double click turns the fullscreen into a background tab (nothing restarts); a tab can be shown fullscreen again; the tab grid
  shows Multiview tabs as miniatures of their arrangement; F5 refreshes every member of a Multiview tab.
- Tooltips on disabled elements explain why an action is unavailable.

### Changed
- "Thumbnail (Random Pos.)" is enabled only for selections that contain a valid video.
- Log obfuscation also covers folder names, tab titles and favorite aliases.
- Media drag and drop is disabled while a fullscreen is open.

### Fixed
- Videos stayed paused after a view switch because an automatic pause was taken for the user's.

## [0.3.40] - 2026-10-13

### Added
- **Thumbnail badge system** with a video duration badge (bottom right, compact format such as `5s`, `04:30`, `01:02:03`) and overflow
  modes (wrap or hide); rating, tag and media type badges use the same engine. Settings > Thumbnail Badges.
- **Regenerate thumbnails** (toolbar button and context menu): replaces only the stored thumbnail and video preview frames of the selection,
  videos at a fresh random position, independent of Refresh; progress shows in the "Gen" bar.
- File conflict dialog: uses the stored thumbnails and generates one only as a fallback, with the user's thumbnail settings.

### Changed
- **New icon system:** every icon is a monochrome Tabler SVG (manifest-driven), crisp at any scale factor; emoji and Unicode symbols
  were replaced by real icons in dialogs, menus, tabs and the tree.
- Context menus: scrollable submenus of unlimited depth, a better "Move to..." design, tooltips for some entries; tag tree children are
  indented less.
- A project guide (`CLAUDE.md`) documents conventions and architecture.

### Fixed
- Several folder context menu and folder delete issues.

## [0.3.38] - 2026-10-12

### Changed
- **New folder scan system** (walker with mount filter): the folder tree refreshes when a folder is moved.

## [0.3.37] - 2026-10-11

### Added
- Hotkeys to select and unselect all, and Del to delete folders, favorites and media.

## [0.3.36] - 2026-10-10

### Added
- A **"last tags" panel** with a collapsible, modular layout and a vertical resizer.

## [0.3.34] - 2026-10-09

### Changed
- Context menu behavior for pending items improved.

## [0.3.33] - 2026-10-08

### Added
- Media added through the CLI API are processed automatically; the CLI API takes a `source` parameter.

## [0.3.30] - 2026-10-07

### Added
- Context menu entry to regenerate a thumbnail at a random position; the refresh entries moved into a submenu.

### Changed
- The info panel sits on the left in the sidebar as a collapsible panel.

## [0.3.29] - 2026-10-06

### Added
- **Favorites** (complete feature); folders and favorites are tab panels.
- All dialogs close with Esc.

### Fixed
- Middle mouse up/down was reserved in the fullscreen; closing tabs through the context menu was not recognized in the tab grid.

## [0.3.28] - 2026-10-04

### Added
- A display (monitor) target for the fullscreen view.

## [0.3.23] - 2026-10-03

### Changed
- Better tab context menu and z-ordering of context menus above mpv.

## [0.3.22] - 2026-10-01

### Fixed
- Restoring the window state (windowed, maximized).

## [0.3.21] - 2026-10-01

### Added
- F2 renames the selected or opened file or folder.

### Changed
- Better tree view refresh management; improved fullscreens (hotkeys, browse sync, non-destructive, faster).

### Removed
- All unit tests and benches of the first months (repository policy tests were added later).

## [0.3.19] - 2026-09-10

### Added
- Keyboard navigation for the grid and the table.

### Fixed
- The filter search field; a freeze caused by deleted files.

## [0.3.18] - 2026-09-03

### Changed
- Improved fullscreen behavior.

### Fixed
- A single click on a media item selects only that item.

## [0.3.17] - 2026-08-29

### Added
- **Tabs:** an option to allow duplicate tabs for a medium, a tab context menu with more closing options, drag and drop of tabs, mouse
  wheel tab switching, and the **tab grid** (masonry view).
- Middle click on a medium in the Browse view opens it fullscreen; Ctrl+double click adds it to a background tab; more "Open" entries in
  the grid item context menu.

### Changed
- Ctrl+W (instead of Esc) closes tabs; the tab grid has no right sidebar.

## [0.3.14] - 2026-08-26

### Added
- **Tab view** with a fullscreen view for images and **mpv embedded** for videos, with auto-pause and player instance management.

## [0.3.12] - 2026-08-24

### Added
- GIF support.

## [0.3.11] - 2026-08-24

### Changed
- Improved status bar layout.

## [0.3.10] - 2026-08-24

### Added
- **Rubber band selection**, a media type filter button (images, videos), fallback views (no results page) and a virtual folder button.

### Changed
- Better handling and display of corrupt or missing files, better folder selection when a folder is deleted, the missing files dialog
  uses a text area.

### Fixed
- The left resizer was inverted.

## [0.3.8] - 2026-08-21

### Added
- **Drag and drop of media files that also works outside of the application.**

### Changed
- Tag items have a dynamic and a maximum width.

## [0.3.7] - 2026-08-21

### Added
- An option to refresh only the metadata.

## [0.3.5] - 2026-08-20

### Added
- More file types, automatic renaming of supported files without an extension, a settings page for file types, more table columns and
  customizable grid items.
- A batch hash query endpoint for the CLI API.

### Fixed
- Short videos had no preview animation; hidden folders were always shown in the folder tree context menu; dragged items were shown with an
  offset instead of at the mouse cursor.

## [0.3.3] - 2026-08-19

### Added
- Drag and drop of files into folders.

### Changed
- Large performance gains; media items load lazily and thumbnail generation is separate from loading.

## [0.3.2] - 2026-08-18

### Added
- The XnView MP migration also outputs the tag tree and its mapping.

## [0.3.1] - 2026-08-18

### Added
- A database API, an option to obscure paths, file names and tag names in logs, a button to show or hide hidden folders in the tree, a
  "collapse all" button, an autoscroll to the selected folder and a "refresh folder" context menu entry.
- Extended context menu entries (Open, Open With, copy file names and paths, ...).

### Changed
- Faster folder tree.
- The built-in player was removed in favor of opening media with the default system handler (mpv returns in 0.3.14).

### Fixed
- Scan-lined video thumbnails, the throbber position, and the WebP thumbnail quality setting that was not applied.

## [0.3.0] - 2026-08-17

First usable version: the application skeleton grew into a working media browser within four days.

### Added
- **Library and browsing:** a media library with folders and tags, a folder tree (with a context menu) and a tag tree, a media grid and a
  table view with a column bar, filter and search fields, selection and keyboard behavior, multi selection and a context
  menu API.
- **Thumbnails:** fast generation and loading for images, quality options, video thumbnails with preview animations, and options for the
  preview quality.
- **UI:** toolbars for the folder, tag and media panels, resizers for the sidebars, tooltips, the tag and rating panel, icons that follow the
  dark theme, scroll bars without a horizontal scroll, and a first-frame render that does not wait for the data.
- **Refresh:** refresh for specific files, orphaned files are marked as deleted, dialogs for missing files and folders.
- **Settings:** options with one source of truth for the default values, settings that do not reload the UI on every change, and
  sanitation that no longer resets them silently.
- **Tools:** a package build for local installation and an XnView MP database migration API.
- Video playback through mpv (phase 4).

### Changed
- File hashes use MD5 instead of BLAKE3.

### Fixed
- The tag context menu was not shown.

## [0.1.0] - 2026-08-14

### Added
- Initial commit of the project.

[Unreleased]: https://github.com/vommie/rustview/compare/v0.6.11...HEAD
[0.6.11]: https://github.com/vommie/rustview/compare/v0.6.9...v0.6.11
[0.6.9]: https://github.com/vommie/rustview/compare/v0.6.7...v0.6.9
[0.6.7]: https://github.com/vommie/rustview/compare/v0.6.6...v0.6.7
[0.6.6]: https://github.com/vommie/rustview/compare/v0.6.5...v0.6.6
[0.6.5]: https://github.com/vommie/rustview/compare/v0.6.0...v0.6.5
[0.6.0]: https://github.com/vommie/rustview/compare/v0.5.1...v0.6.0
[0.5.1]: https://github.com/vommie/rustview/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/vommie/rustview/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/vommie/rustview/compare/v0.3.86...v0.4.0
[0.3.86]: https://github.com/vommie/rustview/compare/v0.3.85...v0.3.86
[0.3.85]: https://github.com/vommie/rustview/compare/v0.3.84...v0.3.85
[0.3.84]: https://github.com/vommie/rustview/compare/v0.3.83...v0.3.84
[0.3.83]: https://github.com/vommie/rustview/compare/v0.3.82...v0.3.83
[0.3.82]: https://github.com/vommie/rustview/compare/v0.3.81...v0.3.82
[0.3.81]: https://github.com/vommie/rustview/compare/v0.3.80...v0.3.81
[0.3.80]: https://github.com/vommie/rustview/compare/v0.3.79...v0.3.80
[0.3.79]: https://github.com/vommie/rustview/compare/v0.3.78...v0.3.79
[0.3.78]: https://github.com/vommie/rustview/compare/v0.3.77...v0.3.78
[0.3.77]: https://github.com/vommie/rustview/compare/v0.3.76...v0.3.77
[0.3.76]: https://github.com/vommie/rustview/compare/v0.3.75...v0.3.76
[0.3.75]: https://github.com/vommie/rustview/compare/v0.3.74...v0.3.75
[0.3.74]: https://github.com/vommie/rustview/compare/v0.3.73...v0.3.74
[0.3.73]: https://github.com/vommie/rustview/compare/v0.3.72...v0.3.73
[0.3.72]: https://github.com/vommie/rustview/compare/v0.3.71...v0.3.72
[0.3.71]: https://github.com/vommie/rustview/compare/v0.3.70...v0.3.71
[0.3.70]: https://github.com/vommie/rustview/compare/v0.3.69...v0.3.70
[0.3.69]: https://github.com/vommie/rustview/compare/v0.3.68...v0.3.69
[0.3.68]: https://github.com/vommie/rustview/compare/v0.3.67...v0.3.68
[0.3.67]: https://github.com/vommie/rustview/compare/v0.3.66...v0.3.67
[0.3.66]: https://github.com/vommie/rustview/compare/v0.3.65...v0.3.66
[0.3.65]: https://github.com/vommie/rustview/compare/v0.3.64...v0.3.65
[0.3.64]: https://github.com/vommie/rustview/compare/v0.3.63...v0.3.64
[0.3.63]: https://github.com/vommie/rustview/compare/v0.3.62...v0.3.63
[0.3.62]: https://github.com/vommie/rustview/compare/v0.3.61...v0.3.62
[0.3.61]: https://github.com/vommie/rustview/compare/v0.3.60...v0.3.61
[0.3.60]: https://github.com/vommie/rustview/compare/v0.3.59...v0.3.60
[0.3.59]: https://github.com/vommie/rustview/compare/v0.3.58...v0.3.59
[0.3.58]: https://github.com/vommie/rustview/compare/v0.3.57...v0.3.58
[0.3.57]: https://github.com/vommie/rustview/compare/v0.3.56...v0.3.57
[0.3.56]: https://github.com/vommie/rustview/compare/v0.3.55...v0.3.56
[0.3.55]: https://github.com/vommie/rustview/compare/v0.3.54...v0.3.55
[0.3.54]: https://github.com/vommie/rustview/compare/v0.3.53...v0.3.54
[0.3.53]: https://github.com/vommie/rustview/compare/v0.3.52...v0.3.53
[0.3.52]: https://github.com/vommie/rustview/compare/v0.3.51...v0.3.52
[0.3.51]: https://github.com/vommie/rustview/compare/v0.3.50...v0.3.51
[0.3.50]: https://github.com/vommie/rustview/compare/v0.3.49...v0.3.50
[0.3.49]: https://github.com/vommie/rustview/compare/v0.3.48...v0.3.49
[0.3.48]: https://github.com/vommie/rustview/compare/v0.3.47...v0.3.48
[0.3.47]: https://github.com/vommie/rustview/compare/v0.3.46...v0.3.47
[0.3.46]: https://github.com/vommie/rustview/compare/v0.3.45...v0.3.46
[0.3.45]: https://github.com/vommie/rustview/compare/v0.3.44...v0.3.45
[0.3.44]: https://github.com/vommie/rustview/compare/v0.3.43...v0.3.44
[0.3.43]: https://github.com/vommie/rustview/compare/v0.3.42...v0.3.43
[0.3.42]: https://github.com/vommie/rustview/compare/v0.3.41...v0.3.42
[0.3.41]: https://github.com/vommie/rustview/compare/v0.3.40...v0.3.41
[0.3.40]: https://github.com/vommie/rustview/compare/v0.3.38...v0.3.40
[0.3.38]: https://github.com/vommie/rustview/compare/v0.3.37...v0.3.38
[0.3.37]: https://github.com/vommie/rustview/compare/v0.3.36...v0.3.37
[0.3.36]: https://github.com/vommie/rustview/compare/v0.3.34...v0.3.36
[0.3.34]: https://github.com/vommie/rustview/compare/v0.3.33...v0.3.34
[0.3.33]: https://github.com/vommie/rustview/compare/v0.3.30...v0.3.33
[0.3.30]: https://github.com/vommie/rustview/compare/v0.3.29...v0.3.30
[0.3.29]: https://github.com/vommie/rustview/compare/v0.3.28...v0.3.29
[0.3.28]: https://github.com/vommie/rustview/compare/v0.3.23...v0.3.28
[0.3.23]: https://github.com/vommie/rustview/compare/v0.3.22...v0.3.23
[0.3.22]: https://github.com/vommie/rustview/compare/v0.3.21...v0.3.22
[0.3.21]: https://github.com/vommie/rustview/compare/v0.3.19...v0.3.21
[0.3.19]: https://github.com/vommie/rustview/compare/v0.3.18...v0.3.19
[0.3.18]: https://github.com/vommie/rustview/compare/v0.3.17...v0.3.18
[0.3.17]: https://github.com/vommie/rustview/compare/v0.3.14...v0.3.17
[0.3.14]: https://github.com/vommie/rustview/compare/v0.3.12...v0.3.14
[0.3.12]: https://github.com/vommie/rustview/compare/v0.3.11...v0.3.12
[0.3.11]: https://github.com/vommie/rustview/compare/v0.3.10...v0.3.11
[0.3.10]: https://github.com/vommie/rustview/compare/v0.3.8...v0.3.10
[0.3.8]: https://github.com/vommie/rustview/compare/v0.3.7...v0.3.8
[0.3.7]: https://github.com/vommie/rustview/compare/v0.3.5...v0.3.7
[0.3.5]: https://github.com/vommie/rustview/compare/v0.3.3...v0.3.5
[0.3.3]: https://github.com/vommie/rustview/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/vommie/rustview/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/vommie/rustview/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/vommie/rustview/compare/v0.1.0...v0.3.0
[0.1.0]: https://github.com/vommie/rustview/releases/tag/v0.1.0
