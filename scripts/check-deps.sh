#!/usr/bin/env bash
# Verifies everything needed to build, test, package and run RustView on Arch/Manjaro.
# Nothing is skipped silently: every missing item is listed with the package that provides it, followed by the exact
# `sudo pacman -S` command. Exit status: 0 = all required items present, 1 = a required item is missing.
#
#   scripts/check-deps.sh            check everything
#   scripts/check-deps.sh --quiet    print only what is missing
set -uo pipefail
cd "$(dirname "$0")/.."

quiet=0
[ "${1:-}" = "--quiet" ] && quiet=1

if [ -t 1 ]; then ok=$'\e[32mok     \e[0m'; req=$'\e[31mMISSING\e[0m'; opt=$'\e[33mmissing\e[0m'; else ok="ok     "; req="MISSING"; opt="missing"; fi

missing_required=()
missing_optional=()

# check KIND TEST PACKAGE DESCRIPTION
#   KIND: required | optional
#   TEST: "cmd:<command>" (found in PATH), "pkg:<package>" (installed pacman package) or "pc:<name>" (pkg-config module)
check() {
  local kind="$1" test="$2" package="$3" what="$4" found=0
  case "$test" in
    cmd:*) command -v "${test#cmd:}" >/dev/null 2>&1 && found=1 ;;
    pkg:*) pacman -Qq "${test#pkg:}" >/dev/null 2>&1 && found=1 ;;
    pc:*) pkg-config --exists "${test#pc:}" >/dev/null 2>&1 && found=1 ;;
  esac
  if [ "$found" = 1 ]; then
    [ "$quiet" = 1 ] || printf '  %s %-24s %s\n' "$ok" "${test#*:}" "$what"
  elif [ "$kind" = required ]; then
    printf '  %s %-24s %s  [package: %s]\n' "$req" "${test#*:}" "$what" "$package"
    missing_required+=("$package")
  else
    printf '  %s %-24s %s  [package: %s]\n' "$opt" "${test#*:}" "$what" "$package"
    missing_optional+=("$package")
  fi
}

section() { [ "$quiet" = 1 ] || printf '\n%s\n' "$1"; }

section "Build tools"
check required cmd:rustc         rustup       "Rust compiler (rust-toolchain.toml pins the channel)"
check required cmd:cargo         rustup       "Cargo"
check required cmd:cc            gcc          "C compiler/linker for native dependencies"
check required cmd:pkg-config    pkgconf      "finds the system libraries"
check required cmd:git           git          "source control"
check required cmd:clang         clang        "bindgen generates the FFmpeg and mpv bindings"

section "Libraries (build and run time)"
for lib in libavcodec libavformat libavutil libswscale libswresample libavfilter libavdevice; do
  check required "pc:$lib" ffmpeg "FFmpeg development files"
done
check required pc:mpv            mpv          "libmpv (video playback)"
check required pkg:fontconfig    fontconfig   "font lookup"
check required pkg:freetype2     freetype2    "font rendering"
check required pkg:libxkbcommon  libxkbcommon "keyboard handling"
check required pkg:libx11        libx11       "X11 session"
check required pkg:libxcb        libxcb       "X11 session"
check required pkg:libxcursor    libxcursor   "X11 mouse cursors"
check required pkg:libxi         libxi        "X11 input"
check required pkg:libxrender    libxrender   "X11 rendering"
check required pkg:libxkbcommon-x11 libxkbcommon-x11 "keyboard handling on X11"
check required pkg:wayland       wayland      "Wayland session"
check required pkg:libglvnd      libglvnd     "OpenGL loader (rendering)"

section "Packaging"
check required cmd:makepkg       pacman       "builds the .pkg.tar.zst"
check optional cmd:namcap        namcap       "lints the PKGBUILD and the package"
check optional cmd:desktop-file-validate desktop-file-utils "validates the desktop file"
check optional cmd:appstreamcli  appstream    "validates the AppStream metainfo"
check optional cmd:rsvg-convert  librsvg      "renders the app icon PNGs (scripts/gen-app-icons.sh)"

section "Release"
check optional cmd:gh            github-cli   "publishes the GitHub release (scripts/release.sh --publish; gh auth login once)"

section "Quality checks"
check optional cmd:cargo-audit   cargo-audit  "checks dependencies for known vulnerabilities (.cargo/audit.toml)"
check optional cmd:cargo-deny    cargo-deny   "checks licenses, bans and advisories (deny.toml)"

section "Visual tests (only needed to take screenshots of the running app: rv-testenv session)"
check optional cmd:xdotool       xdotool      "clicks, typing and window lookup"
check optional cmd:import        imagemagick  "window screenshots"

print_command() {
  local -n list=$1
  [ "${#list[@]}" -gt 0 ] || return 0
  mapfile -t uniq < <(printf '%s\n' "${list[@]}" | sort -u)
  echo
  echo "$2"
  echo
  echo "    sudo pacman -S --needed ${uniq[*]}"
}

if [ "${#missing_required[@]}" -eq 0 ] && [ "${#missing_optional[@]}" -eq 0 ]; then
  [ "$quiet" = 1 ] || { echo; echo "Everything is installed."; }
  exit 0
fi
print_command missing_required "Required packages are missing. Install them with:"
print_command missing_optional "Optional packages are missing (the matching checks or features are unavailable until they are installed):"
[ "${#missing_required[@]}" -eq 0 ]
