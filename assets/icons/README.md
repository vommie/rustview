# RustView icons

All application icons are monochrome [Tabler Icons](https://tabler.io/icons) (MIT, see `tabler/LICENSE`), except the
colored tag and rating icons in `custom/`.

| Path | Content |
| --- | --- |
| `icons.toml` | **Single source of truth.** Maps every semantic icon id to a Tabler icon (or a custom SVG). |
| `tabler/outline/`, `tabler/filled/` | Tabler SVGs with their **original file names** (managed by `scripts/sync-tabler-icons.sh`, do not edit). |
| `custom/` | `tag.svg` and `rating_0..5.svg` (multi-colored, never tinted). |

## Adding or changing an icon

1. Pick the icon on <https://tabler.io/icons> and note its file name (e.g. `folder-open`).
2. Add one line to `icons.toml`, e.g. `folder_open = { tabler = "folder-open", doc = "Open folder" }`
   (`style = "filled"` selects the filled variant).
3. Run `scripts/sync-tabler-icons.sh` (downloads the pinned npm package, verifies its sha512, copies only the used files).
4. Use `Icon::FolderOpen` in Rust or `IconId.folder-open` in Slint.

`build.rs` validates the manifest and generates the Slint enum `IconId` and the Rust enum `crate::icons::Icon` from it.
`tests/icon_policy.rs` verifies that `tabler/` and the manifest agree and that no emoji/Unicode symbols are used as icons.
