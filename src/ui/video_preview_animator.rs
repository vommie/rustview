use crate::library::models::MediaId;
use crate::thumbnails::ThumbnailManager;
use super::models::rgb8_to_slint_image;
use slint::{Image, Timer};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Single Point of Truth (SPoT) constant for video hover preview frame interval (4 FPS / 250ms).
pub const DEFAULT_PREVIEW_INTERVAL_MS: u64 = 250;

/// Centralized, decoupled animation engine driving multi-frame video preview playback across GridView, TableView, and Conflict Dialogs.
pub struct VideoPreviewAnimator;

impl VideoPreviewAnimator {
    /// Loads and decodes multi-frame video preview buffers from PackStore into Slint Images.
    pub fn load_preview_images(
        thumbnails: &ThumbnailManager,
        media_id: MediaId,
    ) -> Option<Vec<Image>> {
        let frames = thumbnails.load_preview_frames_sync(media_id).ok()?;
        let images: Vec<Image> = frames
            .into_iter()
            .map(|(w, h, rgb)| rgb8_to_slint_image(&rgb, w, h))
            .collect();
        if images.is_empty() {
            None
        } else {
            Some(images)
        }
    }

    /// Starts a repeating Slint `Timer` cycling through the provided preview images at the standard 250ms interval.
    pub fn start_animation<F>(
        images: Vec<Image>,
        on_frame: F,
    ) -> Option<Timer>
    where
        F: Fn(Image) + 'static,
    {
        if images.is_empty() {
            return None;
        }
        let frame_idx = Rc::new(RefCell::new(0usize));
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(DEFAULT_PREVIEW_INTERVAL_MS),
            move || {
                let mut idx = frame_idx.borrow_mut();
                let cur = *idx % images.len();
                *idx += 1;
                on_frame(images[cur].clone());
            },
        );
        Some(timer)
    }
}
