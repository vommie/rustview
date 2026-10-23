//! Puts a change of Settings > File Formats into effect while the app runs: the folder counts follow, the tree and the
//! open list are built again without (or with) the records of the formats that changed. Reads only the library, never
//! the drive: files of a newly switched-on format that were never indexed appear with the next refresh.

use super::bridge_folders::BridgeFolders;
use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::bridge_status_bar::StatusBarBridge;
use super::bridge_tags::{BridgeTags, TagActionContext};
use super::MainWindow;
use rustview_core::library::{FormatCountReport, Library};
use rustview_core::media::{FormatMask, MediaFormat};
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;

/// Grouped dependencies of the applier.
pub struct FormatPolicyContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

pub struct FormatPolicyApplier;

impl FormatPolicyApplier {
    /// `old` and `new` are the policy before and after the Settings were saved (the process-wide policy already is `new`).
    pub fn apply(ctx: &FormatPolicyContext<'_>, old: FormatMask, new: FormatMask) {
        let FormatPolicyContext { window, library, thumbnails, tab_manager, settings, state } = *ctx;
        let changes = old.changes_to(new);
        tracing::info!(
            switched_on = changes.iter().filter(|(_, on)| *on).count(),
            switched_off = changes.iter().filter(|(_, on)| !*on).count(),
            "[FormatPolicy: Apply] File format switches changed; adjusting folder counts and rebuilding the lists"
        );

        let weak = window.as_weak();
        let (library, thumbnails, tab_manager, settings, state) =
            (library.clone(), thumbnails.clone(), tab_manager.clone(), settings.clone(), state.clone());
        let spawned = std::thread::Builder::new().name("format-policy".into()).spawn(move || {
            let report = library.folder_format_counts().apply_change(old, new).unwrap_or_else(|e| {
                tracing::error!(error = %e, "[FormatPolicy: Counts] Could not adjust the folder counts; a tree refresh corrects them");
                FormatCountReport::default()
            });
            tracing::info!(
                folders = report.folders_changed,
                hidden = report.hidden,
                restored = report.restored,
                "[FormatPolicy: Counts] Folder counts adjusted"
            );
            let _ = weak.upgrade_in_event_loop(move |window| {
                state.write().invalidate_folder_hierarchy();
                BridgeFolders::reload_tree(&window, &library, &settings, &state);
                Self::rebuild_list(&window, &library, &thumbnails, &tab_manager, &settings, &state);
                BridgeInspector::update(&window, &state);
                StatusBarBridge::update_metrics(&window, &state);
                StatusBarBridge::info(&window, Self::summary(&changes, report));
            });
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[FormatPolicy: Apply] Could not start the worker thread");
        }
    }

    /// The open folder or the open query is built again from the library with the new policy.
    fn rebuild_list(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (is_query, has_folder) = {
            let st = state.read();
            (st.is_query_active, st.current_folder_id.is_some())
        };
        if is_query {
            BridgeTags::execute_query_flow(&TagActionContext { window, library, thumbnails, tab_manager, settings, state });
        } else if has_folder {
            BridgeMedia::reload(window, library, thumbnails, settings, state, true, true);
        }
    }

    /// The status line after a change: what is hidden now, and that new files of a switched-on format need a refresh.
    pub fn summary(changes: &[(MediaFormat, bool)], report: FormatCountReport) -> String {
        let named = |on: bool| -> String {
            let names: Vec<&str> = changes.iter().filter(|(_, enabled)| *enabled == on).map(|(f, _)| f.display_name()).collect();
            match names.len() {
                0 => String::new(),
                1..=3 => names.join(", "),
                n => format!("{n} formats"),
            }
        };
        let mut parts = Vec::new();
        let off = named(false);
        if !off.is_empty() {
            parts.push(match report.hidden {
                0 => format!("Switched off: {off}"),
                1 => format!("Switched off: {off} (1 indexed file hidden)"),
                n => format!("Switched off: {off} ({n} indexed files hidden)"),
            });
        }
        let on = named(true);
        if !on.is_empty() {
            parts.push(format!("Switched on: {on}. Files of {} that were not indexed yet appear after the next refresh.", if changes.iter().filter(|(_, e)| *e).count() == 1 { "this format" } else { "these formats" }));
        }
        format!("File formats: {}", parts.join(". "))
    }
}
