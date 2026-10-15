#!/bin/bash
# Starts a virtual X server (:83) with openbox and a compositing manager for the perf probe scripts.
# usage: PERF_DIR=<scratch dir> scripts/perf/start_x.sh   (needs Xvfb, openbox, xcompmgr)
: "${PERF_DIR:?set PERF_DIR to the scratch directory}"
mkdir -p "$PERF_DIR"
pkill -f "Xvfb :83" 2>/dev/null; sleep 0.5
setsid nohup Xvfb :83 -screen 0 1600x900x24 -ac +extension Composite +extension RENDER +extension SHAPE +extension XFIXES >"$PERF_DIR/xvfb.log" 2>&1 &
sleep 1.5
DISPLAY=:83 setsid nohup openbox >"$PERF_DIR/openbox.log" 2>&1 &
sleep 0.8
DISPLAY=:83 setsid nohup xcompmgr -c >/dev/null 2>&1 &
sleep 0.5
echo "X ready on :83"
