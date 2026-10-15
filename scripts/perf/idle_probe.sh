#!/bin/bash
# usage: idle_probe.sh NAME BINARY [SECONDS]  -> idle CPU and wake-ups of the app showing the seeded folder (no probe plan)
P=${PERF_DIR:?set PERF_DIR to the scratch directory (images, home_seed, runs/)}
N=$1; BIN=$2; SECS=${3:-20}
H=$P/runs/${N}_home; rm -rf $H; mkdir -p $H; cp -a --sparse=always $P/home_seed/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1
unset WAYLAND_DISPLAY RUSTVIEW_PERF
RUST_LOG=rustview=warn setsid nohup $BIN > $P/runs/$N.log 2>&1 &
sleep 10
PID=$(pgrep -x "$(basename $BIN)" | head -1)
read_stat() { awk '{print $14+$15}' /proc/$PID/stat; }
read_ctx() { awk '/voluntary_ctxt_switches/ {v=$2} /^nonvoluntary_ctxt_switches/ {n=$2} END {print v+n}' /proc/$PID/status; }
rss() { awk '/VmRSS/ {print $2/1024}' /proc/$PID/status; }
C0=$(read_stat); X0=$(read_ctx)
# threads: sum of the ctx switches of all threads
T0=$(cat /proc/$PID/task/*/status | awk '/voluntary_ctxt_switches/ {s+=$2} END {print s}')
sleep $SECS
C1=$(read_stat); T1=$(cat /proc/$PID/task/*/status | awk '/voluntary_ctxt_switches/ {s+=$2} END {print s}')
HZ=$(getconf CLK_TCK)
echo "$N idle_cpu_percent=$(echo "scale=2; ($C1-$C0)*100/$HZ/$SECS" | bc) ctx_switches_per_s=$(echo "scale=0; ($T1-$T0)/$SECS" | bc) rss_mb=$(rss)"
kill -TERM $PID; sleep 2
