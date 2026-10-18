#!/bin/bash
# usage: profile_probe.sh NAME BINARY PLAN SECONDS [perf extra args]
# Runs the app with the probe, records the main process with perf while the scenarios run and writes runs/NAME.perf.txt
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
PERF=/usr/lib/linux-tools/6.8.0-147-generic/perf
N=$1; BIN=$2; PLAN=$3; SECS=$4; shift 4
rm -f $P/runs/$N.log
setsid nohup $P/run_probe.sh $N $BIN "$PLAN" ${SEED:-$P/home_seed} > /dev/null 2>&1 &
for i in $(seq 1 600); do
  grep -q "starting the scenarios" $P/runs/$N.log 2>/dev/null && break
  sleep 0.2
done
PID=$(pgrep -x "$(basename $BIN)" | head -1)
sleep 1.5
$PERF record -e cpu-clock -F 499 "$@" -p $PID -o $P/runs/$N.perf.data -- sleep $SECS > $P/runs/$N.perf.log 2>&1
$PERF report -i $P/runs/$N.perf.data --no-children --sort comm,dso,sym --stdio 2>/dev/null | head -80 > $P/runs/$N.perf.txt
echo done
