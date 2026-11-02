#!/usr/bin/env bash
# Runs every check the project has, in order, and reports each step. Nothing is skipped silently: a step whose tool is
# missing is reported as SKIPPED with the package to install, and the script then exits with a failure.
# "finalisiere" runs this without options before anything is merged.
#
#   scripts/check-all.sh           everything including the release build and the package (a cold run takes ~30 minutes)
#   scripts/check-all.sh --fast    without the release build and the package
set -uo pipefail
cd "$(dirname "$0")/.."

fast=0
[ "${1:-}" = "--fast" ] && fast=1

if [ -t 1 ]; then g=$'\e[32m'; r=$'\e[31m'; y=$'\e[33m'; n=$'\e[0m'; else g=""; r=""; y=""; n=""; fi
failed=()
skipped=()

version=$(sed -n '/^\[workspace.package\]/,/^\[/{s/^version *= *"\(.*\)"/\1/p}' Cargo.toml)

# step NAME COMMAND...
step() {
  local name="$1"; shift
  printf '\n== %s\n' "$name"
  if "$@"; then
    printf '%s-> ok%s\n' "$g" "$n"
  else
    printf '%s-> FAILED: %s%s\n' "$r" "$name" "$n"
    failed+=("$name")
  fi
}

# needs COMMAND PACKAGE NAME : true when the tool exists, otherwise records the skipped step
needs() {
  if command -v "$1" >/dev/null 2>&1; then return 0; fi
  printf '\n== %s\n%s-> SKIPPED: %s is not installed (sudo pacman -S %s)%s\n' "$3" "$y" "$1" "$2" "$n"
  skipped+=("$3 (needs $2: sudo pacman -S $2)")
  return 1
}

step "Dependencies and tools" scripts/check-deps.sh --quiet
step "Formatting (cargo fmt --check)" cargo fmt --all --check
step "Clippy with warnings denied" cargo cl -- -D warnings
step "Tests (library, binary and policy tests)" cargo t --all-targets
step "Cargo.lock is up to date" cargo metadata --locked --format-version 1 --no-deps >/dev/null
step "Changelog documents this version" scripts/release.sh --check

if needs desktop-file-validate desktop-file-utils "Desktop file validation"; then
  step "Desktop file validation" desktop-file-validate packaging/io.github.vommie.rustview.desktop
fi
if needs appstreamcli appstream "AppStream metainfo validation"; then
  tmp=$(mktemp)
  sed "s/@VERSION@/$version/; s/@DATE@/$(date +%F)/" packaging/io.github.vommie.rustview.metainfo.xml > "$tmp"
  step "AppStream metainfo validation" appstreamcli validate --no-net "$tmp"
  rm -f "$tmp"
fi
if needs cargo-audit cargo-audit "Vulnerability scan (cargo audit)"; then
  step "Vulnerability scan (cargo audit)" cargo audit
fi
if needs cargo-about cargo-about "ATTRIBUTION.md is up to date"; then
  step "ATTRIBUTION.md is up to date" scripts/attribution.py --check
fi
if needs cargo-machete cargo-machete "Unused dependencies (cargo machete)"; then
  step "Unused dependencies (cargo machete)" cargo machete
fi
if needs cargo-deny cargo-deny "Licenses, bans and sources (cargo deny)"; then
  step "Licenses, bans and sources (cargo deny)" cargo deny --all-features check
fi

if [ "$fast" = 0 ]; then
  step "Release build and package (build-package.sh)" ./build-package.sh
fi

echo
echo "== Summary (version $version)"
for s in "${skipped[@]}"; do printf '%sSKIPPED%s %s\n' "$y" "$n" "$s"; done
if [ "${#failed[@]}" -gt 0 ]; then
  for s in "${failed[@]}"; do printf '%sFAILED %s %s\n' "$r" "$n" "$s"; done
  exit 1
fi
if [ "${#skipped[@]}" -gt 0 ]; then
  echo "All executed steps passed, but some steps were skipped (see above)."
  exit 2
fi
printf '%sAll steps passed.%s\n' "$g" "$n"
