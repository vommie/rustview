//! The callbacks of the tag panel.

use super::{BridgeTags, TagActionContext, TagTreeSync};
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::query::{MatchMode, ScopeMode};
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::{SaturatingAs, WrappingAs};
use slint::ComponentHandle;
use std::sync::Arc;
use std::time::Instant;

impl BridgeTags {
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        tab_manager: Arc<ViewerTabManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_rating_applied(move |rating| {
            Self::apply_rating(
                if rating < 0 {
                    None
                } else {
                    Some(rating.saturating_as::<u8>())
                },
                &TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                },
            );
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_tag_toggled(move |tag_id| {
            Self::toggle_tag(
                tag_id.wrapping_as::<u32>(),
                &TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                },
            );
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_tag_toggle_expand(move |tag_id| {
            Self::toggle_expand(tag_id.wrapping_as::<u32>(), &win, &lib, &set, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_tag_context_menu(move |tag_id, x, y| {
            Self::show_tag_context_menu(tag_id.wrapping_as::<u32>(), &win, &lib, &st, &tm, x, y);
        });

        let win = window.clone_strong();
        let tm = tab_manager.clone();
        let st = state.clone();
        window.on_tag_root_context_menu(move |x, y| {
            Self::show_root_context_menu(&win, &st, &tm, x, y);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_mode_toggled(move |is_assign| {
            st.write().is_assign_mode = is_assign;
            TagTreeSync::sync_in_place(&win, &lib, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_match_mode_changed(move |idx| {
            let t0 = Instant::now();
            let mode = if idx == 0 { MatchMode::All } else { MatchMode::Any };

            let should_execute = {
                let mut s = st.write();
                s.filter.match_mode = mode;
                s.is_query_active || s.selected_rating > 0 || !s.filter.tag_ids.is_empty()
            };

            let elapsed_us = t0.elapsed().as_micros();
            tracing::info!(
                ?mode,
                should_execute,
                elapsed_us = elapsed_us,
                "[TagRatePanel: MatchMode] Match mode toggled"
            );

            if should_execute {
                Self::execute_query_flow(&TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                });
            } else {
                TagTreeSync::sync_in_place(&win, &lib, &st);
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_scope_mode_changed(move |idx| {
            let t0 = Instant::now();
            let scope = ScopeMode::from_index(idx);

            let should_execute = {
                let mut s = st.write();
                s.filter.scope_mode = scope;
                if scope == ScopeMode::Global {
                    s.is_virtual_folder_mode = false;
                    s.virtual_folder_count = 0;
                    win.set_is_virtual_folder(false);
                    win.set_virtual_folder_count(0);
                }
                s.is_query_active || s.selected_rating > 0 || !s.filter.tag_ids.is_empty()
            };

            let elapsed_us = t0.elapsed().as_micros();
            tracing::info!(
                ?scope,
                should_execute,
                elapsed_us = elapsed_us,
                "[TagRatePanel: ScopeMode] Scope mode toggled"
            );

            if should_execute {
                Self::execute_query_flow(&TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                });
            } else {
                TagTreeSync::sync_in_place(&win, &lib, &st);
                BridgeMedia::update_virtual_folder_button_state(&win, &lib, &st);
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_filter_changed(move |query| {
            Self::filter_tags(&win, &lib, &st, &query);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let st = state.clone();
        window.on_tag_regex_toggled(move |enabled| {
            st.write().tag_filter_is_regex = enabled;
            Self::reload_tags(&win, &lib, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_tag_reset_clicked(move || {
            Self::reset_query(&TagActionContext {
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                tab_manager: &tm,
                settings: &set,
                state: &st,
            });
        });

        let lib_geom = library;
        let st_geom = state;
        let win_geom = window.clone_strong();
        window.on_tag_tree_geometry_changed(move |available_h| {
            let (height_changed, prev_h) = {
                let mut st = st_geom.write();
                let prev = st.tag_tree_available_height;
                let changed = (prev - available_h).abs() > 4.0;
                if changed {
                    st.tag_tree_available_height = available_h;
                }
                (changed, prev)
            };

            if height_changed {
                tracing::debug!(
                    prev_height = prev_h,
                    new_height = available_h,
                    "[TagTree: GeometryChanged] Tag tree container height updated, recalculating waterfall layout..."
                );
                Self::reload_tags(&win_geom, &lib_geom, &st_geom);
            }
        });
    }
}
