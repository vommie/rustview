#!/bin/bash
# usage: rss_trace.sh BINARY NAME SECONDS LIMIT_MB  -> starts the app on a copy of the 100k home, samples RSS every 0.5 s, kills it above LIMIT_MB
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
BIN=$1; N=$2; SECS=$3; LIMIT=$4
H=$P/runs/${N}_home; rm -rf $H; mkdir -p $H; cp -a --sparse=always $P/home_seed_huge/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1 RUST_LOG=rustview=info
unset WAYLAND_DISPLAY RUSTVIEW_PERF
setsid nohup $BIN > $P/runs/$N.log 2>&1 &
PID=$!
sleep 0.2
PID=$(pgrep -x "$(basename $BIN)" | head -1)
: > $P/runs/$N.rss.csv
END=$((SECONDS+SECS)); PEAK=0
while [ $SECONDS -lt $END ] && kill -0 $PID 2>/dev/null; do
  RSS=$(awk '/VmRSS/ {print int($2/1024)}' /proc/$PID/status 2>/dev/null)
  [ -z "$RSS" ] && break
  echo "$(date +%s.%N) $RSS" >> $P/runs/$N.rss.csv
  [ "$RSS" -gt "$PEAK" ] && PEAK=$RSS
  if [ "$RSS" -gt "$LIMIT" ]; then echo "$N: killed above ${LIMIT} MB"; break; fi
  sleep 0.5
done
echo "$N peak_rss_mb=$PEAK samples=$(wc -l < $P/runs/$N.rss.csv)"
pkill -KILL -x "$(basename $BIN)"
rm -rf $H
