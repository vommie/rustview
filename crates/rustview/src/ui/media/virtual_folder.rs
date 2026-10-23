//! The virtual (recursive) folder view: its toolbar button and its toggle.

use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::ui::browse_folders::BrowseFolders;
use crate::ui::virtual_folder_wording;
use crate::ui::MainWindow;
use rustview_core::library::Library;
use rustview_core::query::ScopeMode;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use super::BridgeMedia;

impl BridgeMedia {
    pub fn update_virtual_folder_button_state(
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let st = state.read();

        let is_virtual = st.is_virtual_folder_mode;
        let (is_enabled, disabled_reason): (bool, Option<&str>) = if st.is_query_active && st.filter.scope_mode == ScopeMode::Global {
            (false, Some("Recursive folder view is disabled during global query"))
        } else if st.is_query_active {
            (false, Some("Recursive folder view is disabled while query filter is active"))
        } else if let Some(folder_id) = st.current_folder_id {
            let has_subfolders = library
                .folders()
                .get_descendant_folder_ids(folder_id)
                .map(|ids| ids.len() > 1)
                .unwrap_or(false);

            if has_subfolders {
                (true, None)
            } else {
                (false, Some("No subfolders available in current folder"))
            }
        } else {
            (false, Some("No folder selected"))
        };

        drop(st);

        window.set_is_virtual_folder_btn_enabled(is_enabled);
        window.set_virtual_folder_tooltip(virtual_folder_wording::tooltip(is_virtual, disabled_reason).into());
        // The New Folder button depends on the same view state (no query, no recursive view, a folder that exists).
        BrowseFolders::update_new_folder_button(window, state);
    }

    pub fn toggle_virtual_folder_mode(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if Self::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, "Cannot switch view mode: another operation is in progress.");
            return;
        }

        let t0 = Instant::now();
        thumbnails.cancel_pipeline();

        let (current_f_id, current_p_opt, new_mode) = {
            let mut st = state.write();

            if let Some(prev_flag) = st.active_folder_cancel_flag.take() {
                prev_flag.store(true, Ordering::Release);
            }
            let new_flag = Arc::new(AtomicBool::new(false));
            st.active_folder_cancel_flag = Some(new_flag);

            st.active_folder_token = st.active_folder_token.wrapping_add(1);
            st.is_virtual_folder_mode = !st.is_virtual_folder_mode;
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.file_selection.clear();
            st.invalidate_view_window();

            (st.current_folder_id, st.current_folder_path.clone(), st.is_virtual_folder_mode)
        };

        window.set_is_virtual_folder(new_mode);

        if let (Some(folder_id), Some(path)) = (current_f_id, current_p_opt) {
            tracing::info!(
                folder_id,
                path = %LogObscurer::path(&path),
                is_virtual = new_mode,
                "[BridgeMedia: VirtualFolder] Toggled virtual recursive folder mode"
            );
            if state.read().is_query_active {
                BridgeTags::execute_query_flow(&TagActionContext {
                    window,
                    library,
                    thumbnails,
                    tab_manager,
                    settings,
                    state,
                });
            } else {
                let target_folder_ids = if new_mode {
                    library.folders().get_descendant_folder_ids(folder_id).unwrap_or_else(|_| vec![folder_id])
                } else {
                    vec![folder_id]
                };

                let virtual_count = target_folder_ids.len();
                {
                    let mut st = state.write();
                    st.virtual_folder_count = virtual_count;
                }
                StatusBarBridge::update_virtual_folder(window, new_mode, virtual_count);

                let mut media_ids = Vec::new();
                if let Ok(rtxn) = library.raw_db().env.read_txn() {
                    for fid in &target_folder_ids {
                        if let Ok(Some(ids)) = library.raw_db().media_by_folder.get(&rtxn, fid) {
                            media_ids.extend(ids);
                        }
                    }
                }

                let mut known_media = Vec::with_capacity(media_ids.len());
                let mut seen_ids = HashSet::new();
                if let Ok(rtxn) = library.raw_db().env.read_txn() {
                    for id in media_ids {
                        if seen_ids.insert(id)
                            && let Ok(Some(rec)) = library.raw_db().media.get(&rtxn, &id)
                        {
                            known_media.push(rec);
                        }
                    }
                }

                let indexed_media = if new_mode {
                    BridgeMedia::preindex_records(library, known_media)
                } else {
                    BridgeMedia::preindex(library, known_media, &path)
                };

                {
                    let mut st = state.write();
                    st.all_folder_media.replace(indexed_media);
                    st.file_selection.clear();
                }

                window.invoke_scroll_media_to_top();
                Self::reload(window, library, thumbnails, settings, state, false, false);
                BridgeInspector::update(window, state);
                BridgeTags::sync_tag_selection(window, library, state);
                Self::update_virtual_folder_button_state(window, library, state);
                StatusBarBridge::update_metrics(window, state);

                let folder_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                if new_mode {
                    StatusBarBridge::info(
                        window,
                        format!(
                            "Recursive view enabled: Showing {} folder(s) under '{}' ({}ms)",
                            virtual_count, folder_name, t0.elapsed().as_millis()
                        ),
                    );
                } else {
                    StatusBarBridge::info(
                        window,
                        format!(
                            "Recursive view disabled: Showing '{}' only ({}ms)",
                            folder_name, t0.elapsed().as_millis()
                        ),
                    );
                }
            }
        }
    }
}
