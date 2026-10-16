#!/bin/bash
# usage: shots_views.sh BINARY NAME -> view toggle (grid <-> table), folder switch, query, rubber band, keyboard navigation screenshots
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
BIN=$1; N=$2
H=$P/runs/${N}_home; rm -rf $H; mkdir -p $H; cp -a --sparse=always $P/home_seed/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1 RUST_LOG=rustview=info
unset WAYLAND_DISPLAY RUSTVIEW_PERF
setsid nohup $BIN > $P/runs/$N.log 2>&1 &
sleep 12
shot() { import -window root $P/runs/${N}_$1.png; }
xdotool mousemove 800 450
for i in $(seq 1 25); do xdotool click 5; sleep 0.03; done; sleep 1.5
shot a_grid_scrolled
xdotool mousemove 1299 70 click 1; sleep 2; shot b_table
xdotool mousemove 800 450
for i in $(seq 1 25); do xdotool click 5; sleep 0.03; done; sleep 1.5
shot c_table_scrolled
xdotool mousemove 1299 70 click 1; sleep 2; shot d_back_to_grid
xdotool mousemove 110 215 click 1; sleep 2.5; shot e_small_100
xdotool mousemove 110 167 click 1; sleep 3; shot f_large_5000
xdotool mousemove 110 191 click 1; sleep 2.5
xdotool mousemove 1150 70 click 1; sleep 0.3; xdotool type --delay 80 "0007"; sleep 2; shot g_query
xdotool key ctrl+a BackSpace; sleep 2; shot h_query_cleared
xdotool mousemove 300 200 mousedown 1 mousemove 600 400 mousemove 900 500 sleep 0.5; shot i_rubberband; xdotool mouseup 1; sleep 0.8; shot j_rubberband_done
xdotool key Right Right Down; sleep 1; shot k_keys
pkill -TERM -x $(basename $BIN)
