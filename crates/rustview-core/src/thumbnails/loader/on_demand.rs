//! Thumbnails and previews of single media on demand (view requests, synchronous loads and regeneration).

use super::ThumbnailScheduler;
use crate::library::models::MediaId;
use crate::media::{MediaFormat, MediaType};
use crate::thumbnails::error::ThumbnailError;
use crate::thumbnails::gif::{AnimatedPreview, GifThumbnailer};
use crate::thumbnails::ram_cache::CachedThumbnail;
use crate::thumbnails::types::{ThumbnailConfig, VideoPreviewRecord};
use crate::util::cast::SaturatingAs;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl ThumbnailScheduler {
    /// The thumbnail if it is already in RAM (a reference-count bump, no I/O and no decode). The Browse view uses this on the UI
    /// thread; what is missing goes through [`Self::request_view_thumbnails`].
    pub fn cached_thumbnail(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        self.cache.get(media_id)
    }

    /// Asks the view loader for these thumbnails, most urgent first; earlier pending requests are dropped. Returns how many
    /// requests are pending.
    pub fn request_view_thumbnails(&self, ids: impl IntoIterator<Item = MediaId>) -> usize {
        self.view_loader.request(ids)
    }

    pub fn cancel_view_requests(&self) {
        self.view_loader.cancel_pending();
    }

    /// Blocking lookup in RAM, then in the pack store. For callers that are not on the UI thread.
    pub fn try_load_existing(&self, media_id: MediaId) -> Option<SharedPixelBuffer<Rgb8Pixel>> {
        if let Some(cached) = self.cache.get(media_id) {
            return Some(cached);
        }

        let location = {
            let rtxn = self.library.raw_db().env.read_txn().ok()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id).ok().flatten()?
        };

        if let Ok(compressed_bytes) = self.store.read(location)
            && let Ok((w, h, rgb8_data)) = self.generator.decompress(&compressed_bytes)
        {
            return Some(self.cache.insert_rgb(media_id, w, h, &rgb8_data).pixels);
        }

        None
    }

    /// Fast non-blocking lookup for cached preview frames in LMDB / `PackStore` (0.05ms).
    pub fn try_load_preview_frames(&self, media_id: MediaId) -> Option<Vec<(u32, u32, Vec<u8>)>> {
        let rtxn = self.library.raw_db().env.read_txn().ok()?;
        let p_rec = self
            .library
            .raw_db()
            .video_previews
            .get(&rtxn, &media_id)
            .ok()
            .flatten()?;

        let mut frames = Vec::with_capacity(p_rec.frame_locations.len());
        for loc in &p_rec.frame_locations {
            if let Ok(bytes) = self.store.read(*loc)
                && let Ok((w, h, rgb8)) = self.generator.decompress(&bytes)
            {
                frames.push((w, h, rgb8));
            }
        }

        if frames.is_empty() { None } else { Some(frames) }
    }

    /// Fast non-blocking lookup for cached animated preview in RAM (e.g. GIF).
    pub fn try_get_animated_preview(&self, media_id: MediaId) -> Option<Arc<AnimatedPreview>> {
        self.animated_cache.get(media_id)
    }

    /// Asynchronously extracts, scales, and caches animated GIF frames in a background worker thread.
    pub fn get_or_load_animated_preview_async<F>(
        &self,
        media_id: MediaId,
        path: PathBuf,
        config: &ThumbnailConfig,
        on_loaded: F,
    ) where
        F: FnOnce(Result<Arc<AnimatedPreview>, ThumbnailError>) + Send + 'static,
    {
        if let Some(cached) = self.animated_cache.get(media_id) {
            on_loaded(Ok(cached));
            return;
        }

        let cache = self.animated_cache.clone();
        let (max_bound, scaling_algorithm) = (config.max_bound, config.scaling_algorithm);
        self.pool.spawn(move || {
            let res = GifThumbnailer::extract_frames(&path, max_bound, scaling_algorithm).map(Arc::new);

            if let Ok(ref preview) = res {
                cache.insert(media_id, preview.clone());
            }

            on_loaded(res);
        });
    }

    /// Image or video as the library knows it (a file's content decides, not its extension); the extension only
    /// answers for files that are not in the library.
    pub fn is_video_media(&self, media_id: MediaId, file_path: &Path) -> bool {
        if let Ok(rtxn) = self.library.raw_db().env.read_txn()
            && let Ok(Some(record)) = self.library.raw_db().media.get(&rtxn, &media_id)
        {
            return record.media_type == MediaType::Video;
        }
        MediaFormat::from_path(file_path).media_type() == Some(MediaType::Video)
    }

    pub fn load_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        if let Some(item) = self.cache.get_thumbnail(media_id) {
            return Ok(item);
        }

        let location = {
            let rtxn = self.library.raw_db().env.read_txn()?;
            self.library.raw_db().thumbnails.get(&rtxn, &media_id)?
        };

        if let Some(loc) = location
            && let Ok(compressed) = self.store.read(loc)
            && let Ok((w, h, rgb8)) = self.generator.decompress(&compressed)
        {
            let item = self.cache.insert_rgb(media_id, w, h, &rgb8);
            return Ok(item);
        }

        let is_video = self.is_video_media(media_id, file_path);
        let frame = if is_video {
            self.generator
                .generate_video_thumbnails(file_path, config)?
                .primary_frame
        } else {
            self.generator
                .generate_image_thumbnail(file_path, config.max_bound, config.scaling_algorithm)?
        };

        let compressed = self.generator.compress(
            &frame.rgb_pixels,
            frame.width,
            frame.height,
            config.format,
            config.quality,
        )?;
        let location = self.store.append(&compressed)?;

        {
            let mut wtxn = self.library.raw_db().env.write_txn()?;
            self.library.raw_db().thumbnails.put(&mut wtxn, &media_id, &location)?;
            wtxn.commit()?;
        }

        let item = self
            .cache
            .insert_rgb(media_id, frame.width, frame.height, &frame.rgb_pixels);
        Ok(item)
    }

    /// Explicitly regenerates thumbnail & previews from physical file, overwriting `PackStore` and LMDB index.
    pub fn regenerate_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        let is_video = self.is_video_media(media_id, file_path);
        let (frame, video_preview_rec) = if is_video {
            let video_res = self.generator.generate_video_thumbnails(file_path, config)?;
            let primary = video_res.primary_frame;
            let mut preview_rec = None;
            if !video_res.preview_frames.is_empty() {
                let (p_format, p_quality) = if config.custom_preview_settings {
                    (config.preview_format, config.preview_quality)
                } else {
                    (config.format, config.quality)
                };

                let mut locations = Vec::with_capacity(video_res.preview_frames.len());
                for p_frame in &video_res.preview_frames {
                    if let Ok(p_bytes) =
                        self.generator
                            .compress(&p_frame.rgb_pixels, p_frame.width, p_frame.height, p_format, p_quality)
                        && let Ok(p_loc) = self.store.append(&p_bytes)
                    {
                        locations.push(p_loc);
                    }
                }
                if !locations.is_empty() {
                    preview_rec = Some(VideoPreviewRecord {
                        frame_count: locations.len().saturating_as::<u32>(),
                        frame_locations: locations,
                    });
                }
            }
            (primary, preview_rec)
        } else {
            let f = self
                .generator
                .generate_image_thumbnail(file_path, config.max_bound, config.scaling_algorithm)?;
            (f, None)
        };

        let compressed = self.generator.compress(
            &frame.rgb_pixels,
            frame.width,
            frame.height,
            config.format,
            config.quality,
        )?;
        let location = self.store.append(&compressed)?;

        {
            let mut wtxn = self.library.raw_db().env.write_txn()?;
            self.library.raw_db().thumbnails.put(&mut wtxn, &media_id, &location)?;
            if let Some(p_rec) = video_preview_rec {
                self.library.raw_db().video_previews.put(&mut wtxn, &media_id, &p_rec)?;
            } else {
                let _ = self.library.raw_db().video_previews.delete(&mut wtxn, &media_id);
            }
            wtxn.commit()?;
        }

        let item = self
            .cache
            .insert_rgb(media_id, frame.width, frame.height, &frame.rgb_pixels);

        Ok(item)
    }

    /// Explicitly regenerates thumbnail & previews with an overridden video offset percentage, overwriting `PackStore` and LMDB index.
    pub fn regenerate_with_offset_sync(
        &self,
        media_id: MediaId,
        file_path: &Path,
        config: &ThumbnailConfig,
        offset_percent: u8,
    ) -> Result<CachedThumbnail, ThumbnailError> {
        let custom_cfg = config.with_video_offset_percent(offset_percent);
        self.regenerate_sync(media_id, file_path, &custom_cfg)
    }
}
