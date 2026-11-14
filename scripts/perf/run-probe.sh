#!/usr/bin/env bash
# Runs RustView once with a plan of the performance probe (docs/perf/README.md) and stops it again.
#
# usage: run-probe.sh [--bin PATH] [--home DIR] [--out DIR] [--timeout SECONDS] PLAN [LABEL]
#   --bin PATH       the binary (default: target/debug/rustview of this checkout)
#   --home DIR       an isolated profile: data, settings and logs live in DIR/data, DIR/config and DIR/state
#                    (without it the run uses your own library and settings)
#   --out DIR        where the logs go (default: the folder of the plan)
#   --timeout S      the run is ended after S seconds whatever happens (default 900)
# Prints the path of the frame log; read it with scripts/perf/analyze.py.
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
bin="$repo/target/debug/rustview"
home=""; out=""; limit=900
while [ $# -gt 0 ]; do
  case "$1" in
    --bin) bin="$2"; shift 2 ;;
    --home) home="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --timeout) limit="$2"; shift 2 ;;
    -h|--help) sed -n '2,10p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    --*) echo "unknown option: $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
[ $# -ge 1 ] || { echo "the plan is missing (see --help)" >&2; exit 2; }
plan="$(realpath "$1")"
label="${2:-$(basename "${plan%.*}")}"
[ -x "$bin" ] || { echo "no binary at $bin (cargo build -p rustview)" >&2; exit 2; }
[ -n "$out" ] || out="$(dirname "$plan")"
mkdir -p "$out"
log="$out/$label.log"
rm -f "$log"

if [ -n "$home" ]; then
  export XDG_DATA_HOME="$home/data" XDG_CONFIG_HOME="$home/config" XDG_STATE_HOME="$home/state"
fi
status=0
RUSTVIEW_PERF_PLAN="$plan" RUSTVIEW_PERF_LOG="$log" RUST_LOG="${RUST_LOG:-rustview=info}" \
  timeout --signal=TERM --kill-after=20 "$limit" "$bin" >"$out/$label.app.log" 2>&1 || status=$?
if [ "$status" -ne 0 ]; then
  echo "the run ended with status $status (124 = time limit); see $out/$label.app.log" >&2
fi
[ -s "$log" ] || { echo "no frame log was written; see $out/$label.app.log" >&2; exit 1; }
echo "$log"
exit "$status"
