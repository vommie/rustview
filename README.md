# RustView

A fast media browser for images and videos on Linux. RustView keeps its own library of your folders, tags and ratings, scrolls
through very large collections without stutter and never touches the drive unless you ask for a refresh. It is written in Rust with a
Slint user interface; videos play through the embedded mpv player and most formats are decoded by FFmpeg.

![The Browse view: folder tree, thumbnail grid with badges, tags and ratings](docs/screenshots/browse-grid.png)

## Features

- **Browse** your folders as a thumbnail grid or a sortable table with a resizable column bar. Thumbnails carry badges for the rating, tags,
  media type and video duration; hovering a video or an animated picture plays a short preview. Rubber band selection, keyboard
  navigation and type-ahead find (just start typing a name) work in both views.
- **Folder tree and favorites** with hidden-folder handling, drag and drop of files onto folders, New Folder, rename, move and delete
  dialogs that explain exactly what happens on disk and in the library, and a history (`Alt+Left/Right`).
- **Tags and ratings:** a hierarchical tag tree, a tag history, ratings from 1 to 5 and queries by tag and rating. **Ctrl+0** (no rating) to **Ctrl+5** rate the selection, and in a Multiview every medium that is shown, with a short animation of the new rating.
- **Multiview:** up to four images or videos side by side in a fullscreen, or as a tab. Tiles follow your selection, can be pinned
  ("sticky"), swapped, duplicated and resized with gutters; every tile has a media action bar and **zoom** for images (slider, `Ctrl` + wheel,
  minimap).
- **Crop pictures** in the Multiview: a modern crop tool with live pixel size, aspect-ratio presets (left click landscape, right click portrait, O = original,
  Shift keeps the ratio) that works at every zoom; Undo/Redo, and Save that keeps quality (lossless JPEG crops on the block grid, the file's own quality
  otherwise, lossless formats stay lossless, EXIF/ICC/XMP kept) and never damages the original (written completely and checked first, conflicts are asked
  about). Save As exports to JPEG, PNG, WebP, AVIF, TIFF or BMP with their options; formats RustView only reads are saved that way.
- **Tabs and tab grid:** open media in tabs and see them all in a masonry grid; a lone medium fills the whole tab. Select tabs with Ctrl/Shift+click
  and **merge** them into one Multiview tab (videos keep playing), or merge a tab with its neighbors from its menu.
- **Formats:** 47 image and video formats (JPEG, PNG, WebP, AVIF, HEIC, TIFF, GIF, SVG, EXR, MP4, MKV, WebM, raw H.264/HEVC, ... see
  [docs/file_formats.md](docs/file_formats.md)), each switchable in the settings; the content, not the extension, decides what is an image
  or a video.
- **File operations:** copy, move, rename and delete (to the trash or permanently) with conflict handling, cut/copy/paste through the system
  clipboard and drag and drop to other applications.
- **Fast by design:** a ring pool of tiles keeps scrolling O(visible rows), records take about 300 bytes, large lists are loaded on a worker thread and a
  RAM budget bounds the thumbnail cache ([measurements](docs/perf/RESULTS.md)).
- **Private by design:** optional log obfuscation hides file names, paths and tag names in the log output.
- **Scriptable:** a headless command line API reads and writes ratings, tags and hashes for other tools, and a migration tool imports an
  XnView MP database.
- **Everything persists:** window state, sidebar widths, collapsed panels, the open folder, filters and selections.

| | |
| --- | --- |
| ![Table view with ratings and tag counts](docs/screenshots/browse-table.png) | ![Context menu of a selection](docs/screenshots/context-menu.png) |

![Multiview: four media in a 2x2 arrangement with the media action bar](docs/screenshots/multiview.png)

## Platform

RustView targets **Arch Linux and Manjaro with KDE Plasma**. X11 is the default session. In a Wayland session RustView runs as an X11
program through XWayland (part of KDE Plasma), so videos and everything else work as on X11. Without XWayland, or with
`RUSTVIEW_WINDOW_SYSTEM=wayland`, it runs as a native Wayland program: videos and everything over them work there too (they are drawn
by the app itself), only a few X11 features degrade (dragging files to other programs, dialogs tied to their window, the pointer lock
of the minimap drag). Other distributions and desktops are not a target.

## Install

**Download the newest release** (Arch Linux, Manjaro) with the [GitHub CLI](https://cli.github.com/) (`sudo pacman -S github-cli`, then
`gh auth login` once; this also works while the repository is private). The [Releases page](https://github.com/vommie/rustview/releases)
lists every version with its changelog:

```bash
gh release download --repo vommie/rustview --pattern 'rustview-x86_64.pkg.tar.zst' --pattern SHA256SUMS
sha256sum -c SHA256SUMS --ignore-missing
sudo pacman -U rustview-x86_64.pkg.tar.zst
```

The package needs `ffmpeg` and `mpv` (both are installed as dependencies).

### Build from source

```bash
sudo pacman -S --needed rustup base-devel git clang pkgconf ffmpeg mpv
git clone git@github.com:vommie/rustview.git && cd rustview
cargo build --release            # a cold build takes about 10 minutes
./build-package.sh               # optional: builds dist/rustview-<version>-1-x86_64.pkg.tar.zst
```

Your library lives in `~/.local/share/rustview`, the settings in `~/.config/rustview/settings.json`. A new version always reads the
library and the settings of an older one; a schema change that old data cannot survive is not accepted.

## Command line

```bash
rustview api tag-tree --json
rustview api get-media --path /media/clip.mp4 --json
rustview api set-media --path /media/clip.mp4 --rating 5 --tags 1,4,7 --source MyTool --json
rustview api check-hashes --hashes-file hashes.txt --json
rustview --migrate-xnview ~/.config/xnviewmp/XnView.db --yes      # import an XnView MP database
rustview api --help
```

## Development

[CLAUDE.md](CLAUDE.md) holds the rules and the architecture notes of the project (performance principles, Multiview, shutdown, startup,
icons, dialogs). Cargo aliases: `cargo ck` (check), `cargo cl` (clippy), `cargo t` (tests), `cargo br` (release build). The changes of every
version are in [CHANGELOG.md](CHANGELOG.md).

## License

RustView is released under the [MIT License](LICENSE).

### Built with

- **[Slint](https://slint.dev)**, used under the Slint Royalty-free License 2.0. RustView is made with Slint (<https://slint.dev>).
- [mpv](https://mpv.io) (libmpv) for video playback and [FFmpeg](https://ffmpeg.org) for decoding.
- [Tabler Icons](https://tabler.io/icons) (MIT) for the interface icons; the app icon is built from two of them (the grid and the eye).
- [LMDB](https://www.symas.com/lmdb) through [heed](https://github.com/meilisearch/heed), [image](https://github.com/image-rs/image),
  [resvg](https://github.com/linebender/resvg) and many more crates (see `Cargo.lock`).
