//! Pasting the files of the clipboard into a folder.
//!
//! The clipboard is read and planned on a worker thread (`PastePlanner` looks at every file); the transfer then runs on the UI thread in
//! small time slices like the drop transfers, so the interface stays alive and the existing conflict dialog can ask between two files.
//! While it runs `UiState::is_pasting` and the busy flag of the window lock the same controls a scan locks.

use super::report::{self, PasteStats, MAX_LISTED_FAILURES};
use super::summary::ClipboardSummary;
use super::{ClipboardBridge, PasteBlock};
use crate::library::error::LibraryError;
use crate::library::file_operations::FileOperations;
use crate::library::models::{FileConflictReport, FileConflictResolution, FileTransferKind, FolderId, MediaId};
use crate::library::paste::{PasteChoice, PasteExecutor, PasteItem, PastePlan, PastePlanner, SkipReason};
use crate::settings::LogObscurer;
use crate::system::file_clipboard::{ClipboardMode, FileClipboard};
use crate::ui::bridge_dnd::BridgeDnd;
use crate::ui::bridge_file_dialog_conflict::ConflictDialogParams;
use crate::ui::bridge_folders::BridgeFolders;
use crate::ui::bridge_inspector::BridgeInspector;
use crate::ui::bridge_media::BridgeMedia;
use crate::ui::bridge_tags::BridgeTags;
use crate::ui::{MainWindow, NoticeContent, StatusBarBridge};
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Longest stretch of file operations on the UI thread before the event loop gets its turn.
const TIME_SLICE: Duration = Duration::from_millis(12);
const PASTE_LABEL: &str = "Pasting Files: ";
const SCAN_LABEL: &str = "Scanning Media: ";

fn kind_of(mode: ClipboardMode) -> FileTransferKind {
    match mode {
        ClipboardMode::Copy => FileTransferKind::Copy,
        ClipboardMode::Cut => FileTransferKind::Move,
    }
}

/// What the worker thread found on the clipboard.
enum Planned {
    Ready { mode: ClipboardMode, plan: PastePlan, total: usize },
    NoFiles,
    Failed(String),
}

impl ClipboardBridge {
    /// Starts a paste into `folder_id`: every guard first (nothing is touched when one refuses), then the lock, then the worker.
    pub(super) fn paste_into(&self, window: &MainWindow, folder_id: FolderId) {
        if !FileClipboard::is_supported() {
            StatusBarBridge::warn(window, "Cannot paste: the file clipboard needs an X11 session.");
            return;
        }
        if self.state.read().is_pasting {
            StatusBarBridge::warn(window, "A paste is already running.");
            return;
        }
        if BridgeMedia::is_system_busy(window, &self.thumbnails, &self.state) {
            StatusBarBridge::warn(window, "Cannot paste: an active operation is in progress.");
            return;
        }
        let folder = match self.library.folders().get_folder(folder_id) {
            Ok(Some(folder)) => folder,
            _ => {
                StatusBarBridge::warn(window, "Cannot paste: the folder is not in the library any more.");
                return;
            }
        };
        if folder.is_deleted {
            self.explain_block(window, &PasteBlock::MissingFolder(folder.name.clone()));
            return;
        }
        let path = PathBuf::from(&folder.path);
        if let Some(reason) = BridgeDnd::transfer_block_reason(window, &self.state, Some((&folder.name, &path))) {
            StatusBarBridge::warn(window, reason.replace("move or copy files", "paste files"));
            return;
        }

        self.lock(window);
        let library = self.library.clone();
        let spawned = std::thread::Builder::new().name("paste-plan".into()).spawn(move || {
            let planned = match FileClipboard::read() {
                Ok(Some(files)) => {
                    let total = files.paths.len();
                    match PastePlanner::new(&library).plan(&files.paths, folder_id, kind_of(files.mode)) {
                        Ok(plan) => Planned::Ready { mode: files.mode, plan, total },
                        Err(e) => Planned::Failed(e.to_string()),
                    }
                }
                Ok(None) => Planned::NoFiles,
                Err(e) => Planned::Failed(e.to_string()),
            };
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(bridge) = ClipboardBridge::current() {
                    bridge.planned(folder_id, planned);
                }
            });
        });
        if let Err(e) = spawned {
            tracing::error!(error = %e, "[Clipboard: Paste] The worker thread could not be started");
            self.unlock(window);
            StatusBarBridge::error(window, "Cannot paste: a worker thread could not be started.");
        }
    }

    fn planned(&self, folder_id: FolderId, planned: Planned) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let (mode, plan, total) = match planned {
            Planned::Ready { mode, plan, total } => (mode, plan, total),
            Planned::NoFiles => {
                self.unlock(&window);
                ClipboardSummary::of(None).publish();
                StatusBarBridge::info(&window, "Nothing to paste: the clipboard holds no files.");
                return;
            }
            Planned::Failed(why) => {
                self.unlock(&window);
                tracing::warn!(error = %LogObscurer::query(&why), "[Clipboard: Paste] The clipboard could not be read or planned");
                StatusBarBridge::error(&window, format!("Cannot paste: {why}."));
                return;
            }
        };
        let Ok(Some(folder)) = self.library.folders().get_folder(folder_id) else {
            self.unlock(&window);
            StatusBarBridge::warn(&window, "Cannot paste: the folder is not in the library any more.");
            return;
        };
        if plan.items.is_empty() {
            self.unlock(&window);
            self.nothing_to_paste(&window, &plan, total, &folder.name);
            return;
        }
        tracing::info!(items = plan.items.len(), skipped = plan.skipped.len(), mode = ?mode, "[Clipboard: Paste] Pasting");
        PasteRun {
            bridge: self.clone(),
            window,
            folder_id,
            folder_name: folder.name,
            folder_path: PathBuf::from(folder.path),
            mode,
            kind: kind_of(mode),
            plan,
            index: 0,
            stats: PasteStats::default(),
            first_failure: None,
            failures: Vec::new(),
            vanished: Vec::new(),
            remembered: None,
            transferred: HashSet::new(),
            moved_ids: HashSet::new(),
            needs_scan: false,
            started: Instant::now(),
        }
        .run();
    }

    fn nothing_to_paste(&self, window: &MainWindow, plan: &PastePlan, total: usize, folder_name: &str) {
        let notable = plan.skipped.iter().filter(|s| report::is_notable(&s.reason)).count();
        if notable == 0 {
            let in_folder = plan.skipped_because(&SkipReason::AlreadyInFolder);
            let message = if in_folder > 0 {
                format!("Nothing to paste: {} already in '{folder_name}'.", if in_folder == 1 { "the file is" } else { "the files are" })
            } else {
                "Nothing to paste: the clipboard holds no files.".to_string()
            };
            StatusBarBridge::info(window, message);
            return;
        }
        self.notice_dialog.show(
            window,
            NoticeContent {
                window_title: "RustView - Paste".to_string(),
                heading: "Nothing Could Be Pasted".to_string(),
                message: format!("None of the {} on the clipboard can be pasted into '{folder_name}'.", report::files(total)),
                details: report::notice_details(&plan.skipped, &[], &[]),
                footnote: "Only images and videos that exist on disk are pasted.".to_string(),
            },
        );
    }

    pub(super) fn explain_block(&self, window: &MainWindow, block: &PasteBlock) {
        let (heading, message, footnote) = block.explanation();
        self.notice_dialog.show(
            window,
            NoticeContent {
                window_title: "RustView - Paste".to_string(),
                heading: heading.to_string(),
                message,
                details: String::new(),
                footnote: footnote.to_string(),
            },
        );
    }

    fn lock(&self, window: &MainWindow) {
        self.state.write().is_pasting = true;
        window.set_media_index_label(PASTE_LABEL.into());
        StatusBarBridge::update_media_index_progress(window, true, 0, 0);
        window.set_is_system_busy(true);
    }

    fn unlock(&self, window: &MainWindow) {
        self.state.write().is_pasting = false;
        StatusBarBridge::update_media_index_progress(window, false, 0, 0);
        window.set_media_index_label(SCAN_LABEL.into());
        window.set_is_system_busy(BridgeMedia::is_system_busy(window, &self.thumbnails, &self.state));
    }

    /// The folder was filled with files the library does not list yet; it is scanned without opening it. The lock stays until done.
    fn scan_in_background(&self, window: &MainWindow, folder_id: FolderId) {
        self.lock(window);
        window.set_media_index_label("Scanning Folder: ".into());
        let library = self.library.clone();
        let spawned = std::thread::Builder::new().name("paste-scan".into()).spawn(move || {
            let result = library.refresher().refresh_folder(folder_id).map(|r| r.new_media).map_err(|e| e.to_string());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(bridge) = ClipboardBridge::current() {
                    bridge.background_scan_done(result);
                }
            });
        });
        if spawned.is_err() {
            self.unlock(window);
        }
    }

    fn background_scan_done(&self, result: Result<usize, String>) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        self.unlock(&window);
        match result {
            Ok(new_media) => tracing::info!(new_media, "[Clipboard: Paste] The target folder was scanned after the paste"),
            Err(why) => {
                tracing::warn!(error = %LogObscurer::query(&why), "[Clipboard: Paste] The target folder could not be scanned");
                StatusBarBridge::warn(&window, "The pasted files were not listed: the folder could not be scanned. Refresh it to list them.");
            }
        }
        self.state.write().invalidate_folder_hierarchy();
        BridgeFolders::reload_tree(&window, &self.library, &self.settings, &self.state);
        BridgeMedia::reload(&window, &self.library, &self.thumbnails, &self.settings, &self.state, false, true);
        StatusBarBridge::update_metrics(&window, &self.state);
    }
}

/// One paste in progress: the plan, the position in it and what happened so far. Consumed and handed on like the drop transfers.
struct PasteRun {
    bridge: ClipboardBridge,
    window: MainWindow,
    folder_id: FolderId,
    folder_name: String,
    folder_path: PathBuf,
    mode: ClipboardMode,
    kind: FileTransferKind,
    plan: PastePlan,
    index: usize,
    stats: PasteStats,
    first_failure: Option<String>,
    failures: Vec<(String, String)>,
    /// Files that were listed but gone when their turn came.
    vanished: Vec<String>,
    remembered: Option<FileConflictResolution>,
    transferred: HashSet<PathBuf>,
    moved_ids: HashSet<MediaId>,
    needs_scan: bool,
    started: Instant,
}

impl PasteRun {
    fn run(mut self) {
        let slice = Instant::now();
        while self.index < self.plan.items.len() {
            let item = self.plan.items[self.index].clone();
            StatusBarBridge::update_media_index_progress(&self.window, true, self.index, self.plan.items.len());
            let conflict = PasteExecutor::new(&self.bridge.library).conflict(&item, self.folder_id, self.kind);
            match conflict {
                Ok(Some(report)) => {
                    let Some(resolution) = self.remembered else {
                        return self.ask(item, report);
                    };
                    if self.resolve(&item, &report, resolution) {
                        return self.finish();
                    }
                }
                Ok(None) => {
                    self.transfer(&item, PasteChoice::Direct);
                }
                Err(e) => self.note_failure(&item, &e),
            }
            self.index += 1;
            if slice.elapsed() > TIME_SLICE {
                Timer::single_shot(Duration::from_millis(1), move || self.run());
                return;
            }
        }
        self.finish();
    }

    /// Opens the conflict dialog; the answer carries on with the next file.
    fn ask(self, item: PasteItem, report: FileConflictReport) {
        let window = self.window.clone_strong();
        let thumbnails = self.bridge.thumbnails.clone();
        let settings = self.bridge.settings.clone();
        let dialog = self.bridge.conflict_dialog.clone();
        let kind = self.kind;
        let (position, total) = (self.index + 1, self.plan.items.len());
        let shown = report.clone();
        let cell = Rc::new(RefCell::new(Some(self)));
        dialog.show(
            ConflictDialogParams {
                window: &window,
                thumbnails: &thumbnails,
                settings: &settings,
                report: &shown,
                conflict_index: position,
                total_conflicts: total,
                transfer_kind: kind,
            },
            move |resolution, apply_to_all| {
                let Some(mut run) = cell.borrow_mut().take() else {
                    return;
                };
                if apply_to_all {
                    run.remembered = Some(resolution);
                }
                let cancelled = run.resolve(&item, &report, resolution);
                // The dialog is still inside its own callback: carry on once it is done.
                Timer::single_shot(Duration::from_millis(1), move || {
                    if cancelled {
                        run.finish();
                    } else {
                        run.index += 1;
                        run.run();
                    }
                });
            },
        );
    }

    /// Settles one collision. True when the user cancelled the whole paste.
    fn resolve(&mut self, item: &PasteItem, report: &FileConflictReport, resolution: FileConflictResolution) -> bool {
        match resolution {
            FileConflictResolution::Skip => {
                tracing::info!(file = %LogObscurer::filename(&report.source.filename), "[Clipboard: Resolution] Skipped, the file stays where it is");
                self.stats.skipped += 1;
            }
            FileConflictResolution::AutoRename => {
                let name = FileOperations::generate_unique_filename(&self.folder_path, &item.name);
                if self.transfer(item, PasteChoice::Rename(name)) {
                    self.stats.renamed += 1;
                }
            }
            FileConflictResolution::Overwrite => {
                if self.transfer(item, PasteChoice::Overwrite) {
                    self.stats.overwritten += 1;
                }
            }
            FileConflictResolution::Cancel => {
                self.stats.cancelled = self.plan.items.len().saturating_sub(self.index);
                return true;
            }
        }
        false
    }

    /// Transfers one file. True when it worked.
    fn transfer(&mut self, item: &PasteItem, choice: PasteChoice) -> bool {
        let result = PasteExecutor::new(&self.bridge.library).transfer(item, self.folder_id, self.kind, &choice);
        match result {
            Ok(outcome) => {
                self.stats.done += 1;
                self.transferred.insert(item.path.clone());
                self.needs_scan |= outcome.needs_scan;
                if self.kind == FileTransferKind::Move
                    && let Some(id) = item.known
                {
                    self.moved_ids.insert(id);
                }
                true
            }
            Err(LibraryError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                self.stats.missing += 1;
                self.vanished.push(item.name.clone());
                false
            }
            Err(e) => {
                self.note_failure(item, &e);
                false
            }
        }
    }

    fn note_failure(&mut self, item: &PasteItem, error: &dyn std::fmt::Display) {
        let reason = error.to_string();
        tracing::warn!(file = %LogObscurer::filename(&item.name), error = %LogObscurer::query(&reason), "[Clipboard: Paste] A file could not be pasted");
        self.stats.failed += 1;
        self.first_failure.get_or_insert_with(|| reason.clone());
        if self.failures.len() < MAX_LISTED_FAILURES {
            self.failures.push((item.name.clone(), reason));
        }
    }

    fn finish(self) {
        let bridge = self.bridge.clone();
        let window = self.window.clone_strong();
        let elapsed_ms = self.started.elapsed().as_millis();

        if self.kind == FileTransferKind::Move {
            self.settle_clipboard();
        }
        bridge.unlock(&window);

        let visible_plain_target = {
            let mut st = bridge.state.write();
            st.invalidate_folder_hierarchy();
            st.file_selection.deselect_many(&self.moved_ids);
            st.current_folder_id == Some(self.folder_id) && st.is_plain_folder_view()
        };

        if self.needs_scan && visible_plain_target && !BridgeMedia::is_system_busy(&window, &bridge.thumbnails, &bridge.state) {
            // The standard refresh of the open folder: scan, list, thumbnails, with its own progress and locks.
            BridgeFolders::refresh_folder_explicit(
                self.folder_id,
                self.folder_path.clone(),
                &window,
                &bridge.library,
                &bridge.thumbnails,
                &bridge.settings,
                &bridge.state,
            );
        } else {
            BridgeFolders::reload_tree(&window, &bridge.library, &bridge.settings, &bridge.state);
            BridgeMedia::reload(&window, &bridge.library, &bridge.thumbnails, &bridge.settings, &bridge.state, false, true);
            if self.needs_scan {
                bridge.scan_in_background(&window, self.folder_id);
            }
        }
        BridgeInspector::update(&window, &bridge.state);
        BridgeTags::sync_tag_selection(&window, &bridge.library, &bridge.state);

        let line = report::status_line(self.mode, &self.folder_name, &self.stats, self.first_failure.as_deref(), elapsed_ms);
        if self.stats.failed > 0 || self.stats.missing > 0 || self.stats.cancelled > 0 {
            StatusBarBridge::warn(&window, line);
        } else if self.stats.done == 0 {
            StatusBarBridge::info(&window, line);
        } else {
            StatusBarBridge::success(&window, line);
        }
        StatusBarBridge::update_metrics(&window, &bridge.state);
        self.report_problems(&window);
    }

    /// After a cut was pasted the clipboard no longer offers what moved: it is emptied, or keeps the files that were not moved.
    fn settle_clipboard(&self) {
        if self.stats.done == 0 {
            return;
        }
        let mut remaining: Vec<PathBuf> =
            self.plan.items.iter().filter(|i| !self.transferred.contains(&i.path)).map(|i| i.path.clone()).collect();
        remaining.extend(
            self.plan
                .skipped
                .iter()
                .filter(|s| !matches!(s.reason, SkipReason::SourceMissing | SkipReason::AlreadyInFolder | SkipReason::Duplicate))
                .map(|s| s.path.clone()),
        );
        if remaining.is_empty() {
            FileClipboard::empty();
        } else if let Err(e) = FileClipboard::set(&remaining, ClipboardMode::Cut) {
            tracing::warn!(error = %e, "[Clipboard: Paste] The files left over could not be put back on the clipboard");
        }
    }

    fn report_problems(&self, window: &MainWindow) {
        let notable: Vec<_> = self.plan.skipped.iter().filter(|s| report::is_notable(&s.reason)).cloned().collect();
        if notable.is_empty() && self.vanished.is_empty() && self.failures.is_empty() {
            return;
        }
        let problems = notable.len() + self.vanished.len() + self.stats.failed;
        let missing_known = self.plan.items.iter().any(|i| i.known.is_some() && self.vanished.contains(&i.name));
        let footnote = if !self.vanished.is_empty() || notable.iter().any(|s| s.reason == SkipReason::SourceMissing) {
            let flagged = if missing_known { " Entries RustView lists for them are flagged as missing; remove them with 'Remove from Database'." } else { "" };
            format!("Files that no longer exist were probably moved or deleted by another program.{flagged}")
        } else {
            "Only images and videos that exist on disk are pasted.".to_string()
        };
        self.bridge.notice_dialog.show(
            window,
            NoticeContent {
                window_title: "RustView - Paste".to_string(),
                heading: "Some Files Were Not Pasted".to_string(),
                message: format!("{} could not be pasted into '{}'.", report::files(problems), self.folder_name),
                details: report::notice_details(&self.plan.skipped, &self.vanished, &self.failures),
                footnote,
            },
        );
    }
}
