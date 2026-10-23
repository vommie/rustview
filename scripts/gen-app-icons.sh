#!/usr/bin/env bash
# Renders the PNG sizes of the app icon and the embedded window icon from packaging/rustview.svg.
# Needs `rsvg-convert` (package librsvg). The generated PNGs are committed, so a normal build does not need it.
set -euo pipefail
cd "$(dirname "$0")/.."
SVG=packaging/rustview.svg
command -v rsvg-convert >/dev/null || { echo "rsvg-convert is missing: sudo pacman -S librsvg" >&2; exit 1; }
mkdir -p packaging/icons crates/rustview/assets/app
for s in 16 22 24 32 48 64 128 256 512; do
  rsvg-convert -w "$s" -h "$s" "$SVG" -o "packaging/icons/rustview-$s.png"
done
cp packaging/icons/rustview-256.png crates/rustview/assets/app/window-icon.png
echo "icons written to packaging/icons and crates/rustview/assets/app/window-icon.png"
