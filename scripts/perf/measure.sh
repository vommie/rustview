#!/usr/bin/env bash
# Measures the responsiveness of RustView on this machine with the plans of scripts/perf/plans and prints the tables
# (docs/perf/README.md explains the columns). Everything lives in one throwaway folder: a generated library of 126,000
# small pictures (about 1.5 GB, made once), an isolated profile (your own library and settings are never touched) and
# the logs. The first run takes a while: generating the pictures and indexing them with their thumbnails.
#
# usage: measure.sh [--dir DIR] [--bin PATH] [--runs N] [PLAN ...]
#   --dir DIR    the folder for library, profile and logs (default: $XDG_CACHE_HOME/rustview-perf, ~/.cache/rustview-perf)
#   --bin PATH   the binary (default: target/debug/rustview of this checkout; `cargo build -p rustview` first)
#   --runs N     runs per plan, the tables show the median (default 3)
#   PLAN         plans to run (default: grid table loads resize columns)
# Do not use the machine while it runs: the window must stay visible, and other work changes the numbers.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
dir="${XDG_CACHE_HOME:-$HOME/.cache}/rustview-perf"
bin="$repo/target/debug/rustview"
runs=3
while [ $# -gt 0 ]; do
  case "$1" in
    --dir) dir="$2"; shift 2 ;;
    --bin) bin="$2"; shift 2 ;;
    --runs) runs="$2"; shift 2 ;;
    -h|--help) sed -n '2,13p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    --*) echo "unknown option: $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
plans=("$@"); [ ${#plans[@]} -gt 0 ] || plans=(grid table loads resize columns)
[ -x "$bin" ] || { echo "no binary at $bin (cargo build -p rustview)" >&2; exit 2; }

media="$dir/media"; profile="$dir/profile"; out="$dir/runs"
mkdir -p "$out" "$profile/config/rustview"

if [ ! -f "$dir/library.generated" ]; then
  echo "== generating the library in $media (once, about 1.5 GB)"
  python3 "$here/gen-library.py" --out "$media"
  touch "$dir/library.generated"
fi

settings="$profile/config/rustview/settings.json"
if [ ! -f "$dir/settings.base.json" ]; then
  cat > "$settings" <<JSON
{
  "general": { "root_folder": "$media", "remember_last_folder": false },
  "window": { "is_maximized": true }
}
JSON
  echo "== indexing the library (once): scan and thumbnails of 126,000 pictures"
  "$here/run-probe.sh" --bin "$bin" --home "$profile" --out "$out" --timeout 14400 "$here/plans/index-stress.plan" index >/dev/null
  cp "$settings" "$dir/settings.base.json"
fi

for plan in "${plans[@]}"; do
  logs=()
  for run in $(seq 1 "$runs"); do
    # Every run starts with the same settings (a plan moves resizers and sorts).
    cp "$dir/settings.base.json" "$settings"
    logs+=("$("$here/run-probe.sh" --bin "$bin" --home "$profile" --out "$out" --timeout 300 "$here/plans/$plan.plan" "$plan-$run")")
  done
  python3 "$here/analyze.py" --median "${logs[@]}"
done
