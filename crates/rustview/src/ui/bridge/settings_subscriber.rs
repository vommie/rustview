//! Reacts to changed settings: applies every group of settings to the live UI.

use super::UiBridge;
use crate::ui::MainWindow;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::formats_policy::{FormatPolicyApplier, FormatPolicyContext};
use crate::ui::media::BridgeMedia;
use crate::ui::multiview;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::BridgeTags;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use crate::ui::thumbnail_badges::ThumbnailBadgeBridge;
use crate::ui::type_ahead::{self};
use crate::viewer::ViewerTabManager;
use crate::viewer::multiview::prefs;
use crate::viewer::zoom::prefs as zoom_prefs;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::settings::memory_budget::{BYTES_PER_MB, MemoryPlan, SystemMemory};
use rustview_core::settings::{AppSettings, GridAspectRatio, SettingsDiff, SettingsManager};
use rustview_core::thumbnails::ThumbnailConfig;
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;
use std::sync::Arc;

impl UiBridge {
    pub(super) fn register_settings_subscriber(&self) {
        let w_weak = self.window.as_weak();
        let library = self.library.clone();
        let thumbnails = self.thumbnails.clone();
        let tab_manager = self.tab_manager.clone();
        let settings = self.settings.clone();
        let state = self.state.clone();

        self.settings.subscribe(move |old_settings: &AppSettings, new_settings: &AppSettings, diff: &SettingsDiff| {
            if !diff.has_changes {
                return;
            }
            let (old_formats, new_formats) = (old_settings.formats.format_mask(), new_settings.formats.format_mask());

            let library = library.clone();
            let thumbnails = thumbnails.clone();
            let tab_manager = tab_manager.clone();
            let settings = settings.clone();
            let state = state.clone();
            let diff_clone = diff.clone();
            let new_clone = new_settings.clone();

            let _ = w_weak.upgrade_in_event_loop(move |window| {
                if diff_clone.performance_cache_changed {
                    let plan = MemoryPlan::resolve(new_clone.performance.memory_budget_mb, SystemMemory::read());
                    thumbnails.cache().set_budget(plan.thumbnail_cache_bytes.saturating_as::<usize>());
                    tracing::info!(
                        automatic = plan.automatic,
                        budget_mb = plan.budget_bytes / BYTES_PER_MB,
                        thumbnail_cache_mb = plan.thumbnail_cache_bytes / BYTES_PER_MB,
                        "[Memory: Budget] RAM budget changed; the thumbnail cache was resized"
                    );
                    if !plan.warnings.is_empty() {
                        tracing::warn!(warnings = %plan.warning_text(), "[Memory: Budget] The configured RAM budget is questionable");
                    }
                }

                if diff_clone.performance_pipeline_changed {
                    let new_config = ThumbnailConfig::from_performance_settings(&new_clone.performance);
                    thumbnails.set_config(new_config);
                    thumbnails.cache().clear();

                    if state.read().current_folder_id.is_some() {
                        BridgeMedia::reload(&window, &library, &thumbnails, &settings, &state, true, false);
                    }
                }

                if diff_clone.formats_policy_changed {
                    let ctx = FormatPolicyContext {
                        window: &window,
                        library: &library,
                        thumbnails: &thumbnails,
                        tab_manager: &tab_manager,
                        settings: &settings,
                        state: &state,
                    };
                    FormatPolicyApplier::apply(&ctx, old_formats, new_formats);
                }

                if diff_clone.tooltip_settings_changed {
                    window.set_enable_tooltips(new_clone.appearance.enable_tooltips);
                    window.set_tooltip_delay_ms(new_clone.appearance.tooltip_delay_ms.saturating_as::<i32>());
                    multiview::refresh_tooltips(&new_clone.appearance);
                }

                if diff_clone.type_ahead_changed {
                    type_ahead::config::apply(&new_clone.appearance);
                }

                if diff_clone.hotkeys_changed {
                    crate::ui::hotkeys::apply(&new_clone.hotkeys);
                    crate::ui::hotkeys::main_window::show_labels(&window);
                }

                if diff_clone.hold_to_confirm_changed {
                    crate::ui::hold_confirm::install(&window, new_clone.general.hold_to_confirm_ms);
                }

                if diff_clone.image_edit_changed {
                    crate::viewer::edit::prefs::apply(&new_clone.image_edit);
                }
                if diff_clone.fullscreen_settings_changed || diff_clone.viewer_settings_changed {
                    prefs::apply(&new_clone.fullscreen);
                    zoom_prefs::apply(&new_clone.fullscreen);
                    crate::viewer::multiview::video_bar::prefs::apply(&new_clone.fullscreen.player);
                    multiview::refresh_settings(&settings);
                    crate::viewer::multiview::video_filter::apply_default_normalize(
                        &new_clone.fullscreen.player.normalize,
                        &tab_manager.all_instances(),
                    );
                }

                if diff_clone.appearance_grid_changed {
                    window.set_thumb_size(new_clone.appearance.default_thumbnail_size.saturating_as::<i32>());
                    let ratio = GridAspectRatio::from_str_name(&new_clone.appearance.thumbnail_aspect_ratio);
                    window.set_thumb_aspect_idx(ratio.to_index());
                    ThumbnailBadgeBridge::apply_config(&window, &new_clone.appearance);
                    window.set_grid_item_padding_h(new_clone.appearance.grid_item_padding_horizontal.saturating_as::<i32>());
                    window.set_grid_item_padding_v(new_clone.appearance.grid_item_padding_vertical.saturating_as::<i32>());
                    window.set_grid_item_border_width(new_clone.appearance.grid_item_border_width.saturating_as::<i32>());
                    window.set_grid_item_corner_radius(new_clone.appearance.grid_item_corner_radius.saturating_as::<i32>());

                    if window.get_active_view_mode() == 2 {
                        BridgeTabGrid::sync_tab_grid_ui(&window, &tab_manager, &settings, &state);
                    }
                }

                if diff_clone.viewer_settings_changed {
                    crate::viewer::multiview::player_prefs::apply(&new_clone.viewer.videos);
                    let before = crate::viewer::ab_repeat::prefs::apply(&new_clone.viewer.videos.ab_repeat);
                    if before != crate::viewer::ab_repeat::prefs::current() {
                        crate::viewer::mpv_engine::ab_repeat::apply_prefs();
                    }
                    let bg_mode_idx = match new_clone.viewer.background_mode.to_lowercase().as_str() {
                        "deep black" | "black" => 1,
                        "transparent checkerboard" | "checkerboard" => 2,
                        _ => 0,
                    };
                    window.set_viewer_background_mode(bg_mode_idx);
                    window.set_viewer_show_tab_rating_badge(new_clone.viewer.tabs.show_rating_badge);
                    window.set_tab_close_buttons(crate::ui::tabs::strip_layout::close_buttons(&new_clone.viewer.tabs.close_buttons));
                }

                if diff_clone.debug_settings_changed {
                    crate::system::log_settings::apply(&new_clone.debug);
                }

                if diff_clone.folder_tree_presentation_changed && !diff_clone.root_folder_changed {
                    window.set_show_hidden_folders(new_clone.folders.show_hidden_folders);
                    BridgeFolders::reload_tree(&window, &library, &settings, &state);
                }

                if diff_clone.root_folder_changed {
                    state.write().invalidate_folder_hierarchy();
                    BridgeFolders::sync_root(&window, &library, &thumbnails, &settings, &state);
                }

                if diff_clone.tags_settings_changed {
                    {
                        let mut st = state.write();
                        st.max_history_tags = new_clone.tags.max_history_tags;
                        st.is_tag_history_collapsed = new_clone.tags.is_history_collapsed;
                        st.tag_history_ids.clone_from(&new_clone.tags.history_tag_ids);
                        st.tag_history_height = new_clone.tags.history_height;
                    }
                    BridgeTagHistory::sync_ui(&window, &library, &state);
                }

                if diff_clone.tag_tree_appearance_changed {
                    tracing::info!(compact = new_clone.tags.compact_tree, "[TagTree] Mode switched");
                    window.set_tag_tree_compact(new_clone.tags.compact_tree);
                    BridgeTags::reload_tags(&window, &library, &state);
                }

                StatusBarBridge::update_metrics(&window, &state);
            });
        });
    }

    /// Whatever creates a tab (a click, a menu, an action button, a Multiview moved into a tab), the tab list is rendered
    /// right away: after a tab was created the UI thread checks whether the tab UI is behind and, if so, renders it. The paths
    /// that render on their own have already marked the tab UI as current, so the check is free for them.
    pub(super) fn install_tabs_changed_hook(
        window: &MainWindow,
        tab_manager: &Arc<ViewerTabManager>,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let win = window.as_weak();
        let tm_weak = Arc::downgrade(tab_manager);
        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        tab_manager.set_tabs_changed_hook(move || {
            let (tm_weak, lib, set, st) = (tm_weak.clone(), lib.clone(), set.clone(), st.clone());
            let _ = win.upgrade_in_event_loop(move |w| {
                let Some(tm) = tm_weak.upgrade() else {
                    return;
                };
                if !tm.is_tab_ui_stale() {
                    return;
                }
                tracing::info!("[UiBridge: TabsChanged] A created tab was not rendered yet; rendering the tab list");
                BridgeViewerTabs::sync_ui(&w, &tm, &lib, &set, &st);
                if w.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(&w, &tm, &set, &st);
                }
            });
        });
    }
}
