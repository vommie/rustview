use crate::ui::models::rgb8_to_slint_image;
use rustview_core::library::models::MediaId;
use rustview_core::thumbnails::{AnimatedPreview, ThumbnailManager};
use slint::{Image, Timer};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Single Point of Truth (SPoT) baseline constant for video hover preview frame interval (250ms).
pub const DEFAULT_PREVIEW_INTERVAL_MS: u64 = 250;

/// Individual timed frame holding the pre-rendered image and its duration in milliseconds.
#[derive(Debug, Clone)]
pub struct TimedFrame {
    pub image: Image,
    pub delay_ms: u32,
}

/// Centralized animation engine driving multi-frame video previews and faithful animated GIF playback.
pub struct VideoPreviewAnimator;

impl VideoPreviewAnimator {
    /// Loads and decodes multi-frame video preview buffers from PackStore / LMDB into Slint Images.
    /// Fast non-blocking lookup (< 0.05ms): NEVER blocks the GUI event loop.
    pub fn load_preview_images(thumbnails: &ThumbnailManager, media_id: MediaId) -> Option<Vec<Image>> {
        let frames = thumbnails.try_load_preview_frames(media_id)?;
        let images: Vec<Image> = frames
            .into_iter()
            .map(|(w, h, rgb)| rgb8_to_slint_image(&rgb, w, h))
            .collect();
        if images.is_empty() { None } else { Some(images) }
    }

    /// Converts an `AnimatedPreview` (e.g. from an animated GIF) into a collection of `TimedFrame`s.
    pub fn convert_animated_preview_to_timed_frames(preview: &AnimatedPreview) -> Vec<TimedFrame> {
        preview
            .frames
            .iter()
            .map(|f| TimedFrame {
                image: rgb8_to_slint_image(&f.rgb_pixels, f.width, f.height),
                delay_ms: f.delay_ms,
            })
            .collect()
    }

    /// Starts a repeating Slint `Timer` cycling through video preview frames with a uniform cadence.
    pub fn start_animation<F>(images: Vec<Image>, on_frame: F) -> Option<Timer>
    where
        F: Fn(Image) + 'static,
    {
        if images.is_empty() {
            return None;
        }

        let num_frames = images.len();
        // Dynamically compute interval: target ~1.5 - 2.0 second full loop, clamped between 120ms and 250ms per frame
        let interval_ms = ((1800 / num_frames.max(1)) as u32).clamp(120, 250);

        let timed_frames: Vec<TimedFrame> = images
            .into_iter()
            .map(|img| TimedFrame {
                image: img,
                delay_ms: interval_ms,
            })
            .collect();

        Self::start_variable_delay_animation(timed_frames, on_frame)
    }

    /// Starts a high-precision, drift-free playback timer honoring individual frame delays (faithful GIF specification).
    pub fn start_variable_delay_animation<F>(frames: Vec<TimedFrame>, on_frame: F) -> Option<Timer>
    where
        F: Fn(Image) + 'static,
    {
        if frames.is_empty() {
            return None;
        }

        if frames.len() == 1 {
            on_frame(frames[0].image.clone());
            return None;
        }

        // 1. Calculate cumulative timeline offsets [0, d0, d0+d1, d0+d1+d2, ...]
        let mut cumulative_offsets = Vec::with_capacity(frames.len() + 1);
        let mut total_duration_ms = 0u64;
        cumulative_offsets.push(0u64);

        for f in &frames {
            let d = (f.delay_ms as u64).max(10);
            total_duration_ms += d;
            cumulative_offsets.push(total_duration_ms);
        }

        if total_duration_ms == 0 {
            total_duration_ms = (frames.len() as u64) * 100;
        }

        let start_time = Instant::now();
        let last_emitted_index = Rc::new(RefCell::new(usize::MAX));
        let frames_rc = Rc::new(frames);
        let offsets_rc = Rc::new(cumulative_offsets);
        let on_frame_rc = Rc::new(on_frame);

        // Emit initial frame 0 immediately
        on_frame_rc(frames_rc[0].image.clone());
        *last_emitted_index.borrow_mut() = 0;

        let timer = Timer::default();
        let timer_interval = Duration::from_millis(15); // ~60 FPS sampling for exact sub-frame accuracy without timer drift

        let frames_c = frames_rc;
        let offsets_c = offsets_rc;
        let on_frame_c = on_frame_rc;
        let last_idx_c = last_emitted_index;

        timer.start(slint::TimerMode::Repeated, timer_interval, move || {
            let elapsed_ms = (start_time.elapsed().as_millis() as u64) % total_duration_ms;

            // Search for the frame that contains the current timeline timestamp
            let mut current_frame_idx = 0;
            for i in 0..frames_c.len() {
                if elapsed_ms >= offsets_c[i] && elapsed_ms < offsets_c[i + 1] {
                    current_frame_idx = i;
                    break;
                }
            }

            let mut last = last_idx_c.borrow_mut();
            if *last != current_frame_idx {
                *last = current_frame_idx;
                on_frame_c(frames_c[current_frame_idx].image.clone());
            }
        });

        Some(timer)
    }
}
