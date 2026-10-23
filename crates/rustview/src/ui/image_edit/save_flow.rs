//! Saving the edits of one tile: over the original when its format can be written, else (and for "Save As") through the export
//! dialog. The file is written on a worker; the answer comes back to the UI thread, where a conflict or a failure is put to
//! the user (`choice_dialog`) and a success updates the library and every view of the medium (`library_update`). The original
//! is never changed by a save that does not succeed (`rustview_core::image_edit::save`).

use super::choice_dialog::{Choice, ChoiceDialog, Question};
use super::export_dialog::{ExportChoice, ExportDialog, ExportSetup};
use super::export_plan::{initial_format, suggested_stem};
use super::library_update;
use super::{AfterSave, EditContext};
use crate::ui::MainWindow;
use crate::ui::status_bar::StatusBarBridge;
use crate::viewer::edit::prefs as edit_prefs;
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
use crate::viewer::multiview::instance_edit_view::SaveRequest;
use rustview_core::image_edit::conflict::{ConflictChoice, DiskState, conflict_prompt};
use rustview_core::image_edit::formats::EditRefusal;
use rustview_core::image_edit::formats::SaveSupport;
use rustview_core::image_edit::save::{EditedPicture, ExportTarget, SaveError, SaveOutcome, export, save_in_place};
use rustview_core::image_edit::writer::WriteError;
use rustview_core::settings::LogObscurer;
use rustview_core::settings::schema::FormatPresets;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveIntent {
    /// Over the original (through the export dialog when its format cannot be written).
    Save,
    /// To a new file of the user's choice.
    SaveAs,
}

/// Saves the edits of tile `key` of `instance`. `after` learns whether the edits are done (saved or deliberately dropped) or
/// still open (cancelled, failed).
pub fn save_tile(context: &EditContext, instance: &SharedInstance, key: TileKey, intent: SaveIntent, after: AfterSave) {
    let request = instance.lock().save_request(key);
    let Some(request) = request else {
        // Nothing to save (or a save is running already).
        finish(
            after,
            !instance.lock().unsaved_entries_of(&[key]).iter().any(|e| e.tile == key),
        );
        return;
    };
    let weak = Arc::downgrade(instance);
    match (intent, request.source.support()) {
        (_, Err(refusal)) => {
            if let Some(window) = context.window.upgrade() {
                StatusBarBridge::error(&window, refusal.text());
            }
            finish(after, false);
        }
        (SaveIntent::SaveAs, _) => ask_export(context, weak, request, String::new(), after),
        (SaveIntent::Save, Ok(SaveSupport::ExportOnly(reason))) => {
            ask_export(context, weak, request, reason.text(), after);
        }
        (SaveIntent::Save, Ok(SaveSupport::InPlace(_))) => write_in_place(context, weak, request, false, after),
    }
}

fn finish(after: AfterSave, done: bool) {
    if let Some(after) = after {
        after(done);
    }
}

fn picture(request: &SaveRequest) -> Option<EditedPicture<'_>> {
    Some(EditedPicture {
        path: &request.path,
        format: request.source.format,
        probe: request.source.probe()?,
        plan: request.plan,
        original_size: request.source.size,
        identity: request.source.identity,
    })
}

fn with_instance(
    context: &EditContext,
    weak: &WeakInstance,
    f: impl FnOnce(&mut crate::viewer::multiview::instance::MultiviewInstance),
) {
    if let Some(instance) = weak.upgrade() {
        f(&mut instance.lock());
        context.handle.refresh_instance(&instance);
    }
}

/// What a save running on a worker comes back to (it stays on the UI thread; the worker only carries the ticket).
struct Waiting {
    context: EditContext,
    weak: WeakInstance,
    request: SaveRequest,
    export: Option<ExportChoice>,
    after: AfterSave,
}

thread_local! {
    static WAITING: RefCell<HashMap<u64, Waiting>> = RefCell::new(HashMap::new());
}

static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

fn park(waiting: Waiting) -> u64 {
    let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
    WAITING.with(|w| w.borrow_mut().insert(ticket, waiting));
    ticket
}

fn unpark(ticket: u64) -> Option<Waiting> {
    WAITING.with(|w| w.borrow_mut().remove(&ticket))
}

/// Writes over the original on a worker. `overwrite_anyway` is the user's answer to a conflict.
fn write_in_place(
    context: &EditContext,
    weak: WeakInstance,
    request: SaveRequest,
    overwrite_anyway: bool,
    after: AfterSave,
) {
    with_instance(context, &weak, |mv| mv.set_saving(request.tile, true));
    let prefs = edit_prefs::current();
    let job = request.clone();
    let ticket = park(Waiting {
        context: context.clone(),
        weak,
        request,
        export: None,
        after,
    });
    let spawned = std::thread::Builder::new().name("image-save".into()).spawn(move || {
        let result = match picture(&job) {
            Some(picture) => save_in_place(&picture, &prefs.presets, prefs.overwrite, overwrite_anyway),
            None => Err(SaveError::Refused(EditRefusal::Unreadable)),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = unpark(ticket) {
                in_place_done(&w.context, w.weak, w.request, result, w.after);
            }
        });
    });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[ImageEdit: Save] The save thread could not be started");
        if let Some(w) = unpark(ticket) {
            in_place_done(
                &w.context,
                w.weak,
                w.request,
                Err(SaveError::Render(e.to_string())),
                w.after,
            );
        }
    }
}

fn in_place_done(
    context: &EditContext,
    weak: WeakInstance,
    request: SaveRequest,
    result: Result<SaveOutcome, SaveError>,
    after: AfterSave,
) {
    let Some(window) = context.window.upgrade() else { return };
    match result {
        Ok(outcome) => {
            with_instance(context, &weak, |mv| {
                mv.save_finished(request.tile, outcome.identity, outcome.size);
            });
            StatusBarBridge::success(
                &window,
                format!("Saved '{}' ({}).", request.filename, outcome.encoding.describe()),
            );
            library_update::after_overwrite(context, &window, request.media_id);
            finish(after, true);
        }
        Err(SaveError::Conflict(state)) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            ask_conflict(context, &window, weak, request, &state, after);
        }
        Err(SaveError::NeedsExport(reason)) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            ask_export(context, weak, request, reason.text(), after);
        }
        Err(SaveError::NothingToSave) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            finish(after, true);
        }
        Err(error) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            tracing::error!(
                path = %LogObscurer::path(&request.path),
                error = %if LogObscurer::is_active() { "<obfuscated>".to_string() } else { error.to_string() },
                "[ImageEdit: Save] Saving failed; the original is unchanged"
            );
            ask_failure(context, &window, weak, request, &error, after);
        }
    }
}

/// The file changed on disk, is gone or cannot be checked: the user decides.
fn ask_conflict(
    context: &EditContext,
    window: &MainWindow,
    weak: WeakInstance,
    request: SaveRequest,
    state: &DiskState,
    after: AfterSave,
) {
    let Some(prompt) = conflict_prompt(state, &request.filename) else {
        write_in_place(context, weak, request, false, after);
        return;
    };
    let question = Question {
        heading: prompt.title,
        message: prompt.message,
        detail: request.path.display().to_string(),
        choices: prompt
            .choices
            .iter()
            .map(|(choice, label)| {
                let c = Choice::new(choice.code(), label.as_str());
                match choice {
                    ConflictChoice::SaveAsCopy => c.primary(),
                    ConflictChoice::OverwriteAnyway => c.destructive(),
                    _ => c,
                }
            })
            .collect(),
        cancel: ConflictChoice::Cancel.code(),
    };
    let context = context.clone();
    ChoiceDialog::ask(window, &question, move |code| match ConflictChoice::from_code(code) {
        Some(ConflictChoice::OverwriteAnyway) => write_in_place(&context, weak, request, true, after),
        Some(ConflictChoice::SaveAsCopy) => ask_export(&context, weak, request, String::new(), after),
        Some(ConflictChoice::DiscardAndReload) => {
            with_instance(&context, &weak, |mv| {
                mv.finish_edits_without_save(request.tile);
                mv.reload_member(request.media_id);
            });
            finish(after, true);
        }
        Some(ConflictChoice::Cancel) | None => finish(after, false),
    });
}

/// Writing failed (read-only file, full disk, no permission ...): the original is unchanged; the user can save a copy.
fn ask_failure(
    context: &EditContext,
    window: &MainWindow,
    weak: WeakInstance,
    request: SaveRequest,
    error: &SaveError,
    after: AfterSave,
) {
    let reason = match error {
        SaveError::Write(WriteError::ReadOnly) => "The file is read-only.".to_string(),
        other => format!("{}.", capitalized(&other.to_string())),
    };
    let question = Question {
        heading: "Picture Not Saved".to_string(),
        message: format!(
            "'{}' could not be saved. {reason} The original was not changed; the edits are still there.",
            request.filename
        ),
        detail: request.path.display().to_string(),
        choices: vec![
            Choice::new(ConflictChoice::SaveAsCopy.code(), "Save As Copy...").primary(),
            Choice::new(ConflictChoice::Cancel.code(), "Cancel"),
        ],
        cancel: ConflictChoice::Cancel.code(),
    };
    let context = context.clone();
    ChoiceDialog::ask(window, &question, move |code| {
        if code == ConflictChoice::SaveAsCopy.code() {
            ask_export(&context, weak, request, String::new(), after);
        } else {
            finish(after, false);
        }
    });
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// The export dialog for the edits of `request`; `note` says why the original is not overwritten.
fn ask_export(context: &EditContext, weak: WeakInstance, request: SaveRequest, note: String, after: AfterSave) {
    let Some(window) = context.window.upgrade() else { return };
    let settings = context.app.settings.get().image_edit;
    let last = settings.last_export.write_format();
    let probe = request.source.probe();
    let options = settings
        .last_export
        .options
        .as_ref()
        .filter(|_| last.is_some())
        .map_or_else(|| settings.presets.encode_options(), FormatPresets::encode_options);
    let setup = ExportSetup {
        note,
        folder: request
            .path
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_default(),
        stem: suggested_stem(&request.filename),
        format: initial_format(
            last,
            probe.and_then(|p| p.signature).unwrap_or(request.source.format),
            probe.and_then(|p| p.has_alpha),
        ),
        options,
        size: request.plan.output_size,
        has_alpha: probe.and_then(|p| p.has_alpha),
        bit_depth: probe.and_then(|p| p.bit_depth),
    };
    let context = context.clone();
    ExportDialog::ask(&window, &setup, move |choice| match choice {
        Some(choice) => write_export(&context, weak, request, choice, after),
        None => finish(after, false),
    });
}

fn write_export(
    context: &EditContext,
    weak: WeakInstance,
    request: SaveRequest,
    choice: ExportChoice,
    after: AfterSave,
) {
    remember_export(context, &choice);
    with_instance(context, &weak, |mv| mv.set_saving(request.tile, true));
    let (job, target) = (request.clone(), choice.clone());
    let ticket = park(Waiting {
        context: context.clone(),
        weak,
        request,
        export: Some(choice),
        after,
    });
    let spawned = std::thread::Builder::new().name("image-export".into()).spawn(move || {
        let export_target = ExportTarget {
            path: &target.path,
            format: target.format,
            options: target.options,
            replace: target.replace,
        };
        let result = match picture(&job) {
            Some(picture) => export(&picture, &export_target),
            None => Err(SaveError::Refused(EditRefusal::Unreadable)),
        };
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = unpark(ticket)
                && let Some(choice) = w.export
            {
                export_done(&w.context, w.weak, w.request, choice, result, w.after);
            }
        });
    });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[ImageEdit: Export] The export thread could not be started");
        if let Some(w) = unpark(ticket)
            && let Some(choice) = w.export
        {
            export_done(
                &w.context,
                w.weak,
                w.request,
                choice,
                Err(SaveError::Render(e.to_string())),
                w.after,
            );
        }
    }
}

fn export_done(
    context: &EditContext,
    weak: WeakInstance,
    request: SaveRequest,
    choice: ExportChoice,
    result: Result<SaveOutcome, SaveError>,
    after: AfterSave,
) {
    let Some(window) = context.window.upgrade() else { return };
    match result {
        Ok(outcome) => {
            // The original is unchanged: the tile shows it again, the edits live on in the new file.
            with_instance(context, &weak, |mv| mv.finish_edits_without_save(request.tile));
            let name = outcome
                .written
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            StatusBarBridge::success(
                &window,
                format!(
                    "Saved the edited picture as '{name}' ({}).",
                    outcome.encoding.describe()
                ),
            );
            library_update::after_export(context, &window, request.media_id, &outcome.written);
            finish(after, true);
        }
        Err(SaveError::Write(WriteError::Exists)) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            let question = Question {
                heading: "File Exists".to_string(),
                message: "A file with this name appeared in the folder meanwhile. Replace it?".to_string(),
                detail: choice.path.display().to_string(),
                choices: vec![
                    Choice::new(0, "Replace").destructive(),
                    Choice::new(1, "Choose Another Name...").primary(),
                    Choice::new(2, "Cancel"),
                ],
                cancel: 2,
            };
            let context = context.clone();
            ChoiceDialog::ask(&window, &question, move |code| match code {
                0 => write_export(
                    &context,
                    weak,
                    request,
                    ExportChoice {
                        replace: true,
                        ..choice
                    },
                    after,
                ),
                1 => ask_export(&context, weak, request, String::new(), after),
                _ => finish(after, false),
            });
        }
        Err(error) => {
            with_instance(context, &weak, |mv| mv.set_saving(request.tile, false));
            tracing::error!(
                error = %if LogObscurer::is_active() { "<obfuscated>".to_string() } else { error.to_string() },
                "[ImageEdit: Export] Writing the new file failed"
            );
            StatusBarBridge::error(&window, format!("The edited picture could not be saved: {error}"));
            let question = Question {
                heading: "Picture Not Saved".to_string(),
                message: format!(
                    "The new file could not be written: {error}. Nothing was changed; the edits are still there."
                ),
                detail: choice.path.display().to_string(),
                choices: vec![
                    Choice::new(1, "Choose Another Place...").primary(),
                    Choice::new(2, "Cancel"),
                ],
                cancel: 2,
            };
            let context = context.clone();
            ChoiceDialog::ask(&window, &question, move |code| {
                if code == 1 {
                    ask_export(&context, weak, request, String::new(), after);
                } else {
                    finish(after, false);
                }
            });
        }
    }
}

/// The export dialog starts with the same choices next time (persisted with the settings).
fn remember_export(context: &EditContext, choice: &ExportChoice) {
    let settings = &context.app.settings;
    let mut next = settings.get();
    next.image_edit.last_export.format = choice.format.id().to_string();
    next.image_edit.last_export.options = Some(FormatPresets::from_options(&choice.options));
    settings.update(&next);
}
