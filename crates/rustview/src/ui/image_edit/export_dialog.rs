//! The export dialog ("Save As"): name, folder, format and its options, "Same as original" and whether the rating and tags go
//! along. It decides nothing itself (`export_plan` and `export_target` do); it reads the controls, checks the name and the
//! folder and whether a file of that name exists (on every change, while the user types: the dialog is an explicit save) and
//! hands the choice back.

use super::export_plan::{
    check_stem, format_index, format_of, png_index, png_of, subsampling_index, subsampling_of, target_path, tiff_index,
    tiff_of, warnings,
};
use super::export_target::{FolderCheck, check_folder, extension_for};
use crate::ui::{ImageExportDialog, MainWindow};
use rustview_core::image_edit::formats::WriteFormat;
use rustview_core::image_edit::options::EncodeOptions;
use rustview_core::image_edit::save::EncodingChoice;
use rustview_core::library::file_operations::is_same_file_path;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// What the dialog starts with.
#[derive(Debug, Clone)]
pub struct ExportSetup {
    /// Why the picture is saved as a new file (empty for a plain Save As).
    pub note: String,
    /// The original file (its folder, name and format are where the dialog starts; the unchanged dialog replaces it).
    pub source: PathBuf,
    pub stem: String,
    pub format: WriteFormat,
    pub options: EncodeOptions,
    /// The format the original is saved in when it can be written ("Same as original" exists for it).
    pub own_format: Option<WriteFormat>,
    pub same_as_original: bool,
    /// How "Same as original" saves this picture.
    pub same_hint: String,
    /// The rating and tags of the original in words; empty = it has none.
    pub assignments: String,
    pub size: (u32, u32),
    pub has_alpha: Option<bool>,
    pub bit_depth: Option<u8>,
}

/// What the user chose.
#[derive(Debug, Clone)]
pub struct ExportChoice {
    pub path: PathBuf,
    pub format: WriteFormat,
    pub encoding: EncodingChoice,
    /// A file of that name exists and the user chose to replace it.
    pub replace: bool,
    /// The file is the original itself (saved over it like Save, with the encoding of the dialog).
    pub is_original: bool,
    /// The folder does not exist yet: it is created before the file is written.
    pub create_folder: bool,
    /// The new file takes over the rating and the tags of the original.
    pub keep_assignments: bool,
    /// "Same as original" was on (remembered for the next time).
    pub same_as_original: bool,
}

type Answer = Box<dyn FnOnce(Option<ExportChoice>)>;

struct Pending {
    setup: ExportSetup,
    answer: Answer,
}

thread_local! {
    static DIALOG: RefCell<Option<ImageExportDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

/// The state of the controls, checked.
struct Checked {
    target: Option<PathBuf>,
    format: WriteFormat,
    folder: FolderCheck,
    error: String,
    exists: bool,
    is_original: bool,
}

pub struct ExportDialog;

impl ExportDialog {
    pub fn ask(window: &MainWindow, setup: ExportSetup, answer: impl FnOnce(Option<ExportChoice>) + 'static) {
        Self::finish(None);
        let created = DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match ImageExportDialog::new() {
                    Ok(win) => {
                        crate::ui::dialog_fit::install(&win);
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[ImageEdit: Export] The dialog could not be created");
                    }
                }
            }
            dialog.as_ref().map(ComponentHandle::clone_strong)
        });
        let Some(win) = created else {
            answer(None);
            return;
        };
        Self::fill(&win, &setup);
        let (stem_len, format) = (setup.stem.len(), setup.format);
        PENDING.with(|p| {
            *p.borrow_mut() = Some(Pending {
                setup,
                answer: Box::new(answer),
            });
        });
        // Everything the dialog shows is in place before the window exists: it opens at the height of its content.
        Self::revalidate();
        window.set_is_context_menu_open(false);
        window.set_is_modal_locked(true);
        if win.show().is_err() {
            tracing::error!("[ImageEdit: Export] The dialog could not be shown");
            Self::hide(window);
            Self::finish(None);
            return;
        }
        // The cursor at the end of the name: the user can type a new name (or add to it) at once.
        win.invoke_focus_name(stem_len.saturating_as::<i32>());
        tracing::info!(format = format.id(), "[ImageEdit: Export] Dialog shown");
    }

    fn fill(win: &ImageExportDialog, setup: &ExportSetup) {
        let o = setup.options.sanitized();
        let folder = setup.source.parent().map(Path::to_path_buf).unwrap_or_default();
        win.set_note(setup.note.as_str().into());
        win.set_file_name(setup.stem.as_str().into());
        win.set_folder(folder.display().to_string().into());
        win.set_format_index(format_index(setup.format));
        win.set_jpeg_quality(i32::from(o.jpeg_quality));
        win.set_jpeg_subsampling_index(subsampling_index(o.jpeg_subsampling));
        win.set_jpeg_progressive(o.jpeg_progressive);
        win.set_png_compression_index(png_index(o.png_compression));
        win.set_webp_lossless(o.webp_lossless);
        win.set_webp_quality(i32::from(o.webp_quality));
        win.set_avif_quality(i32::from(o.avif_quality));
        win.set_avif_speed(i32::from(o.avif_speed));
        win.set_tiff_compression_index(tiff_index(o.tiff_compression));
        win.set_same_as_original(setup.same_as_original);
        win.set_same_hint(setup.same_hint.as_str().into());
        win.set_keep_assignments(true);
        win.set_size_text(format!("{} x {} px", setup.size.0, setup.size.1).into());
    }

    /// The options as the controls show them.
    fn options(win: &ImageExportDialog) -> EncodeOptions {
        EncodeOptions {
            jpeg_quality: win.get_jpeg_quality().clamp(1, 100).saturating_as::<u8>(),
            jpeg_subsampling: subsampling_of(win.get_jpeg_subsampling_index()),
            jpeg_progressive: win.get_jpeg_progressive(),
            png_compression: png_of(win.get_png_compression_index()),
            webp_lossless: win.get_webp_lossless(),
            webp_quality: win.get_webp_quality().clamp(1, 100).saturating_as::<u8>(),
            avif_quality: win.get_avif_quality().clamp(1, 100).saturating_as::<u8>(),
            avif_speed: win.get_avif_speed().clamp(1, 10).saturating_as::<u8>(),
            tiff_compression: tiff_of(win.get_tiff_compression_index()),
        }
        .sanitized()
    }

    /// Reads and checks the controls against the setup.
    fn check(win: &ImageExportDialog, setup: &ExportSetup) -> Checked {
        let format = format_of(win.get_format_index());
        let original_name = setup
            .source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let extension = extension_for(format, &original_name);
        win.set_extension(extension.as_str().into());
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let folder = check_folder(&win.get_folder(), home.as_deref());
        let stem = win.get_file_name().to_string();
        let target = folder.folder().map(|f| target_path(f, &stem, &extension));
        let mut error = match (&folder, check_stem(&stem)) {
            (_, Err(e)) => e,
            (FolderCheck::Invalid(e), Ok(())) => e.clone(),
            _ => String::new(),
        };
        let found = target
            .as_deref()
            .filter(|_| error.is_empty())
            .map(Path::symlink_metadata);
        if let Some(Ok(meta)) = &found
            && meta.is_dir()
        {
            error = "A folder with this name is there; choose another name.".to_string();
        }
        let exists = error.is_empty() && matches!(found, Some(Ok(_)));
        let is_original = exists && target.as_deref().is_some_and(|t| is_same_file_path(t, &setup.source));
        Checked {
            target,
            format,
            folder,
            error,
            exists,
            is_original,
        }
    }

    /// Checks the name and the folder and shows what the format loses (called on every change).
    fn revalidate() {
        let Some(setup) = PENDING.with(|p| p.borrow().as_ref().map(|p| p.setup.clone())) else {
            return;
        };
        DIALOG.with(|d| {
            let Some(win) = d.borrow().as_ref().map(ComponentHandle::clone_strong) else {
                return;
            };
            let checked = Self::check(&win, &setup);
            win.set_error(checked.error.as_str().into());
            win.set_exists(checked.exists);
            win.set_exists_text(
                if checked.is_original {
                    "This is the original file: saving replaces it with the edited picture."
                } else {
                    "A file with this name is in the folder; saving replaces it."
                }
                .into(),
            );
            win.set_folder_notice(
                if matches!(checked.folder, FolderCheck::Create(_)) {
                    "The folder does not exist yet; it is created when you save."
                } else {
                    ""
                }
                .into(),
            );
            win.set_same_available(setup.own_format == Some(checked.format));
            win.set_assignments(
                if checked.is_original {
                    ""
                } else {
                    setup.assignments.as_str()
                }
                .into(),
            );
            let lines: Vec<SharedString> = warnings(checked.format, setup.has_alpha, setup.bit_depth)
                .into_iter()
                .map(Into::into)
                .collect();
            win.set_warnings(ModelRc::from(Rc::new(VecModel::from(lines))));
        });
    }

    pub fn close(window: &MainWindow) {
        Self::hide(window);
        Self::finish(None);
    }

    fn hide(window: &MainWindow) {
        DIALOG.with(|d| {
            if let Some(win) = d.borrow().as_ref() {
                let _ = win.hide();
            }
        });
        window.set_is_modal_locked(false);
    }

    fn finish(choice: Option<ExportChoice>) {
        let pending = PENDING.with(|p| p.borrow_mut().take());
        if let Some(pending) = pending {
            (pending.answer)(choice);
        }
    }

    /// The choice of the controls, when they can be used.
    fn choice(win: &ImageExportDialog) -> Option<ExportChoice> {
        let setup = PENDING.with(|p| p.borrow().as_ref().map(|p| p.setup.clone()))?;
        let checked = Self::check(win, &setup);
        if !checked.error.is_empty() {
            return None;
        }
        let same = setup.own_format == Some(checked.format) && win.get_same_as_original();
        let options = Self::options(win);
        Some(ExportChoice {
            path: checked.target?,
            format: checked.format,
            encoding: if same {
                EncodingChoice::SameAsOriginal(options)
            } else {
                EncodingChoice::Options(options)
            },
            replace: checked.exists,
            is_original: checked.is_original,
            create_folder: matches!(checked.folder, FolderCheck::Create(_)),
            keep_assignments: win.get_keep_assignments() && !setup.assignments.is_empty() && !checked.is_original,
            same_as_original: win.get_same_as_original(),
        })
    }

    fn bind_events(win: &ImageExportDialog, window: &MainWindow) {
        win.on_changed(Self::revalidate);
        let dialog = win.as_weak();
        win.on_browse(move || {
            let Some(win) = dialog.upgrade() else { return };
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let current = check_folder(&win.get_folder(), home.as_deref());
            let mut picker = rfd::FileDialog::new().set_title("Save the picture in");
            if let FolderCheck::Ready(folder) = &current {
                picker = picker.set_directory(folder);
            }
            if let Some(folder) = picker.pick_folder() {
                tracing::info!(folder = %LogObscurer::path(&folder), "[ImageEdit: Export] Folder chosen");
                win.set_folder(folder.display().to_string().into());
                Self::revalidate();
            }
        });
        let (weak, dialog) = (window.as_weak(), win.as_weak());
        win.on_save(move || {
            let (Some(window), Some(win)) = (weak.upgrade(), dialog.upgrade()) else {
                return;
            };
            Self::revalidate();
            let Some(choice) = Self::choice(&win) else { return };
            Self::hide(&window);
            Self::finish(Some(choice));
        });
        let weak = window.as_weak();
        win.on_cancel(move || {
            if let Some(window) = weak.upgrade() {
                Self::hide(&window);
            }
            Self::finish(None);
        });
        let weak = window.as_weak();
        win.window().on_close_requested(move || {
            if let Some(window) = weak.upgrade() {
                window.set_is_modal_locked(false);
            }
            Self::finish(None);
            CloseRequestResponse::HideWindow
        });
    }
}
