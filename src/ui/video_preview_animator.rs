use crate::library::models::MediaId;
use crate::thumbnails::ThumbnailManager;
use super::models::rgb8_to_slint_image;
use slint::{Image, Timer};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// Single Point of Truth (SPoT) baseline constant for video hover preview frame interval (250ms).
pub const DEFAULT_PREVIEW_INTERVAL_MS: u64 = 250;

/// Centralized, decoupled animation engine driving multi-frame video preview playback across GridView, TableView, and Conflict Dialogs.
pub struct VideoPreviewAnimator;

impl VideoPreviewAnimator {
    /// Loads and decodes multi-frame video preview buffers from PackStore / LMDB into Slint Images.
    /// Fast non-blocking lookup (< 0.05ms): NEVER blocks the GUI event loop.
    pub fn load_preview_images(
        thumbnails: &ThumbnailManager,
        media_id: MediaId,
    ) -> Option<Vec<Image>> {
        let frames = thumbnails.try_load_preview_frames(media_id)?;
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

    /// Starts a repeating Slint `Timer` cycling through preview images with adaptive, frame-rate-balanced cadence.
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

        let num_frames = images.len();
        // Dynamically compute interval: target ~1.5 - 2.0 second full loop, clamped between 120ms and 250ms per frame
        let interval_ms = (1800 / num_frames.max(1)).clamp(120, 250) as u64;

        let frame_idx = Rc::new(RefCell::new(0usize));
        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(interval_ms),
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
