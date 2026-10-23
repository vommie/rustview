//! Settings > Image Edit: fills the page from the settings and reads it back, and keeps the list of crop ratios the user edits
//! while the dialog is open (the chips, adding with validation, removing, the defaults).

use crate::ui::{RatioChipView, SettingsWindow};
use rustview_core::image_edit::options::{CropGrid, OverwriteMode};
use rustview_core::image_edit::ratio::{Orientation, RatioPreset, RatioSet};
use rustview_core::settings::schema::{DEFAULT_CROP_ASPECT_RATIOS, ImageEditSettings, MAX_CROP_ASPECT_RATIOS};
use rustview_core::util::cast::SaturatingAs;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

use crate::ui::image_edit::export_plan::{png_index, png_of, subsampling_index, subsampling_of, tiff_index, tiff_of};

thread_local! {
    /// The ratios as the page shows them (saved with the dialog).
    static RATIOS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn chips(entries: &[String]) -> Vec<RatioChipView> {
    RatioSet::parse(entries)
        .0
        .presets
        .iter()
        .map(|p| RatioChipView {
            label: p.label.as_str().into(),
            kind: match p.ratio.orientation() {
                Orientation::Landscape => 0,
                Orientation::Portrait => 1,
                Orientation::Square => 2,
            },
        })
        .collect()
}

fn show_ratios(window: &SettingsWindow) {
    let entries = RATIOS.with(|r| r.borrow().clone());
    window.set_image_edit_ratios(ModelRc::from(Rc::new(VecModel::from(chips(&entries)))));
}

const GRIDS: [CropGrid; 3] = [CropGrid::Dragging, CropGrid::Always, CropGrid::Never];
const MODES: [OverwriteMode; 3] = [OverwriteMode::Safe, OverwriteMode::Trash, OverwriteMode::Direct];

fn index_of<T: PartialEq>(all: &[T], value: &T) -> i32 {
    all.iter().position(|v| v == value).unwrap_or(0).saturating_as::<i32>()
}

pub fn populate(window: &SettingsWindow, settings: &ImageEditSettings) {
    RATIOS.with(|r| r.borrow_mut().clone_from(&settings.crop.aspect_ratios));
    show_ratios(window);
    window.set_image_edit_ratio_input("".into());
    window.set_image_edit_ratio_error("".into());
    window.set_image_edit_crop_grid_idx(index_of(&GRIDS, &settings.crop.grid_mode()));
    window.set_image_edit_align_jpeg(settings.crop.align_jpeg_to_lossless_grid);
    window.set_image_edit_overwrite_mode_idx(index_of(&MODES, &settings.save.mode()));
    let o = settings.presets.encode_options();
    window.set_image_edit_jpeg_quality(i32::from(o.jpeg_quality));
    window.set_image_edit_jpeg_subsampling_idx(subsampling_index(o.jpeg_subsampling));
    window.set_image_edit_jpeg_progressive(o.jpeg_progressive);
    window.set_image_edit_png_compression_idx(png_index(o.png_compression));
    window.set_image_edit_webp_lossless(o.webp_lossless);
    window.set_image_edit_webp_quality(i32::from(o.webp_quality));
    window.set_image_edit_avif_quality(i32::from(o.avif_quality));
    window.set_image_edit_avif_speed(i32::from(o.avif_speed));
    window.set_image_edit_tiff_compression_idx(tiff_index(o.tiff_compression));
}

pub fn save(window: &SettingsWindow, settings: &mut ImageEditSettings) {
    settings.crop.aspect_ratios = RATIOS.with(|r| r.borrow().clone());
    let pick = |all: &[&'static str], index: i32| {
        usize::try_from(index)
            .ok()
            .and_then(|i| all.get(i).copied())
            .unwrap_or(all[0])
            .to_string()
    };
    settings.crop.grid = pick(&GRIDS.map(CropGrid::as_str), window.get_image_edit_crop_grid_idx());
    settings.crop.align_jpeg_to_lossless_grid = window.get_image_edit_align_jpeg();
    settings.save.overwrite_mode = pick(
        &MODES.map(OverwriteMode::as_str),
        window.get_image_edit_overwrite_mode_idx(),
    );
    let quality = |v: i32| v.clamp(1, 100).saturating_as::<u8>();
    let p = &mut settings.presets;
    p.jpeg_quality = quality(window.get_image_edit_jpeg_quality());
    p.jpeg_subsampling = subsampling_of(window.get_image_edit_jpeg_subsampling_idx())
        .as_str()
        .to_string();
    p.jpeg_progressive = window.get_image_edit_jpeg_progressive();
    p.png_compression = png_of(window.get_image_edit_png_compression_idx()).as_str().to_string();
    p.webp_lossless = window.get_image_edit_webp_lossless();
    p.webp_quality = quality(window.get_image_edit_webp_quality());
    p.avif_quality = quality(window.get_image_edit_avif_quality());
    p.avif_speed = window.get_image_edit_avif_speed().clamp(1, 10).saturating_as::<u8>();
    p.tiff_compression = tiff_of(window.get_image_edit_tiff_compression_idx())
        .as_str()
        .to_string();
    settings.sanitize();
}

/// Adds `text` to the ratios, or says why it cannot be added.
fn add(entries: &mut Vec<String>, text: &str) -> Result<(), String> {
    let preset = RatioPreset::parse(text).map_err(|e| e.to_string())?;
    let (set, _) = RatioSet::parse(entries);
    if let Some(same) = set.presets.iter().find(|p| p.ratio == preset.ratio) {
        return Err(format!(
            "'{}' is already in the list as '{}'.",
            preset.label, same.label
        ));
    }
    if entries.len() >= MAX_CROP_ASPECT_RATIOS {
        return Err(format!("At most {MAX_CROP_ASPECT_RATIOS} ratios can be defined."));
    }
    entries.push(preset.label);
    Ok(())
}

pub fn register_callbacks(window: &SettingsWindow) {
    let weak = window.as_weak();
    window.on_image_edit_add_ratio(move |text| {
        let Some(window) = weak.upgrade() else { return };
        let result = RATIOS.with(|r| add(&mut r.borrow_mut(), &text));
        match result {
            Ok(()) => {
                window.set_image_edit_ratio_input("".into());
                window.set_image_edit_ratio_error("".into());
                show_ratios(&window);
            }
            Err(e) => window.set_image_edit_ratio_error(e.into()),
        }
    });
    let weak = window.as_weak();
    window.on_image_edit_remove_ratio(move |index| {
        let Some(window) = weak.upgrade() else { return };
        RATIOS.with(|r| {
            // The chips show the valid entries only; remove the one at that place among them.
            let mut entries = r.borrow_mut();
            let labels: Vec<String> = RatioSet::parse(&entries)
                .0
                .presets
                .iter()
                .map(|p| p.label.clone())
                .collect();
            if let Some(label) = usize::try_from(index).ok().and_then(|i| labels.get(i)) {
                entries.retain(|e| RatioPreset::parse(e).map(|p| p.label) != Ok(label.clone()));
            }
        });
        window.set_image_edit_ratio_error("".into());
        show_ratios(&window);
    });
    let weak = window.as_weak();
    window.on_image_edit_reset_ratios(move || {
        let Some(window) = weak.upgrade() else { return };
        RATIOS.with(|r| *r.borrow_mut() = DEFAULT_CROP_ASPECT_RATIOS.iter().map(ToString::to_string).collect());
        window.set_image_edit_ratio_error("".into());
        show_ratios(&window);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios_are_added_once_and_invalid_ones_are_explained() {
        let mut entries = vec!["16:9".to_string()];
        assert!(add(&mut entries, "4:5").is_ok());
        assert_eq!(
            add(&mut entries, "32:18"),
            Err("'32:18' is already in the list as '16:9'.".to_string())
        );
        assert!(add(&mut entries, "abc").is_err());
        assert_eq!(entries, ["16:9", "4:5"]);
        assert_eq!(chips(&entries).iter().map(|c| c.kind).collect::<Vec<_>>(), [0, 1]);
    }
}
