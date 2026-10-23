//! The export dialog ("Save As"): name, folder, format and its options. It decides nothing itself (`export_plan` does); it reads
//! the controls, checks the name and whether a file of that name exists (on every change, while the user types: the dialog is
//! an explicit save) and hands the choice back.

use super::export_plan::{
    check_stem, format_index, format_of, png_index, png_of, subsampling_index, subsampling_of, target_path, tiff_index,
    tiff_of, warnings,
};
use crate::ui::{ImageExportDialog, MainWindow};
use rustview_core::image_edit::formats::WriteFormat;
use rustview_core::image_edit::options::EncodeOptions;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::SaturatingAs;
use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// What the dialog starts with.
#[derive(Debug, Clone)]
pub struct ExportSetup {
    /// Why the picture is saved as a new file (empty for a plain Save As).
    pub note: String,
    pub folder: PathBuf,
    pub stem: String,
    pub format: WriteFormat,
    pub options: EncodeOptions,
    pub size: (u32, u32),
    pub has_alpha: Option<bool>,
    pub bit_depth: Option<u8>,
}

/// What the user chose.
#[derive(Debug, Clone)]
pub struct ExportChoice {
    pub path: PathBuf,
    pub format: WriteFormat,
    pub options: EncodeOptions,
    /// A file of that name exists and the user chose to replace it.
    pub replace: bool,
}

type Answer = Box<dyn FnOnce(Option<ExportChoice>)>;

struct Pending {
    folder: PathBuf,
    has_alpha: Option<bool>,
    bit_depth: Option<u8>,
    answer: Answer,
}

thread_local! {
    static DIALOG: RefCell<Option<ImageExportDialog>> = const { RefCell::new(None) };
    static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
}

pub struct ExportDialog;

impl ExportDialog {
    pub fn ask(window: &MainWindow, setup: &ExportSetup, answer: impl FnOnce(Option<ExportChoice>) + 'static) {
        Self::finish(None);
        let shown = DIALOG.with(|d| {
            let mut dialog = d.borrow_mut();
            if dialog.is_none() {
                match ImageExportDialog::new() {
                    Ok(win) => {
                        Self::bind_events(&win, window);
                        *dialog = Some(win);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "[ImageEdit: Export] The dialog could not be created");
                        return false;
                    }
                }
            }
            let Some(win) = dialog.as_ref() else { return false };
            Self::fill(win, setup);
            window.set_is_context_menu_open(false);
            window.set_is_modal_locked(true);
            win.show().is_ok()
        });
        if shown {
            PENDING.with(|p| {
                *p.borrow_mut() = Some(Pending {
                    folder: setup.folder.clone(),
                    has_alpha: setup.has_alpha,
                    bit_depth: setup.bit_depth,
                    answer: Box::new(answer),
                });
            });
            Self::revalidate();
            tracing::info!(format = setup.format.id(), "[ImageEdit: Export] Dialog shown");
        } else {
            answer(None);
        }
    }

    fn fill(win: &ImageExportDialog, setup: &ExportSetup) {
        let o = setup.options.sanitized();
        win.set_note(setup.note.as_str().into());
        win.set_file_name(setup.stem.as_str().into());
        win.set_folder(setup.folder.display().to_string().into());
        win.set_format_index(format_index(setup.format));
        win.set_extension(setup.format.extension().into());
        win.set_jpeg_quality(i32::from(o.jpeg_quality));
        win.set_jpeg_subsampling_index(subsampling_index(o.jpeg_subsampling));
        win.set_jpeg_progressive(o.jpeg_progressive);
        win.set_png_compression_index(png_index(o.png_compression));
        win.set_webp_lossless(o.webp_lossless);
        win.set_webp_quality(i32::from(o.webp_quality));
        win.set_avif_quality(i32::from(o.avif_quality));
        win.set_avif_speed(i32::from(o.avif_speed));
        win.set_tiff_compression_index(tiff_index(o.tiff_compression));
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

    /// Checks the name and the folder and shows what the format loses (called on every change).
    fn revalidate() {
        let Some((folder, alpha, depth)) = PENDING.with(|p| {
            p.borrow()
                .as_ref()
                .map(|p| (p.folder.clone(), p.has_alpha, p.bit_depth))
        }) else {
            return;
        };
        DIALOG.with(|d| {
            let Some(win) = d.borrow().as_ref().map(ComponentHandle::clone_strong) else {
                return;
            };
            let format = format_of(win.get_format_index());
            win.set_extension(format.extension().into());
            let stem = win.get_file_name().to_string();
            let error = match check_stem(&stem) {
                Err(e) => e,
                Ok(()) if !folder.is_dir() => "The folder does not exist.".to_string(),
                Ok(()) => String::new(),
            };
            let exists = error.is_empty() && target_path(&folder, &stem, format).exists();
            win.set_error(error.into());
            win.set_exists(exists);
            let lines: Vec<SharedString> = warnings(format, alpha, depth).into_iter().map(Into::into).collect();
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

    fn bind_events(win: &ImageExportDialog, window: &MainWindow) {
        win.on_changed(Self::revalidate);
        let dialog = win.as_weak();
        win.on_browse(move || {
            let current = PENDING.with(|p| p.borrow().as_ref().map(|p| p.folder.clone()));
            let mut picker = rfd::FileDialog::new().set_title("Save the picture in");
            if let Some(folder) = &current {
                picker = picker.set_directory(folder);
            }
            if let Some(folder) = picker.pick_folder() {
                tracing::info!(folder = %LogObscurer::path(&folder), "[ImageEdit: Export] Folder chosen");
                if let Some(win) = dialog.upgrade() {
                    win.set_folder(folder.display().to_string().into());
                }
                PENDING.with(|p| {
                    if let Some(p) = p.borrow_mut().as_mut() {
                        p.folder = folder;
                    }
                });
                Self::revalidate();
            }
        });
        let (weak, dialog) = (window.as_weak(), win.as_weak());
        win.on_save(move || {
            let (Some(window), Some(win)) = (weak.upgrade(), dialog.upgrade()) else {
                return;
            };
            Self::revalidate();
            if !win.get_error().is_empty() {
                return;
            }
            let Some(folder) = PENDING.with(|p| p.borrow().as_ref().map(|p| p.folder.clone())) else {
                return;
            };
            let format = format_of(win.get_format_index());
            let choice = ExportChoice {
                path: target_path(&folder, &win.get_file_name(), format),
                format,
                options: Self::options(&win),
                replace: win.get_exists(),
            };
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
