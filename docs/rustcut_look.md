# RustView looks like RustCut (0.6.14)

Questionnaire `2026-11-06-rustview-rustcut-look` (released 2026-11-06). RustView took RustCut's look and, where RustCut's code
fits, its code (same author; the ported files say so in their header).

| Area | Decision | Where |
|---|---|---|
| Sidebars | Full dock like RustCut: panels Folders, Favorites, Info, Tag & Rate, Tag History are tabs in groups; drag a tab into another group, a new group or the other side; fold, maximize, collapse to a rail with peek and hold | `src/ui/dock/` (model, plan, view, persist, bridge, tests), `ui/dock_*.slint`; the panels are single instances placed over their group (`DockPanelSlot`); stored in `view.dock` |
| Info | The tab is called "Info" | `src/ui/dock/model.rs` |
| Top bar | Browser tabs like RustCut; Browse and Tab-Grid are fixed tabs of their own group, a line after them | `ui/toolbar.slint`, `ui/tab_bar.slint` |
| Overflow | Equal-width tabs (110 to 200 px); what does not fit waits behind "+n" with a scrolling bubble list; the wheel, drag, Ctrl/Shift selection and middle click work as before | `src/ui/tabs/strip_layout.rs` |
| Close buttons | On every tab by default, configurable (`viewer.tabs.close_buttons`) | Settings > Viewer > Tabs |
| Tooltips | RustCut's bubbles with a tail (placement in Rust, tested) | `src/ui/tooltip/`, `ui/tooltips.slint`, `ui/global_tooltip_layer.slint` |
| Settings | Changes apply at once, bottom bar "Close"; page title with a held reset; switches instead of check boxes; sliders stay in RustCut's style; one control column | `ui/settings_controls.slint`, `src/ui/settings_dialog/apply.rs`, `rustview_core::settings::page_reset` |

Deviations with a reason: media tabs have equal widths (exact drag places, many tabs) where RustCut's dock tabs size to their
title; the rail has a separator line (RustView's view next to it has the rail's color, RustCut's video is darker); the bar
above the window is darker (`Theme.chrome`) so the active tab merges into the window.

## Light theme (0.6.15, RustCut 0.0.109)

Questionnaire `2026-11-06-rustview-light-theme`: all as proposed. Catppuccin Latte as the light palette with the same token
names and values in both apps (`ui/styles.slint`, `Theme.dark`); Settings > Appearance > Theme: System (default, KDE's
color scheme through the desktop portal, followed live; dark without an answer), Dark, Light; the bars, menus and sliders
over pictures and videos stay dark (`MediaTheme`); the Browse grid follows the theme; the accent is the theme's blue.
Code: `rustview_core::settings::theme`, `src/ui/theme.rs` (every window through `dialog_parent`), `src/system/color_scheme.rs`.
