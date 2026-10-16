#!/bin/bash
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
BIN=$1; N=$2
H=$P/runs/${N}_home; rm -rf $H; mkdir -p $H; cp -a --sparse=always $P/home_seed/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1 RUST_LOG=rustview=info
unset WAYLAND_DISPLAY RUSTVIEW_PERF
setsid nohup $BIN > $P/runs/$N.log 2>&1 &
sleep 12
shot() { import -window root $P/runs/${N}_$1.png; }
xdotool mousemove 1250 300 mousedown 1
for x in 1150 1000 900 800 700; do xdotool mousemove $x $((300+(1250-x)/3)); sleep 0.1; done
sleep 0.4; shot a_rb_drag
xdotool mouseup 1; sleep 1; shot b_rb_done
xdotool key ctrl+a; sleep 1; shot c_select_all
xdotool mousemove 800 450; for i in $(seq 1 20); do xdotool click 5; sleep 0.03; done; sleep 1.5; shot d_all_scrolled
pkill -TERM -x $(basename $BIN)
