# Cleanup findings (for the branch after the acceptance)

User decision 2026-10-26 (M0 questionnaire, question 4): a general cleanup and optimization pass of RustView comes as a
branch of its own **after** the acceptance of `video-in-slint`. Every milestone logs here what it notices outside the video
work (one line each: where, what, why it matters, a rough size). Nothing here is done on this branch. At the end the list is
presented to the user as a questionnaire, and the user decides what is done.

| # | Found in | Where | What | Why it matters |
|---|---|---|---|---|
| 1 | M0 | `src/viewer/mpv_engine/events.rs` `spawn_event_observer_thread` | `time-pos`, `duration`, `pause`, `volume`, `eof-reached`, `idle-active`, `fullscreen`, `window-maximized` are read with `get_property` every 20 ms (100 ms hidden) per player instead of `observe_property` events | CPU and wake-ups per video while nothing changes |
| 2 | M0 | `src/ui/multiview/session.rs`, `tab_host.rs` (`MULTIVIEW_TICK_MS` = 50) | a 50 ms tick runs while a Multiview is shown (aspect probing, zoom polling, mark reveal); after M3 the pointer part is gone, the rest could be event-driven | idle CPU, the "no polling" rule of CLAUDE.md |
| 3 | M0 (lab) | Slint 1.18 on X11 | renders twice per isolated redraw request (`TimerBasedFrameThrottle`); affects every window | worth an upstream report; double GPU work per video frame once videos are drawn by Slint |
| 4 | M0 | `src/viewer/mpv_engine.rs` | stale docs ("native OSC", "unreserved mouse wheel pass-through") and the `MpvPlayer` struct with ten callback lists | readability |
| 5 | M0 | `src/ui/multiview/session.rs` (697 lines), `src/ui/multiview/video_bar/window.rs` (694) | over the ~600-line limit of CLAUDE.md (the second goes in M2 anyway) | file size rule |
