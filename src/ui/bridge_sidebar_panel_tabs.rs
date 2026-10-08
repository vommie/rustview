use super::sidebar_panel_tab_dnd::{SidebarPanelTabDndGeometry, SidebarPanelTabDndSession};
use super::{MainWindow, SidebarPanelTabModel};
use crate::settings::SettingsManager;
use crate::ui::bridge_state::{LastActivePanel, UiState};
use parking_lot::RwLock;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::sync::Arc;

/// Central coordinator bridge managing sidebar dock panel tabs (Folders vs Favorites)
/// with real-time drag-and-drop reordering, smooth displacement animation, and settings persistence.
pub struct BridgeSidebarPanelTabs;

impl BridgeSidebarPanelTabs {
    /// Initializes sidebar panel tabs from persisted settings and synchronizes with the UI.
    pub fn init(
        window: &MainWindow,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let app_settings = settings.get();
        let active_tab = app_settings.view.left_sidebar_active_panel_tab.clone();
        {
            let mut st = state.write();
            st.active_sidebar_panel_tab = active_tab;
        }
        Self::sync_ui(window, settings, state);
    }

    /// Synchronizes current panel tabs ordering and active state to Slint's `sidebar-panel-tabs` model.
    pub fn sync_ui(
        window: &MainWindow,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let app_settings = settings.get();
        let tab_ids = &app_settings.view.left_sidebar_panel_tabs;
        let active_tab = &state.read().active_sidebar_panel_tab;

        let models: Vec<SidebarPanelTabModel> = tab_ids
            .iter()
            .map(|id| {
                let (title, icon_name) = match id.as_str() {
                    "folders" => ("Folders".to_string(), "folder".to_string()),
                    "favorites" => ("Favorites".to_string(), "favorite".to_string()),
                    other => (
                        format!("{}{}", other[..1].to_uppercase(), &other[1..]),
                        "folder".to_string(),
                    ),
                };
                let is_active = id == active_tab;
                SidebarPanelTabModel {
                    id: id.clone().into(),
                    title: title.into(),
                    icon_name: icon_name.into(),
                    is_active,
                    shift_x: 0.0,
                    is_placeholder: false,
                }
            })
            .collect();

        window.set_sidebar_panel_tabs(ModelRc::new(VecModel::from(models)));
        window.set_active_sidebar_panel_tab(active_tab.clone().into());
    }

    /// Registers selection and drag-and-drop callbacks on `MainWindow`.
    pub fn register_callbacks(
        window: &MainWindow,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        // 1. Tab Selection
        let win_sel = window.clone_strong();
        let set_sel = settings.clone();
        let st_sel = state.clone();
        window.on_sidebar_panel_tab_selected(move |tab_id| {
            let id_str = tab_id.as_str().to_string();
            tracing::info!(
                tab_id = %id_str,
                "[SidebarPanelTabs: Select] User switched active sidebar panel tab"
            );
            {
                let mut st = st_sel.write();
                st.active_sidebar_panel_tab = id_str.clone();
                if id_str == "favorites" {
                    st.last_active_panel = LastActivePanel::Favorite;
                } else if id_str == "folders" {
                    st.last_active_panel = LastActivePanel::Folder;
                }
            }
            {
                let mut app_settings = set_sel.get();
                app_settings.view.left_sidebar_active_panel_tab = id_str;
                set_sel.save_quiet_async(&app_settings);
            }
            Self::sync_ui(&win_sel, &set_sel, &st_sel);
        });

        // 2. Drag Started
        let win_dstart = window.clone_strong();
        let set_dstart = settings.clone();
        let st_dstart = state.clone();
        window.on_sidebar_panel_tab_drag_started(move |tab_id, source_idx, mouse_x, tab_width| {
            let id_str = tab_id.as_str().to_string();
            let app_settings = set_dstart.get();
            let tab_ids = app_settings.view.left_sidebar_panel_tabs.clone();

            let widths: Vec<f32> = tab_ids
                .iter()
                .enumerate()
                .map(|(i, id)| {
                    if i == source_idx as usize && tab_width > 20.0 {
                        tab_width.clamp(60.0, 110.0)
                    } else {
                        let title = match id.as_str() {
                            "folders" => "Folders",
                            "favorites" => "Favorites",
                            other => other,
                        };
                        SidebarPanelTabDndGeometry::estimate_tab_width(title)
                    }
                })
                .collect();

            let slots = SidebarPanelTabDndGeometry::compute_tab_slots(&tab_ids, &widths, 4.0, 0.0);

            let grab_offset = if let Some(s) = slots.get(source_idx as usize) {
                (mouse_x - s.start_x).clamp(0.0, s.width)
            } else {
                tab_width / 2.0
            };

            let session = SidebarPanelTabDndSession::new(
                id_str.clone(),
                source_idx as usize,
                grab_offset,
                mouse_x,
                slots,
            );

            let ghost_x = session.ghost_left_x();
            let ghost_w = session.current_slot_width();
            let (ghost_title, ghost_icon) = match id_str.as_str() {
                "folders" => ("Folders".to_string(), "folder".to_string()),
                "favorites" => ("Favorites".to_string(), "favorite".to_string()),
                other => (other.to_string(), "folder".to_string()),
            };

            win_dstart.set_is_dragging_panel_tab(true);
            win_dstart.set_panel_tab_drag_ghost_x(ghost_x);
            win_dstart.set_panel_tab_drag_ghost_width(ghost_w);
            win_dstart.set_panel_tab_drag_ghost_title(ghost_title.into());
            win_dstart.set_panel_tab_drag_ghost_icon(ghost_icon.into());

            let tab_models = win_dstart.get_sidebar_panel_tabs();
            for i in 0..tab_models.row_count() {
                if let Some(mut m) = tab_models.row_data(i) {
                    let is_ph = m.id.as_str() == id_str.as_str();
                    if m.is_placeholder != is_ph {
                        m.is_placeholder = is_ph;
                        tab_models.set_row_data(i, m);
                    }
                }
            }

            st_dstart.write().active_sidebar_panel_tab_dnd_session = Some(session);

            tracing::info!(
                tab_id = %id_str,
                source_idx,
                mouse_x,
                "[SidebarPanelTabs: DragStart] Sidebar panel tab drag gesture initiated"
            );
        });

        // 3. Drag Moved
        let win_dmove = window.clone_strong();
        let st_dmove = state.clone();
        window.on_sidebar_panel_tab_drag_moved(move |mouse_x, _view_width| {
            let (drag_tab_id, source_idx, cur_target_idx, ghost_x, shifts) = {
                let mut st = st_dmove.write();
                let Some(ref mut session) = st.active_sidebar_panel_tab_dnd_session else {
                    return;
                };

                session.current_cursor_x = mouse_x;
                let center_x = session.dragged_center_x();
                let new_target_idx = SidebarPanelTabDndGeometry::calculate_target_index(
                    &session.slots,
                    session.source_index,
                    center_x,
                    session.target_index,
                );

                session.target_index = new_target_idx;
                let shifts = SidebarPanelTabDndGeometry::compute_shift_offsets(
                    &session.slots,
                    session.source_index,
                    new_target_idx,
                    4.0,
                );

                (
                    session.dragged_tab_id.clone(),
                    session.source_index,
                    new_target_idx,
                    session.ghost_left_x(),
                    shifts,
                )
            };

            win_dmove.set_panel_tab_drag_ghost_x(ghost_x);

            let tab_models = win_dmove.get_sidebar_panel_tabs();
            for i in 0..tab_models.row_count() {
                if let Some(mut item) = tab_models.row_data(i) {
                    let target_shift = shifts.get(i).copied().unwrap_or(0.0);
                    let is_ph = item.id.as_str() == drag_tab_id.as_str();

                    if (item.shift_x - target_shift).abs() > 0.1 || item.is_placeholder != is_ph {
                        item.shift_x = target_shift;
                        item.is_placeholder = is_ph;
                        tab_models.set_row_data(i, item);
                    }
                }
            }

            tracing::trace!(
                tab_id = %drag_tab_id,
                source_idx,
                target_idx = cur_target_idx,
                ghost_x,
                "[SidebarPanelTabs: DragMove] Neighbor panel tab shift offsets updated"
            );
        });

        // 4. Drag Ended
        let win_dend = window.clone_strong();
        let set_dend = settings.clone();
        let st_dend = state.clone();
        window.on_sidebar_panel_tab_drag_ended(move |tab_id, _target_idx| {
            let session_opt = st_dend.write().active_sidebar_panel_tab_dnd_session.take();
            win_dend.set_is_dragging_panel_tab(false);

            if let Some(session) = session_opt {
                let source_idx = session.source_index;
                let target_idx = session.target_index;
                let id_str = tab_id.as_str().to_string();

                if source_idx != target_idx {
                    let mut app_settings = set_dend.get();
                    let tabs_vec = &mut app_settings.view.left_sidebar_panel_tabs;
                    if source_idx < tabs_vec.len() {
                        let item = tabs_vec.remove(source_idx);
                        let target_clamped = target_idx.min(tabs_vec.len());
                        tabs_vec.insert(target_clamped, item);
                        set_dend.save_quiet_async(&app_settings);
                    }
                    tracing::info!(
                        tab_id = %id_str,
                        source_idx,
                        target_idx,
                        "[SidebarPanelTabs: DragEnd] Panel tab reordered and persisted to settings"
                    );
                }
            }

            Self::sync_ui(&win_dend, &set_dend, &st_dend);
        });

        // 5. Drag Cancelled
        let win_dcancel = window.clone_strong();
        let set_dcancel = settings;
        let st_dcancel = state;
        window.on_sidebar_panel_tab_drag_cancelled(move |_tab_id| {
            st_dcancel.write().active_sidebar_panel_tab_dnd_session = None;
            win_dcancel.set_is_dragging_panel_tab(false);
            Self::sync_ui(&win_dcancel, &set_dcancel, &st_dcancel);
        });
    }
}
