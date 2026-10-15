#!/bin/bash
# usage: run_probe.sh VARIANT BINARY PLAN [SEED_HOME]
# Copies the seeded home to a private one (the app rewrites settings on exit), runs the app under Xvfb with the probe plan and
# stores the log in runs/VARIANT.log.
P=${PERF_DIR:?set PERF_DIR to the scratch directory (images, home_seed, runs/)}
V=$1; BIN=$2; PLAN=$3; SEED=${4:-$P/home_seed}
mkdir -p $P/runs
H=$P/runs/${V}_home
rm -rf $H; mkdir -p $H
cp -a --sparse=always $SEED/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share
export DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1
unset WAYLAND_DISPLAY
export RUSTVIEW_PERF="$PLAN" RUST_LOG=${RUST_LOG:-rustview=info}
timeout ${PROBE_TIMEOUT:-900} $BIN > $P/runs/$V.log 2>&1
echo "exit=$?" >> $P/runs/$V.log
