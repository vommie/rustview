# Video in Slint – acceptance checklist

The user's checklist for the acceptance ("Abnahme") of the branch `video-in-slint`: every behavior of videos in the
Multiview that was decided before this branch (`docs/video_controls.md`, `docs/mpv_ab_repeat.md`, `docs/multiview_focus.md`,
`docs/hotkeys.md`, the Multiview chapter of CLAUDE.md) and has to work the same afterwards, plus what the branch promises on
top (section R). Each item says what should happen and how to try it. Milestones tick off the items they touched in their
self-check ("checked in Mx"); the user's acceptance at the end goes through all of them. Before the acceptance the list is
also offered as a German questionnaire with a tick and a note field per item.

## Before you start

- Media: any videos work. In the test environment: `Videos/Clips` (14 short clips: 720p, 1080p, portrait, square, 2.39:1,
  silent, 60 fps), `Videos/Long` (90 s and 300 s, good for seeking), `Multiview-Tests` (4 images + 4 videos).
- **Tab view** = a video opened with a double click or Enter (a tab in the main window). **Fullscreen** = right click >
  "Open in Fullscreen", a middle click or F11. **Tab grid** = Alt+2 (all tabs as tiles).
- **Two or more videos side by side**: select 2-4 media in Browse and press F11, or use the **Duplicate** button of a video's
  media bar (top right of the video) to show the same video twice.
- For section D the mpv script `ab_repeat.lua` must be in `~/.config/mpv/scripts/` and switched on in Settings > Multiview.
- With and without the compositor: KWin's Alt+Shift+F12 switches compositing off and on. Items marked *(no compositor)*
  are tried once with it off.

## A. The picture

- [ ] **A1** A video plays in the tab view, in the fullscreen and (paused) in the tab grid, sharp and in the right
  shape. — *Test:* open `full_hd_h264.mp4`, `portrait_1080x1920.mp4` and `anamorphic_720x480.mp4` in each of the three.
- [ ] **A2** A lone video in a tab fills the whole tab area; mpv adds the black bars itself where the shape differs. —
  *Test:* open `cinemascope_2560x1072.mp4` in a tab, make the window narrower and wider.
- [ ] **A3** 2, 3 and 4 media are arranged as before (a row, or a 2x2 grid for four wide media); the gutters can be dragged,
  columns swapped. — *Test:* `Multiview-Tests`, select 2, 3, 4 media, F11; drag a gutter.
- [ ] **A4** The fullscreen background color (Settings > Multiview) also shows around a letterboxed video. — *Test:* set a
  color, open `square_1000x1000.mp4` in the fullscreen.
- [ ] **A5** A tab that is not shown pauses its videos and plays on when it comes back, unless you paused it yourself. —
  *Test:* play a video tab, switch to another tab, come back; then pause it, switch, come back: still paused.
- [ ] **A6** No video stays black or frozen while others play (also 4 videos at once, also 4K). — *Test:* four clips in the
  fullscreen; `Videos/4K`.
- [ ] **A7** Opening a video shows its first picture quickly, without a flash of another color. — *Test:* open several
  clips one after another.

## B. The mouse over a video

- [ ] **B1** A click on a video selects it (when 2+ media are shown); it never pauses it. — *Test:* two videos, click one.
- [ ] **B2** Double click: in a tab it opens the fullscreen, in the fullscreen it closes it. Middle click: the same. —
  *Test:* both in a tab and in the fullscreen.
- [ ] **B3** Ctrl + double click in the fullscreen (opened from Browse) turns it into a background tab; the video keeps
  playing there. — *Test:* F11 on a video, Ctrl + double click, look at the new tab.
- [ ] **B4** The mouse wheel over a video changes its volume by 5 % per notch; Ctrl + wheel zooms it toward the pointer. —
  *Test:* wheel up/down, then Ctrl + wheel.
- [ ] **B5** A right click on a video plays or pauses it. — *Test:* right click twice.
- [ ] **B6** The resting mouse cursor is hidden after the set time over a video exactly as over a picture, and any movement
  shows it again. — *Test:* primary bar's cursor button on, keep the mouse still over a video.

## C. The video control bar

- [ ] **C1** Each video has its own bar at its bottom: play/pause, current time, progress bar, remaining time, speaker and
  volume slider. — *Test:* two videos, hover the bottom of each.
- [ ] **C2** It shows while the pointer is near the bottom of the video (default) or, with the other setting, after any
  movement over the video, and goes after 1.5 s at rest; while paused only with the pointer. — *Test:* both settings
  (Settings > Multiview > Player).
- [ ] **C3** Style "bar" (whole width, ~70 % opaque) and "pill" (floating, with a margin); opacity and height (default
  30 px) can be changed and everything grows with the height. — *Test:* change each setting, look at the bar.
- [ ] **C4** The short fade (0.15 s) when the bar comes and goes (setting, default on; only with a compositor). — *Test:*
  watch the bar appear.
- [ ] **C5** Clicking and dragging on the progress bar seeks exactly by default; the bar shows the target until mpv is there;
  a bubble above it shows the time under the pointer. — *Test:* `Videos/Long`, click and drag.
- [ ] **C6** The seek modes (exact, like mpv's controller, keyframes, as mpv is configured) behave as named. — *Test:*
  switch the setting, drag.
- [ ] **C7** Times are short ("1:23", "-0:45", hours only for long videos) and do not move while playing; a click on either
  time switches both to milliseconds (until the video ends; the setting decides how a new video starts). — *Test:* click a
  time.
- [ ] **C8** Volume: the slider sets the level and never touches mute (a muted video stays muted, the slider is dimmed); the
  speaker unmutes a muted video, mutes an audible one and brings a silent one back to its last level. — *Test:* all three
  cases.
- [ ] **C9** A key that moves the video (jumps, frame steps, A-B keys) shows the bar for 1.5 s; the A-B loop's own jumps do
  not. — *Test:* press Right arrow without touching the mouse.
- [ ] **C10** No control bar while a dialog or a menu is open. — *Test:* press F2 over a video tab; right click a tab in
  the tab bar.
- [ ] **C11** *(no compositor)* The bar is drawn solid and still works. — *Test:* Alt+Shift+F12, hover the bar.

## D. The A-B group (mpv script ab_repeat.lua)

- [ ] **D1** In the bar between the remaining time and the speaker: A, B, AB, Save, Load in a capsule of their own. A and B
  are yellow when set; AB is yellow while the loop plays, blue when a valid A-B could play, crossed out without one; Load
  shows the number of saved repeats as a badge and is disabled at 0. — *Test:* set A (Home) and B (End), toggle AB.
- [ ] **D2** Holding AB (hold to confirm) clears A and B. — *Test:* hold AB for a second.
- [ ] **D3** Above the progress bar a "tab" grows out of the bar as soon as there is a saved repeat, A or B: same color,
  curved flanks, no seam where it meets the bar; in it the saved repeats as colored lanes (overlaps stronger), the current
  A-B in yellow (blue while its loop is off), the flags of A and B with their times. — *Test:* `Videos/Long` with saved
  repeats (`gen_ab_repeats.py` makes some).
- [ ] **D4** While a repeat loops the progress bar outside A-B is darker; a point set by the script itself is dashed. —
  *Test:* set only A (B is set automatically).
- [ ] **D5** Hovering a lane shows its name (and times, when switched on) in a small label that follows the pointer; a
  click loads it; the flags can be dragged. — *Test:* each, with the matching switches in Settings > Multiview.
- [ ] **D6** **Save bubble:** a click on Save opens a bubble above it with a text field (focused at once, placeholder
  "Range N") and a save button; Enter saves, Esc, a click outside or a second click on Save cancels; Save is yellow while it
  is open; a hint appears only while the name or the range already exists. — *Test:* each way to close it.
- [ ] **D7** **Load menu:** speech-bubble menu with search field (filters while typing), number, color stripe, name,
  start → end, length, a mini bar per row, the running repeat highlighted, rename and delete (hold) per row, "delete all"
  (hold) at the bottom; pages remembered per video; picking an entry jumps to A and plays. — *Test:* 26 repeats for paging
  (`gen_ab_repeats.py`), search, rename, delete.
- [ ] **D8** **Opening the saved menu does not make the playbar flicker.** — *Test:* open and close Load several times,
  watch the bar.
- [ ] **D9** **No strange gap** between the menu bubble and the playbar. — *Test:* look at the open menu.
- [ ] **D10** **The bubble's caret points at the Load button** (and the Save bubble's at Save), wherever the button is. —
  *Test:* a narrow and a wide video, pill and bar style.
- [ ] **D11** Mouse in the popups: clicking a row, holding to delete and the search field work with the real mouse; a short
  click on the video closes the menu. — *Test:* each.
- [ ] **D12** Two players of one file stay in sync: save, rename, delete in one shows in the other at once; A and B of the
  other are not touched. — *Test:* Duplicate a video, save in the left one, look at the right one.
- [ ] **D13** Keys act on the selected video only: Home = A, End = B, Ctrl+Delete = clear (and the keys without defaults once
  assigned). — *Test:* two videos, select one, press Home.
- [ ] **D14** *(no compositor)* Solid bar, the tab as a plain rectangle over the progress bar, menus fine. — *Test:*
  Alt+Shift+F12.

## E. The media bar (top right of each medium)

- [ ] **E1** It appears when the pointer comes near the top right corner of a medium's view, at the height of the primary
  bar, whatever the video's size or zoom; every tile has its own. — *Test:* 3 videos, move to each corner.
- [ ] **E2** Buttons: Mute, Sticky, Open as tab / in a background tab (Browse fullscreen), Remove, Duplicate, Zoom, Open
  With, Delete (and the edit buttons on pictures). Disabled buttons say why in the tooltip (e.g. Duplicate when the view is
  full). — *Test:* hover and click each on a video.
- [ ] **E3** "Open With" opens a speech-bubble drop-down below its button (caret under the button, button stays highlighted);
  it closes on a click outside, a second click on the button and on choosing an entry. — *Test:* each.
- [ ] **E4** The zoom button opens the zoom slider bubble; several sliders (one per tile) can be open at once. — *Test:* open
  two.
- [ ] **E5** A bar follows its medium when media come and go or the layout switches between a row and a 2x2 grid. — *Test:*
  Remove a tile while another's slider is open.
- [ ] **E6** Tooltips of the bar buttons follow the tooltip settings (on/off, delay). — *Test:* switch tooltips off.

## F. The primary bar (top left of the view)

- [ ] **F1** Buttons: Send to tab, Close, Mute All, Play/Pause All, Reset Layout, hide the resting cursor, Pin. — *Test:*
  each in the fullscreen with 2+ videos.
- [ ] **F2** A refused close of a pinned fullscreen makes the Pin button blink. — *Test:* Pin, then Esc.

## G. Menus, tooltips and dialogs over a video

- [ ] **G1** A context menu of the main window (right click on a tab of the tab bar, on the sidebar, the tag panel) shows
  completely over a video, nothing is cut or hidden; a click anywhere closes it and goes on to what is below (KDE style). —
  *Test:* a video tab, right click a tab title so the menu hangs over the video.
- [ ] **G2** Tooltips show over a video. — *Test:* hover a toolbar button whose tooltip lies over the video.
- [ ] **G3** Dialogs (F2 rename, Delete, Save As) open centered over their window, the window behind is locked (dimmed by
  KWin), a click on it brings the dialog to the front; no bar is shown or usable while it is open. — *Test:* F2 on a video in
  the fullscreen and in a tab.
- [ ] **G4** The settings window over a video tab: same as a dialog. — *Test:* Ctrl+Comma while a video plays.
- [ ] **G5** Opening and closing a menu over a playing video never flickers. — *Test:* open/close quickly a few times.

## H. Switching to another application

- [ ] **H1** Nothing of RustView floats over another application: no bar, menu, slider, hint or label. — *Test:* open a
  menu or zoom slider over a video, Alt+Tab to another program, look.
- [ ] **H2** Coming back shows what was open as before (an open slider comes back). — *Test:* Alt+Tab back.

## I. The selected medium (focus marks)

- [ ] **I1** With 2+ media, a click on a medium (or on any button of its media bar) selects it; hover, wheel, dragging,
  the minimap and Ctrl+click do not. — *Test:* click, then wheel over another one.
- [ ] **I2** Corner marks in the corners of the selected medium's VIEW (its column at the full row height, not the video
  itself), in the primary color, also where the video covers the corner; the free part of the view is brightened (Off /
  Weak / Medium / Strong; marks Short / Medium / Long). — *Test:* a letterboxed and a full video, a zoomed one; each setting.
- [ ] **I3** The marks hide with the resting cursor (when "Hide the selection marks too" is on) and a key that acts on the
  selected medium shows them for about a second. — *Test:* let the cursor hide, press Space.
- [ ] **I4** With 2+ media and none selected, a key that needs a medium does nothing and the hint "Select a medium first"
  (circle with hand icon, one line) shows centered over the view, also over a video; it holds and fades like the rating
  icon. Other refusals name their reason ("Nothing to save", ...). — *Test:* open 2 videos, press Ctrl+3 without clicking.
- [ ] **I5** Tab view panels follow the selected medium (Inspector, Tag & Rate); without one they say "Select a medium in
  the view". — *Test:* a merged tab of two videos.

## J. Feedback over a video

- [ ] **J1** Ctrl+0..5 rates the selected video and shows the rating icon centered over it for 750 ms, then a 250 ms fade;
  a new rating replaces it at once. — *Test:* Ctrl+4, then quickly Ctrl+2.
- [ ] **J2** The rating animation can be switched off (Settings > Multiview > Rating); the rating is saved anyway. —
  *Test:* switch off, rate, look at the badge in Browse.
- [ ] **J3** The mute key (M) shows the speaker icon (crossed out when muted) like the rating icon; the mute buttons show
  none. — *Test:* M, then the bar's speaker.
- [ ] **J4** The wheel and the Louder/Quieter keys show a flat, semi-transparent volume bar centered in the lower part of the
  picture, at most half its width, dimmed while muted, same timing; the bar's slider shows none. — *Test:* wheel, Ctrl+Up,
  then the slider.
- [ ] **J5** The feedback also shows over a paused video. — *Test:* pause, Ctrl+3.

## K. Keys

- [ ] **K1** Video player keys act on the selected video wherever the pointer is: Space play/pause, M mute, Left/Right jump,
  Shift+Left/Right long jump, Comma/Period frame steps, Ctrl+Up/Down volume, [ / ] speed. — *Test:* two videos, select
  the right one, keep the pointer over the left one, press each key.
- [ ] **K2** A key pressed with the pointer over a video reaches RustView (never mpv's own keys: no screenshots, no audio
  delay, no quit). — *Test:* Ctrl+S, s, Ctrl+Plus, Ctrl+C over a video.
- [ ] **K3** DEL opens the delete dialog for the selected medium (also in the tab view); F2 renames it. — *Test:* each.
- [ ] **K4** Esc: closes a crop tool first, then the fullscreen (a pinned one refuses); in the tab view it never opens the
  fullscreen. — *Test:* Esc over a video in a tab.
- [ ] **K5** F11 / F show a tab in the fullscreen and leave it; Ctrl+W closes. — *Test:* each.
- [ ] **K6** Changed keys (Settings > Hotkeys) work over videos too. — *Test:* rebind "Play or pause" to P.

## L. Sound

- [ ] **L1** "Sound follows" (Settings > Multiview > Sound): the first video (default), the newest one, the selected one, or
  nobody: only that video is audible, the others are muted. — *Test:* 3 videos with sound, each setting, click another video.
- [ ] **L2** A duplicate starts muted; Mute All mutes all and brings them back. — *Test:* Duplicate, Mute All.
- [ ] **L3** Merging tabs keeps the sound choices the user made. — *Test:* mute one video tab, merge it with another.

## M. Video zoom and minimap

- [ ] **M1** Ctrl+wheel zooms toward the pointer, Ctrl+Plus/Minus and Ctrl+Backspace (reset) zoom the selected video, the
  media bar's zoom slider (logarithmic, marks, percentage below the bubble) too. — *Test:* each on `full_hd_h264.mp4`.
- [ ] **M2** A zoomed video grows into the free room of its tile (a portrait video over the whole width when alone); the
  picture can be dragged; never an empty margin when it is larger than the room. — *Test:* `portrait_1080x1920.mp4` alone
  in the fullscreen, zoom in, drag.
- [ ] **M3** Going from 100 % to zoomed and back shows no stale or stretched frame. — *Test:* Ctrl+wheel one notch in and
  out several times.
- [ ] **M4** The minimap (bottom corner) shows the whole video with the visible part marked, its picture follows the video
  (refreshed while it plays); drag in it moves the view finely (the pointer is held and hidden), middle click resets, right
  click opens a slider bubble above it. — *Test:* each.
- [ ] **M5** While zoomed, the control bar, the media bar and the focus marks stay at the right places and nothing covers
  the minimap. — *Test:* zoom, hover the bottom, select the tile.

## N. Tab grid

- [ ] **N1** Video items are paused and show the video's picture; the mouse works like over a picture item: click opens the
  tab, Ctrl/Shift+click select, middle click opens the fullscreen, right click the tab's menu, the wheel scrolls the grid
  smoothly, the blue hover frame shows. — *Test:* Alt+2 with several video tabs.
- [ ] **N2** The video under the pointer gets a small bar (play/pause, progress; a click on it controls the video instead
  of opening the tab); every other video shows a time badge "0:03 / 0:08" bottom right, hidden while the small bar shows. —
  *Test:* hover, play one in the grid.
- [ ] **N3** Scrolling the grid moves and clips the videos exactly with their items (no picture outside the grid, nothing
  lags behind). — *Test:* many tabs, scroll fast.
- [ ] **N4** Grid keys: arrows move the mark, Enter opens, Del/Ctrl+W close, F11 opens the fullscreen. — *Test:* each.
- [ ] **N5** A tab that is shown in the fullscreen shows "Playing in fullscreen" on its video tiles in the grid, and the tab
  view of that tab shows the away panel; both go only after the fullscreen closed and the tab shows its videos again. —
  *Test:* F11 on a video tab, Alt+2 in the main window (second monitor or after Alt+Tab), close the fullscreen.

## O. Handover between windows

- [ ] **O1** Tab → fullscreen → back: position, pause state, volume, zoom and A-B stay; no visible gap or black frame. —
  *Test:* play a tab to 0:30, F11, Esc.
- [ ] **O2** Fullscreen → background tab (Ctrl + double click or "Send to tab"): the videos run on in the tab. — *Test:*
  both ways.
- [ ] **O3** Merging tabs (Ctrl+click two tabs > Merge, or Merge With Left/Right) keeps every video running with its
  position and choices; Duplicate shares the same file in a second player. — *Test:* merge two playing video tabs.

## P. Robustness

- [ ] **P1** Closing a tab with playing videos, closing the fullscreen, quitting RustView (window close, Ctrl+C in the
  terminal) never hangs or crashes. — *Test:* each with 4 videos playing.
- [ ] **P2** Settings > Debug: the video self-test passes and the page names the way videos are drawn. — *Test:* run it.
- [ ] **P3** A corrupt video shows its error tile, the others go on. — *Test:* `Corrupt/` with a good video.
- [ ] **P4** *(no compositor)* Everything above still works with KWin compositing off. — *Test:* Alt+Shift+F12, repeat a
  few items of C, D, E.

## Q. Quick performance look (no benchmarks)

- [ ] **Q1** Four 1080p videos play smoothly in the fullscreen while you move the mouse and open menus. — *Test:* look.
- [ ] **Q2** RustView idles quietly with videos paused (no busy CPU in the system monitor). — *Test:* pause all, look at
  the CPU for 10 s.

## R. What the branch adds

- [ ] **R1** No extra windows over videos: bars, menus, sliders and the hint are part of the RustView window (KDE's task
  switcher and `xwininfo -tree` show no extra windows). — *Test:* open a bar and a menu, Alt+Tab list.
- [ ] **R2** Native Wayland is the default (decision 2026-10-26, tested together): in a Plasma Wayland session RustView runs
  without XWayland and everything above works; what native Wayland still lacks (dragging files to other programs, dialogs that
  belong to their window, pointer lock of the minimap) is solved or accepted by you. — *Test:* log into Plasma (Wayland),
  repeat A, C, D, G, then switch to X11/XWayland (`RUSTVIEW_WINDOW_SYSTEM`) and repeat a few.
- [ ] **R3** The UI looks right under the renderer chosen after the AMD measurement (skia unless something spoke against it):
  text, rounded corners, icons, thumbnails. — *Test:* look through Browse, Settings, a dialog and the Multiview.
