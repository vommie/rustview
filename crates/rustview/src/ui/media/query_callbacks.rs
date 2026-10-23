//! The callbacks of the filter, sort and search controls of the Browse view.

use super::{ACTIVE_HOVER_MEDIA_ID, ACTIVE_PREVIEW_TIMER, BridgeMedia, MediaReloadOptions};
use crate::ui::MainWindow;
use crate::ui::browse_window::update_tile;
use crate::ui::media::video_preview::VideoPreviewAnimator;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::ui::thumbnail_regeneration::ThumbnailRegeneration;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::media::MediaFormat;
use rustview_core::query::{MediaTypeFilter, SortColumn, SortOrder};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailManager};
use rustview_core::util::cast::{SaturatingAs, WrappingAs};
use slint::{ComponentHandle, Image};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

impl BridgeMedia {
    pub fn register_query_callbacks(
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
        let set = settings.clone();
        let st = state.clone();
        window.on_search_query_changed(move |query| {
            tracing::info!(
                query = %LogObscurer::query(&query),
                "[SearchFilter: QueryChanged] User modified search filter query"
            );
            st.write().last_active_panel = LastActivePanel::Media;
            st.write().filter.search_query = query.to_string();
            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_regex_mode_toggled(move |enabled| {
            tracing::info!(
                enabled,
                "[SearchFilter: RegexToggle] Regular expression filter mode toggled"
            );
            st.write().last_active_panel = LastActivePanel::Media;
            st.write().filter.is_regex = enabled;
            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        ThumbnailRegeneration::register_callbacks(window, thumbnails.clone(), settings.clone(), state.clone());

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_virtual_folder_toggled(move |_active| {
            st.write().last_active_panel = LastActivePanel::Media;
            Self::toggle_virtual_folder_mode(&win, &lib, &thumb, &tm, &set, &st);
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let tm = tab_manager.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_media_type_filter_changed(move |idx| {
            let new_filter = MediaTypeFilter::from_index(idx);
            tracing::info!(
                media_type = ?new_filter,
                "[MediaTypeFilter: Changed] Media type view filter updated"
            );

            {
                let mut s = st.write();
                s.last_active_panel = LastActivePanel::Media;
                s.filter.media_type_filter = new_filter;
            }

            {
                let mut app_settings = set.get();
                app_settings.view.media_type_filter = new_filter.as_str().to_string();
                set.save_quiet_async(&app_settings);
            }

            if st.read().is_query_active {
                BridgeTags::execute_query_flow(&TagActionContext {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                });
            } else {
                Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
            }
        });

        let win = window.clone_strong();
        let lib = library.clone();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();
        window.on_sort_column_clicked(move |col_idx| {
            let target_col = SortColumn::from_index(col_idx);
            let (new_col, new_ord) = {
                let mut s = st.write();
                s.last_active_panel = LastActivePanel::Media;
                if s.sort_column == target_col {
                    s.sort_order = s.sort_order.toggle();
                } else {
                    s.sort_column = target_col;
                    s.sort_order = SortOrder::Ascending;
                }
                (s.sort_column, s.sort_order)
            };

            {
                let mut app_settings = set.get();
                app_settings.view.sort_column = new_col.as_str().to_string();
                app_settings.view.sort_order = new_ord.as_str().to_string();
                set.save_quiet(&app_settings);
            }

            win.set_sort_column_idx(new_col.to_index());
            win.set_sort_order_idx(new_ord.to_index());

            Self::reload_deferred(&win, &lib, &thumb, &set, &st, MediaReloadOptions::new(false, false));
        });

        let win = window.clone_strong();
        let thumb = thumbnails.clone();
        let set = settings.clone();
        let st = state.clone();

        let win_hover = win.clone_strong();
        window.on_media_hover_changed(move |id, is_hover| {
            let media_id = id.wrapping_as::<u64>();
            if !set.get().appearance.enable_hover_previews {
                return;
            }

            if is_hover {
                ACTIVE_HOVER_MEDIA_ID.with(|active_id| *active_id.borrow_mut() = Some(media_id));
                ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                let (format, full_path_opt) = {
                    let st_r = st.read();
                    if let Some(item) = st_r.all_folder_media.find(media_id) {
                        (item.record.format, Some(item.full_path()))
                    } else {
                        (MediaFormat::Unknown, None)
                    }
                };

                let win_w = win_hover.as_weak();
                let st_c = st.clone();

                // The preview frames stored in the pack store (videos and animated pictures) never touch the file. Only a GIF
                // without stored frames (indexed before they existed) still decodes its file, as it always did, until a refresh
                // the user starts stores them.
                if let Some(images) = VideoPreviewAnimator::load_preview_images(&thumb, media_id) {
                    let timer = VideoPreviewAnimator::start_animation(images, move |img| {
                        if let Some(w) = win_w.upgrade() {
                            update_tile(&w, &st_c.read(), media_id, |row| row.thumbnail = img);
                        }
                    });
                    ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                } else if format == MediaFormat::Gif
                    && let Some(file_path) = full_path_opt
                {
                    if let Some(preview) = thumb.try_get_animated_preview(media_id) {
                        let timed_frames = VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                        let timer = VideoPreviewAnimator::start_variable_delay_animation(timed_frames, move |img| {
                            if let Some(w) = win_w.upgrade() {
                                update_tile(&w, &st_c.read(), media_id, |row| row.thumbnail = img);
                            }
                        });
                        ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                    } else {
                        let config = ThumbnailConfig::from_performance_settings(&set.get().performance);
                        let win_w_async = win_hover.as_weak();
                        let st_async = st.clone();

                        thumb.get_or_load_animated_preview_async(media_id, file_path, config, move |res| {
                            if let Ok(preview) = res {
                                let _ = win_w_async.upgrade_in_event_loop(move |w| {
                                    let is_still_hovered =
                                        ACTIVE_HOVER_MEDIA_ID.with(|id| *id.borrow() == Some(media_id));
                                    if is_still_hovered {
                                        let timed_frames =
                                            VideoPreviewAnimator::convert_animated_preview_to_timed_frames(&preview);
                                        let timer = VideoPreviewAnimator::start_variable_delay_animation(
                                            timed_frames,
                                            move |img| {
                                                update_tile(&w, &st_async.read(), media_id, |row| row.thumbnail = img);
                                            },
                                        );
                                        ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = timer);
                                    }
                                });
                            }
                        });
                    }
                }
            } else {
                ACTIVE_HOVER_MEDIA_ID.with(|active_id| *active_id.borrow_mut() = None);
                ACTIVE_PREVIEW_TIMER.with(|t| *t.borrow_mut() = None);

                let st_r = st.read();
                if let Some(buf) = st_r.thumbs.get(media_id) {
                    update_tile(&win_hover, &st_r, media_id, |row| row.thumbnail = Image::from_rgb8(buf));
                }
            }
        });

        let thumb_stream = thumbnails.clone();
        let st_stream = state.clone();
        let win_stream = window.clone_strong();
        window.on_viewport_changed(move |first_row, visible_rows, columns, total| {
            Self::handle_viewport_changed(
                first_row.max(0).saturating_as::<usize>(),
                visible_rows.max(1).saturating_as::<usize>(),
                columns.max(1).saturating_as::<usize>(),
                total.max(0).saturating_as::<usize>(),
                &win_stream,
                &st_stream,
                &thumb_stream,
            );
        });

        let last_resize_time = Arc::new(parking_lot::Mutex::new(Instant::now()));
        let resize_events_count = Arc::new(AtomicUsize::new(0));

        let last_time_clone = last_resize_time;
        let count_clone = resize_events_count;

        window.on_column_resized(move |_col_idx, _new_width| {
            let _count = count_clone.fetch_add(1, Ordering::Relaxed);
            let mut last_log = last_time_clone.lock();
            if last_log.elapsed().as_millis() >= 100 {
                *last_log = Instant::now();
                count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_res = settings.clone();
        window.on_column_resize_finished(move |col_idx, final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_res.get();

            match col_idx {
                0 => app_settings.view.col_filename_width = width_f32,
                1 => app_settings.view.col_path_width = width_f32,
                2 => app_settings.view.col_modified_date_width = width_f32,
                3 => app_settings.view.col_created_date_width = width_f32,
                4 => app_settings.view.col_size_width = width_f32,
                5 => app_settings.view.col_rating_width = width_f32,
                6 => app_settings.view.col_tags_width = width_f32,
                7 => app_settings.view.col_width_width = width_f32,
                8 => app_settings.view.col_height_width = width_f32,
                9 => app_settings.view.col_ar_width = width_f32,
                10 => app_settings.view.col_duration_width = width_f32,
                11 => app_settings.view.col_format_width = width_f32,
                12 => app_settings.view.col_flags_width = width_f32,
                _ => {}
            }

            set_res.save_quiet(&app_settings);
        });

        let set_mode = settings;
        let win_mode = window.clone_strong();
        let thumb_mode = thumbnails;
        let st_mode = state;

        window.on_view_mode_toggled_with_index(move |is_table, top_item_idx| {
            st_mode.write().last_active_panel = LastActivePanel::Media;
            Self::handle_view_mode_toggled(
                is_table,
                top_item_idx.saturating_as::<usize>(),
                &win_mode,
                &thumb_mode,
                &set_mode,
                &st_mode,
            );
        });
    }
}
