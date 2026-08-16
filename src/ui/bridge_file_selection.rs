use super::MainWindow;
use super::bridge_inspector::BridgeInspector;
use super::bridge_state::UiState;
use super::bridge_tags::BridgeTags;
use super::file_selection::FileSelectionModel;
use crate::library::Library;
use crate::library::models::MediaId;
use crate::player::{Player, PlayerController};
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, Model};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Parameters representing a media click event with modifier keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileClickEvent {
    pub media_id: MediaId,
    pub is_ctrl: bool,
    pub is_shift: bool,
}

/// Subsystem managing media file selection calculations, fast in-place model mutations, and clicks.
pub struct BridgeFileSelection;

impl BridgeFileSelection {
    /// Registers media click, background click, and selection callbacks onto MainWindow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        player: Arc<PlayerController>,
        state: Arc<RwLock<UiState>>,
        last_click: Arc<Mutex<Option<(MediaId, Instant)>>>,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let pl = player;
        let st = state.clone();
        let lc = last_click;
        window.on_media_clicked(move |id, is_ctrl, is_shift| {
            Self::handle_click(
                FileClickEvent {
                    media_id: id as u64,
                    is_ctrl,
                    is_shift,
                },
                &win,
                &lib,
                &pl,
                &st,
                &lc,
            );
        });

        let win_bg = window.clone_strong();
        let lib_bg = library;
        let st_bg = state;
        window.on_background_clicked(move || {
            tracing::info!("[FileSelection: BackgroundClick] Background free space clicked, clearing active file selection");
            Self::clear_selection(&win_bg, &lib_bg, &st_bg);
        });
    }

    /// Handles media tile clicks with modifier keys, double-click timing, and surgical UI updates.
    pub fn handle_click(
        event: FileClickEvent,
        window: &MainWindow,
        library: &Arc<Library>,
        player: &Arc<PlayerController>,
        state: &Arc<RwLock<UiState>>,
        last_click: &Arc<Mutex<Option<(MediaId, Instant)>>>,
    ) {
        let t_start = Instant::now();

        // 1. Double Click Detection (< 400ms delta on same ID without modifiers)
        if Self::check_double_click(event, last_click) {
            Self::handle_double_click(event.media_id, state, library, player, window);
            return;
        }

        // 2. Selection Mutation Math (Lazy visible_ids computation)
        let (changed_ids, sel_count) = Self::mutate_selection(event, state);
        let t_math = t_start.elapsed();

        // 3. Fast In-Place Slint Model Mutation
        let t_sync_start = Instant::now();
        Self::sync_selection_diff(window, state, &changed_ids);
        let t_sync = t_sync_start.elapsed();

        // 4. Update Inspector Fields & Synchronize TagTree Highlight/Counts In-Place
        let t_insp_start = Instant::now();
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        let t_insp = t_insp_start.elapsed();

        tracing::debug!(
            target_media_id = event.media_id,
            is_ctrl = event.is_ctrl,
            is_shift = event.is_shift,
            selected_count = sel_count,
            changed_rows = changed_ids.len(),
            math_us = t_math.as_micros(),
            model_sync_us = t_sync.as_micros(),
            inspector_and_tags_us = t_insp.as_micros(),
            total_click_us = t_start.elapsed().as_micros(),
            "[FileSelection: FastInPlace] Instant selection updated with tag synchronization"
        );
    }

    fn check_double_click(
        event: FileClickEvent,
        last_click: &Arc<Mutex<Option<(MediaId, Instant)>>>,
    ) -> bool {
        let now = Instant::now();
        let mut click_guard = last_click.lock();
        if let Some((last_id, last_time)) = *click_guard
            && last_id == event.media_id
            && now.duration_since(last_time) < Duration::from_millis(400)
            && !event.is_ctrl
            && !event.is_shift
        {
            *click_guard = None;
            return true;
        }
        *click_guard = Some((event.media_id, now));
        false
    }

    fn mutate_selection(
        event: FileClickEvent,
        state: &Arc<RwLock<UiState>>,
    ) -> (Vec<MediaId>, usize) {
        let mut st = state.write();
        let prev_selected = st.file_selection.selected_ids().clone();

        if event.is_shift {
            let visible_ids: Vec<MediaId> = if !st.active_indices.is_empty() {
                st.active_indices
                    .iter()
                    .filter_map(|&idx| st.all_folder_media.get(idx))
                    .map(|item| item.record.id)
                    .collect()
            } else {
                st.all_folder_media
                    .iter()
                    .map(|item| item.record.id)
                    .collect()
            };

            let additive = event.is_ctrl;
            st.file_selection
                .select_range(event.media_id, &visible_ids, additive);
        } else if event.is_ctrl {
            st.file_selection.toggle(event.media_id);
        } else {
            st.file_selection.select_single(event.media_id);
        }

        let newly_selected = st.file_selection.selected_ids();
        let changed = FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected);
        let count = st.file_selection.count();
        (changed, count)
    }

    /// Surgically modifies only changed rows in the Slint Model with strict ID verification.
    pub fn sync_selection_diff(
        window: &MainWindow,
        state: &RwLock<UiState>,
        changed_ids: &[MediaId],
    ) {
        if changed_ids.is_empty() {
            return;
        }

        let media_model = window.get_media_items();
        let st = state.read();

        for &id in changed_ids {
            if let Some(&row_idx) = st.media_row_map.get(&id)
                && let Some(mut item) = media_model.row_data(row_idx)
                && item.id == id as i32
            {
                let is_selected = st.file_selection.is_selected(id);
                if item.is_selected != is_selected {
                    item.is_selected = is_selected;
                    media_model.set_row_data(row_idx, item);
                }
            }
        }
    }

    pub fn select_all(window: &MainWindow, library: &Arc<Library>, state: &Arc<RwLock<UiState>>) {
        let t_start = Instant::now();
        let changed_ids = {
            let mut st = state.write();
            let prev_selected = st.file_selection.selected_ids().clone();
            let visible_ids: Vec<MediaId> = if !st.active_indices.is_empty() {
                st.active_indices
                    .iter()
                    .filter_map(|&idx| st.all_folder_media.get(idx))
                    .map(|m| m.record.id)
                    .collect()
            } else {
                st.all_folder_media.iter().map(|m| m.record.id).collect()
            };
            st.file_selection.select_all(&visible_ids);
            let newly_selected = st.file_selection.selected_ids();
            FileSelectionModel::compute_selection_diff(&prev_selected, newly_selected)
        };

        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);

        tracing::debug!(
            changed_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAll] Fast in-place select all executed"
        );
    }

    pub fn clear_selection(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();
        let prev_selected = {
            let mut st = state.write();
            let prev = st.file_selection.selected_ids().clone();
            st.file_selection.clear();
            prev
        };

        let changed_ids: Vec<MediaId> = prev_selected.into_iter().collect();
        Self::sync_selection_diff(window, state, &changed_ids);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);

        tracing::debug!(
            cleared_rows = changed_ids.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: ClearSelection] Fast in-place clear selection executed"
        );
    }

    pub fn handle_double_click(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        player: &Arc<PlayerController>,
        window: &MainWindow,
    ) {
        if let Ok(Some(media)) = library.media().get(media_id) {
            let full_path = state
                .read()
                .current_folder_path
                .as_ref()
                .map(|p| p.join(&media.filename))
                .unwrap_or_default();

            tracing::info!(
                media_id = media_id,
                filename = %media.filename,
                media_type = ?media.media_type,
                path = ?full_path,
                "[MediaAction: DoubleClick] Double-click event triggered on media item"
            );

            Self::open_media(media_id, state, library, player, window);
        }
    }

    pub fn open_media(
        media_id: MediaId,
        state: &Arc<RwLock<UiState>>,
        library: &Arc<Library>,
        player: &Arc<PlayerController>,
        window: &MainWindow,
    ) {
        if let Ok(Some(media)) = library.media().get(media_id) {
            let full_path = state
                .read()
                .current_folder_path
                .as_ref()
                .map(|p| p.join(&media.filename))
                .unwrap_or_default();

            tracing::info!(media_id = media_id, file = ?full_path, "Opening media item");

            if matches!(media.media_type, crate::media::MediaType::Video)
                && player.load(&full_path, true).is_ok()
            {
                window.set_player_title(media.filename.into());
                window.set_is_player_open(true);
                window.set_player_is_playing(true);
            }
        }
    }
}
