#!/bin/bash
P=${PERF_DIR:?set PERF_DIR to the scratch directory}
BIN=$1; N=$2
H=$P/runs/${N}_home; rm -rf $H; mkdir -p $H; cp -a --sparse=always $P/home_seed_huge/. $H/
export HOME=$H XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share DISPLAY=:83 XDG_SESSION_TYPE=x11 LIBGL_ALWAYS_SOFTWARE=1 SLINT_BACKEND=winit NO_COLOR=1 RUST_LOG=rustview=info
unset WAYLAND_DISPLAY RUSTVIEW_PERF
setsid nohup $BIN > $P/runs/$N.log 2>&1 &
shot() { import -window root $P/runs/${N}_$1.png; }
sleep 3.2; shot a_loading
sleep 6; shot b_loaded
xdotool mousemove 800 450; for i in $(seq 1 30); do xdotool click 5; sleep 0.03; done; sleep 2; shot c_scrolled
xdotool mousemove 1150 70 click 1; sleep 0.3; xdotool type --delay 80 "0007"; sleep 2.5; shot d_query
xdotool key ctrl+a BackSpace; sleep 3; shot e_query_cleared
xdotool mousemove 1392 145 click 1; sleep 4; shot f_rating_query
xdotool mousemove 1348 145 click 1; sleep 5; shot g_rating_reset
xdotool mousemove 110 191 click 1; sleep 0.4; shot h_switching
sleep 3; shot i_small
xdotool mousemove 110 167 click 1; sleep 6; shot j_huge_again
pkill -TERM -x $(basename $BIN)
