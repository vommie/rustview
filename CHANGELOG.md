# Changelog

All notable changes to RustView are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/):
until 1.0, the patch number rises with every state that can be tested, the minor number with a milestone.

The entries before 0.3.82 were reconstructed from the git history (first to last commit); the versions are the ones set in
`Cargo.toml` at the time. They were never published as releases, but each one is tagged (`v0.1.0` to `v0.3.81`).

## [Unreleased]

### Added
- **Crop pictures** in every Multiview (fullscreen and tab): the media bar of an image has a **Crop** button. The crop tool draws a rectangle over the picture with
  corner and edge handles, the picture outside it dimmed and rule-of-thirds lines while you drag. Top left it shows the size of the crop in pixels of the picture
  (e.g. "893 x 1116 px"), top right a small **ratio button**: a left click goes through the landscape ratios, a right click through the portrait ones, **O** is the
  picture's own ratio, "Free" means no ratio. **Shift** while resizing keeps the ratio; drag inside to move, drag outside to draw a new rectangle. It works at every
  zoom (a zoomed picture starts with the part you see; the middle button pans, Ctrl + wheel and the minimap keep working). **Double click or Enter** applies the crop;
  nothing is written yet. **Esc** cancels the crop tool that was opened last (with several open, newest to oldest) before it does anything else, also from a
  video's window. Animated and multi-page pictures cannot be cropped (the button says why); a picture can only be edited in one view at a time.
- **Undo, Redo and Save** appear in the media bar while a picture has unsaved changes (Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y, Ctrl+S), and a dot marks its tab. The
  **Save** button is emphasized; its arrow offers **Save As...** (Ctrl+Shift+S) and **Discard Changes**. After saving it turns grey and fades away.
- **Saving keeps quality and data safe.** Save overwrites the original in its own format: a JPEG crop that starts on the 8/16 pixel blocks is cut out without
  any loss (like jpegtran), any other is encoded with the file's own quantization tables; PNG keeps its palette and bit depth, WebP stays lossless or lossy,
  a still GIF keeps its colors, TIFF its 16 bits. EXIF, ICC profile and XMP are kept (EXIF gets the new size, its stale preview is dropped). The file is written
  completely to a temporary file, read back and only then put in place; if it changed on disk since it was opened you are asked (Overwrite Anyway, Save As
  Copy, Discard & Refresh, Cancel). Read-only files are never overwritten. Afterwards the library record, the hash and the thumbnail are refreshed and every view
  of the picture shows the new file.
- **Save As / export dialog:** name, folder, format (JPEG, PNG, WebP, AVIF, TIFF, BMP) and its few options (quality, chroma subsampling, progressive, compression,
  lossless, AVIF speed), with a warning when a format loses something (transparency, 16 bits, metadata). Formats RustView reads but cannot write (HEIC, SVG,
  PSD, JPEG XL, JPEG 2000, EXR, ...) are saved through it. A copy in a library folder joins the library at once with the rating and tags of the original; the
  dialog remembers the last format and options.
- **No unsaved picture is lost unasked:** closing the fullscreen, closing tabs (also left/right/others/all/selected and from the tab grid), removing a tile, paging
  to another picture or following another selection first lists the pictures with unsaved changes, one checkbox each (save or discard), then Apply or Cancel.
  Quitting the application does not ask (the files are never touched by unsaved edits).
- **Settings > Image Edit:** the crop ratios (chips with their orientation, add, remove, defaults), the rule-of-thirds lines, "Keep JPEG crops lossless" (the
  rectangle snaps to the JPEG blocks), how the original is replaced (**safe**: a backup until the new file is in place, default; **to the trash**; **directly**)
  and the presets of JPEG, PNG, WebP, AVIF and TIFF. Settings > Fullscreen has a switch for the Crop button.

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

[Unreleased]: https://github.com/vommie/rustview/compare/v0.3.86...HEAD
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
