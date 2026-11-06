# AGENTS.md

Rules for anyone, human or coding agent, who changes RustView. Keep changes small and in line with these rules.

## Stack

- Rust and Slint only. Library in LMDB and SQLite, video through system libmpv (OpenGL render API, drawn by Slint) and
  FFmpeg. No web views, no Python or shell helpers at runtime, no extra runtimes.
- Target: Arch Linux / Manjaro with KDE Plasma. X11 is the default, Wayland must keep working; ask
  `window_system::current()`, never environment variables. X11 code only in `crates/rustview/src/viewer/x11/` (x11rb).
- `crates/rustview-core` has no UI and never depends on `crates/rustview`.

## Licenses and third-party material

- Own code is MIT; the built program is GPL-3.0-only (libmpv, FFmpeg, Slint under GPL-3.0). Keep AboutSlint in the
  About card.
- Never copy code, icons, fonts or other assets from other projects. Use a dependency, or write it yourself. Check every
  new dependency's license first; `deny.toml` and `about.toml` define what is accepted.
- `ATTRIBUTION.md` is generated: run `scripts/attribution.py` after every dependency change (`check-all.sh` fails
  while it is stale). Notices for non-crate material (icons, fonts, bundled C code) go into `scripts/attribution-head.md`.

## Icons

- Only Tabler Icons (MIT), monochrome, from `crates/rustview/assets/icons/tabler/`. Add one with a line in
  `assets/icons/icons.toml` and `scripts/sync-tabler-icons.sh`; keep Tabler's file names. No emoji or Unicode symbols
  as icons. `crates/rustview/tests/icon_policy.rs` enforces this. Remix Icon is not allowed (license).
- Exceptions: the colored rating icons in `assets/icons/custom/` and the app icon `packaging/rustview.svg`.

## Code

- Prefer maintained crates over handwritten code; own code is domain glue. Name a new crate in the commit message and
  add it to `[workspace.dependencies]`. A crate must not add drive access or work to a hot path.
- US English everywhere: code, comments, logs, UI strings, docs, commits.
- `clippy::pedantic` stays on; no lossy `as` casts (`rustview_core::util::cast`); every `unsafe` block has a
  `// SAFETY:` comment; no `unwrap()` on user data or I/O. Files under ~600 lines; `src/` holds only `lib.rs`,
  `main.rs` and module folders.
- Never touch the drive except on an explicit refresh (no `exists()`/`metadata()` on startup, selection, menus, hover).
  No polling or repeating timers while idle.
- User data lives in the XDG folders (`~/.config/rustview`, `~/.local/share/rustview`). Database and settings changes
  stay backward compatible, with an automatic migration and a backup first. Edits never damage an original file.
- Never remove features, tests or log statements to make something pass.

## UI

- Dialogs follow `crates/rustview/tests/dialog_layout_policy.rs` and use `dialog_fit` / `dialog_parent`; nothing is always-on-top.
- Every key action is an `Action` of the keymap. Tooltips go through `ui/global_tooltip_layer.slint`.
- A new window is listed in `src/ui/shutdown/windows.rs` (`crates/rustview/tests/window_shutdown_policy.rs`).

## Before a commit

- `cargo fmt --all` and `cargo cl -- -D warnings` clean, zero compiler warnings, relevant tests green
  (`cargo t`). Before a release: `scripts/check-all.sh` (`scripts/check-deps.sh` lists missing tools).
- Every user-visible change gets an entry under `[Unreleased]` in `CHANGELOG.md` (Keep a Changelog, SemVer) in the
  same commit.
