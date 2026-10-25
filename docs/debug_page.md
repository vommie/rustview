# Settings > Debug: checks, log, report and tools

The Debug page answers "is everything RustView needs in place, and is RustView itself healthy?", keeps the log so that a
problem can be looked at afterwards, and has the tools for hunting a bug. Modeled on RustStart's "System check" and "Debug"
pages (`ruststart/crates/ruststart/src/diagnostics.rs`, `src/app/debug.rs`), tailored to what RustView really uses.
User decisions: questionnaire `~/claude/feedback/questionnaires/rustview/2026-10-25-rustview-debug-seite` (2026-10-25).

## Decisions (questionnaire 2026-10-25)
| Question | Decision |
|---|---|
| Layout | One page of collapsible cards; RustView remembers which cards are open (`view.debug_page`). |
| Order of the results | Two cards, **Dependencies** (programs and libraries) and **Health** (RustView's data and environment), side by side when the window is wide, one below the other when it is narrow. |
| Rows | Problems first; everything that passed is one row ("17 checks passed") that opens on a click. |
| Missing program | The exact command in the row with a copy button; below the card one combined `sudo pacman -S --needed ...` line when two or more packages are missing. |
| When checks run | Only on a click of the check button. What RustView already knows in memory (window system, graphics, RAM budget) is shown at once. |
| Dependencies | Videos (libmpv, FFmpeg + versions), copying pictures (xclip / wl-clipboard), monitor detection (kscreen-doctor, xrandr), file dialogs and Open With (desktop portal, gio, xdg-open), the file manager, X11 features (SHAPE, XFixes, a compositor), and a collapsed group **Developer** (samply, `perf_event_paranoid`). No build tools (`scripts/check-deps.sh` has them). |
| Health | Library database (size, media, folders, tags), folder tree consistency, thumbnails, the settings file, free space, the drive of the root folder, window system and graphics, the RAM budget as a short status with a jump to Performance & Cache (its warning stays only there). Not: monitors (Multiview page), trash (would wake the media drive). |
| Hint when a feature fails | **No**: missing programs are reported on the Debug page only, never in the status line. |
| Log storage | The last lines in memory, log files only when switched on (off by default). User addition: **two logs**, a private one with names hidden (to share) and a real one without (for oneself). |
| Log view | A **window of its own** that follows the log live (not on the Debug page: the settings window locks the main window). |
| Log level | Chosen on the page, applied after Save without a restart, stored; `RUST_LOG` wins when it is set and the page says so. |
| mpv messages | mpv's warnings and errors go into RustView's log; a switch writes mpv's detailed log to `mpv.log`. |
| Tools | Open the folders (settings, library + thumbnails, logs) and copy their paths, live runtime values, the startup timeline of this session, a video self-test with a test picture mpv makes itself, reset the window size and position, reset the layout (held), and `rustview check` on the command line. |
| Without a question (no objection) | Results only on this page; open cards remembered; the diagnostic report can be copied and saved, contains version, system, window system, graphics, the check results, the changed settings, the startup timeline and the last 300 log lines, names hidden when "Obfuscate" is on; the obfuscation checkbox stays as a card "Privacy"; checks read numbers from the database and never walk the media. |

## No drive access without a click
Nothing of this page touches a drive on its own. The checks (looking up programs in `PATH`, the size of the database file,
the settings file, free space) run on a worker thread when the check button is clicked; numbers of media, folders, tags and
thumbnails come from the database; the drive of the root folder is judged from the mount table (`/proc/self/mountinfo`),
never by touching the folder. The log is kept in memory; files are written only while the user has switched them on.

## Logging (`rustview_core::logging`)
- **Two renderings of every line.** `LogObscurer`'s helpers (`path`, `folder`, `media`, `filename`, `target`, `tag`, `query`)
  return values that decide how to print themselves when they are formatted, not when the log call is made. The capture
  layer formats each event twice, once with names shown ("real") and once with names hidden ("private"), by switching the
  obscurer's thread-local mode around each formatting. Everything else (the terminal, `println!` of the migration tool)
  follows the setting `debug.obfuscate_logs` as before.
- **In memory:** a ring of the last 5,000 lines with both renderings, a level and a sequence number (`LogRing`). The log
  window and the report read it; the window polls the sequence number while it is visible.
- **Files (switch `debug.log_files`, off):** in `$XDG_STATE_HOME/rustview/` (`~/.local/state/rustview/`): `rustview.log` (real)
  and `rustview-private.log` (names hidden), a new pair per start, the last 5 kept (`file-rotate`, written through
  `tracing-appender`'s non-blocking writer). Switched on during a session, the files start with the lines already in memory.
- **Level (`debug.log_level`, default `info`):** one reloadable filter for the terminal, the memory and the files. `RUST_LOG`
  replaces it when set.
- **mpv:** every player asks libmpv for its messages (`warn`, or `v` while `debug.mpv_log` is on). Warnings and errors become
  `[mpv: <module>]` log lines (the player's file name is hidden in the private rendering); with the switch on, every message
  is appended to `mpv.log` next to the log files (one per start, the last 5 kept).

## Checks (`rustview_core::diagnostics`, app facts in `system::diagnostics`)
Pure evaluation: the checks are made from collected facts (`DependencyFacts`, `HealthFacts`), so every rule is unit tested;
collecting the facts is the only part that looks at the system. A check has an id, a group, a status (OK, info, warning,
problem), a title, a detail line and an optional fix (packages for `pacman`, or commands such as `sysctl`).
`rustview check` prints the same checks in the terminal (exit status 1 when a problem was found); it knows everything except
what only the running window can tell (OpenGL renderer, mpv's GPU context of a running video).

## Tools
- **Video self-test:** plays `av://lavfi:testsrc2` for three seconds in a small mpv window of its own, placed over the
  settings window, with the same video options as a real player (GPU contexts limited to X11, `gpu-sw`). It reports the time
  to the first frame, the GPU context and video output mpv chose, the decoding and the dropped frames. No file is read.
- **Reset window:** the main window gets its default size, centered on its monitor, not maximized.
- **Reset layout (held):** sidebar widths, the inspector, the panel tabs of the left sidebar, the column widths of the table,
  grid or table and the tab grid columns go back to their defaults. Sort order and filters stay.
- **Startup timeline:** `startup_clock` keeps the stages of this launch in memory (`[Startup: Stage]`).
- **Runtime:** version and build, process (pid, uptime, threads), RAM, thumbnail cache, players, window system, graphics,
  environment overrides (`RUST_LOG`, `RUSTVIEW_WINDOW_SYSTEM`, `SLINT_*`), log level; refreshed every second while the card is
  open and the page is shown.
