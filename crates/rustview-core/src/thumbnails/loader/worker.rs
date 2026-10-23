//! The generation worker loop and its database batching.

use super::{BatchedDbWriteItem, PreparedTarget, ProbedMetadata, ReadyThumbnailItem, ThumbnailScheduler};
use crate::telemetry::benchmark::ThumbnailPipelineBenchmark;
use crate::thumbnails::animated_frames;
use crate::thumbnails::types::{ThumbnailConfig, VideoPreviewRecord};
use crate::util::cast::SaturatingAs;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

impl ThumbnailScheduler {
    pub(super) fn worker_generation_loop(
        &self,
        pipeline_id: usize,
        folder_id: u64,
        config: &ThumbnailConfig,
        benchmark: &Arc<ThumbnailPipelineBenchmark>,
        t_phase2_start: &Arc<Instant>,
    ) {
        let mut batched_db_writes: Vec<BatchedDbWriteItem> = Vec::with_capacity(config.batch_commit_size);

        while !self.is_pipeline_stale(pipeline_id) {
            let target = {
                let mut q = self.generation_queue.lock();
                q.pop_front()
            };

            let Some(target) = target else { break };

            if self.is_pipeline_stale(pipeline_id) {
                break;
            }

            self.tracker.increment_active_workers();

            let is_video = target.is_video;

            let mut is_corrupted = false;

            if is_video {
                let t_vid = Instant::now();
                match self.generator.generate_video_thumbnails(&target.file_path, config) {
                    Ok(video_res) => {
                        benchmark.record_video_extract(t_vid.elapsed().as_micros().saturating_as::<u64>());

                        if self.is_pipeline_stale(pipeline_id) {
                            self.tracker.decrement_active_workers();
                            break;
                        }

                        let primary = video_res.primary_frame;
                        let t_comp = Instant::now();

                        if let Ok(compressed_primary) = self.generator.compress(
                            &primary.rgb_pixels,
                            primary.width,
                            primary.height,
                            config.format,
                            config.quality,
                        ) {
                            benchmark.record_compress(
                                compressed_primary.len() as u64,
                                t_comp.elapsed().as_micros().saturating_as::<u64>(),
                            );

                            let t_app = Instant::now();
                            if let Ok(primary_loc) = self.store.append(&compressed_primary) {
                                benchmark.record_pack_append(
                                    compressed_primary.len() as u64,
                                    t_app.elapsed().as_micros().saturating_as::<u64>(),
                                );

                                let mut preview_rec = None;
                                if !video_res.preview_frames.is_empty() {
                                    let (p_format, p_quality) = if config.custom_preview_settings {
                                        (config.preview_format, config.preview_quality)
                                    } else {
                                        (config.format, config.quality)
                                    };

                                    let mut locations = Vec::with_capacity(video_res.preview_frames.len());
                                    for p_frame in &video_res.preview_frames {
                                        if let Ok(p_bytes) = self.generator.compress(
                                            &p_frame.rgb_pixels,
                                            p_frame.width,
                                            p_frame.height,
                                            p_format,
                                            p_quality,
                                        ) && let Ok(p_loc) = self.store.append(&p_bytes)
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

                                if !self.is_pipeline_stale(pipeline_id) {
                                    let meta_update = if video_res.orig_width > 0 && video_res.orig_height > 0 {
                                        Some(ProbedMetadata {
                                            width: video_res.orig_width,
                                            height: video_res.orig_height,
                                            duration_ms: video_res.duration_ms,
                                        })
                                    } else {
                                        None
                                    };

                                    batched_db_writes.push(BatchedDbWriteItem {
                                        media_id: target.media_id,
                                        location: Some(primary_loc),
                                        preview_record: preview_rec,
                                        probed_metadata: meta_update,
                                        is_corrupted: false,
                                    });

                                    let pixel_buffer = self
                                        .cache
                                        .insert_rgb(target.media_id, primary.width, primary.height, &primary.rgb_pixels)
                                        .pixels;

                                    let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                                        pipeline_id,
                                        folder_id,
                                        media_id: target.media_id,
                                        width: primary.width,
                                        height: primary.height,
                                        pixel_buffer: Some(pixel_buffer),
                                        orig_width: video_res.orig_width,
                                        orig_height: video_res.orig_height,
                                        duration_ms: video_res.duration_ms,
                                        is_corrupted: false,
                                        is_animated: false,
                                    });
                                    benchmark.record_queue_depth(depth);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(media_id = target.media_id, error = %e, "[ThumbnailPipeline] Video thumbnail generation failed, marking file as corrupted");
                        is_corrupted = true;
                    }
                }
            } else {
                let t_img = Instant::now();
                match self.generator.generate_image_thumbnail(
                    &target.file_path,
                    config.max_bound,
                    config.scaling_algorithm,
                ) {
                    Ok(frame) => {
                        benchmark.record_image_decode_resize(t_img.elapsed().as_micros().saturating_as::<u64>());

                        let t_comp = Instant::now();
                        if let Ok(compressed_bytes) = self.generator.compress(
                            &frame.rgb_pixels,
                            frame.width,
                            frame.height,
                            config.format,
                            config.quality,
                        ) {
                            benchmark.record_compress(
                                compressed_bytes.len() as u64,
                                t_comp.elapsed().as_micros().saturating_as::<u64>(),
                            );

                            let t_app = Instant::now();
                            if let Ok(location) = self.store.append(&compressed_bytes) {
                                benchmark.record_pack_append(
                                    compressed_bytes.len() as u64,
                                    t_app.elapsed().as_micros().saturating_as::<u64>(),
                                );

                                if !self.is_pipeline_stale(pipeline_id) {
                                    let meta_update = if frame.orig_width > 0 && frame.orig_height > 0 {
                                        Some(ProbedMetadata {
                                            width: frame.orig_width,
                                            height: frame.orig_height,
                                            duration_ms: 0,
                                        })
                                    } else {
                                        None
                                    };

                                    let preview_record = if target.animatable && config.generate_video_previews {
                                        self.animated_preview_record(&target, config)
                                    } else {
                                        None
                                    };
                                    let is_animated = preview_record.as_ref().is_some_and(|r| r.frame_count > 1);

                                    batched_db_writes.push(BatchedDbWriteItem {
                                        media_id: target.media_id,
                                        location: Some(location),
                                        preview_record,
                                        probed_metadata: meta_update,
                                        is_corrupted: false,
                                    });

                                    let pixel_buffer = self
                                        .cache
                                        .insert_rgb(target.media_id, frame.width, frame.height, &frame.rgb_pixels)
                                        .pixels;

                                    let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                                        pipeline_id,
                                        folder_id,
                                        media_id: target.media_id,
                                        width: frame.width,
                                        height: frame.height,
                                        pixel_buffer: Some(pixel_buffer),
                                        orig_width: frame.orig_width,
                                        orig_height: frame.orig_height,
                                        duration_ms: 0,
                                        is_corrupted: false,
                                        is_animated,
                                    });
                                    benchmark.record_queue_depth(depth);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(media_id = target.media_id, error = %e, "[ThumbnailPipeline] Image thumbnail generation failed, marking file as corrupted");
                        is_corrupted = true;
                    }
                }
            }

            if is_corrupted && !self.is_pipeline_stale(pipeline_id) {
                batched_db_writes.push(BatchedDbWriteItem {
                    media_id: target.media_id,
                    location: None,
                    preview_record: None,
                    probed_metadata: None,
                    is_corrupted: true,
                });

                let depth = self.ready_drain_queue.push(ReadyThumbnailItem {
                    pipeline_id,
                    folder_id,
                    media_id: target.media_id,
                    width: 0,
                    height: 0,
                    pixel_buffer: None,
                    orig_width: 0,
                    orig_height: 0,
                    duration_ms: 0,
                    is_corrupted: true,
                    is_animated: false,
                });
                benchmark.record_queue_depth(depth);
            }

            if batched_db_writes.len() >= config.batch_commit_size {
                self.flush_db_batch(&mut batched_db_writes, benchmark);
            }

            self.tracker.increment_generate_done();
            self.tracker.decrement_active_workers();
        }

        if !batched_db_writes.is_empty() {
            self.flush_db_batch(&mut batched_db_writes, benchmark);
        }

        benchmark.set_generate_duration(t_phase2_start.elapsed().as_micros().saturating_as::<u64>());
    }

    /// The hover preview of an animated picture, stored like the preview of a video. `Some` with no frames marks a picture that was
    /// checked and is not animated; `None` when the file could not be read (nothing is recorded, a later refresh tries again).
    pub(super) fn animated_preview_record(
        &self,
        target: &PreparedTarget,
        config: &ThumbnailConfig,
    ) -> Option<VideoPreviewRecord> {
        let count = (config.preview_frame_count as usize).max(2);
        match animated_frames::sample(&target.file_path, config.max_bound, config.scaling_algorithm, count) {
            Ok(Some(frames)) => {
                let (format, quality) = if config.custom_preview_settings {
                    (config.preview_format, config.preview_quality)
                } else {
                    (config.format, config.quality)
                };
                let locations: Vec<_> = frames
                    .iter()
                    .filter_map(|f| {
                        self.generator
                            .compress(&f.rgb_pixels, f.width, f.height, format, quality)
                            .ok()
                    })
                    .filter_map(|bytes| self.store.append(&bytes).ok())
                    .collect();
                let frame_count = locations.len().saturating_as::<u32>();
                (frame_count > 1).then_some(VideoPreviewRecord {
                    frame_count,
                    frame_locations: locations,
                })
            }
            Ok(None) => Some(VideoPreviewRecord {
                frame_count: 0,
                frame_locations: Vec::new(),
            }),
            Err(e) => {
                tracing::debug!(media_id = target.media_id, error = %e, "[ThumbnailPipeline] Animation check failed; the picture stays unchecked");
                None
            }
        }
    }

    pub(super) fn flush_db_batch(&self, batch: &mut Vec<BatchedDbWriteItem>, benchmark: &ThumbnailPipelineBenchmark) {
        if batch.is_empty() {
            return;
        }

        let t_db = Instant::now();
        if let Ok(mut wtxn) = self.library.raw_db().env.write_txn() {
            let count = batch.len();
            for item in batch.drain(..) {
                if let Some(loc) = item.location {
                    let _ = self.library.raw_db().thumbnails.put(&mut wtxn, &item.media_id, &loc);
                }
                if let Some(p_rec) = item.preview_record {
                    let _ = self
                        .library
                        .raw_db()
                        .video_previews
                        .put(&mut wtxn, &item.media_id, &p_rec);
                }

                if let Ok(Some(mut rec)) = self.library.raw_db().media.get(&wtxn, &item.media_id) {
                    let mut changed = false;
                    if rec.is_corrupted != item.is_corrupted {
                        rec.is_corrupted = item.is_corrupted;
                        changed = true;
                    }
                    if let Some(meta) = item.probed_metadata {
                        if rec.width == 0 && meta.width > 0 {
                            rec.width = meta.width;
                            changed = true;
                        }
                        if rec.height == 0 && meta.height > 0 {
                            rec.height = meta.height;
                            changed = true;
                        }
                        if rec.duration_ms == 0 && meta.duration_ms > 0 {
                            rec.duration_ms = meta.duration_ms;
                            changed = true;
                        }
                    }
                    if changed {
                        let _ = self.library.raw_db().media.put(&mut wtxn, &item.media_id, &rec);
                    }
                }
            }
            let _ = wtxn.commit();
            benchmark.record_lmdb_commit(count, t_db.elapsed().as_micros().saturating_as::<u64>());
        }
    }

    pub(super) fn is_pipeline_stale(&self, pipeline_id: usize) -> bool {
        self.is_cancelled.load(Ordering::Acquire) || self.active_pipeline_id.load(Ordering::Acquire) != pipeline_id
    }

    pub(super) fn clone_handle(&self) -> Self {
        Self {
            library: self.library.clone(),
            cache: self.cache.clone(),
            animated_cache: self.animated_cache.clone(),
            store: self.store.clone(),
            generator: self.generator.clone(),
            pool: self.pool.clone(),
            view_loader: self.view_loader.clone(),
            tracker: self.tracker.clone(),
            ready_drain_queue: self.ready_drain_queue.clone(),
            generation_queue: self.generation_queue.clone(),
            active_pipeline_id: self.active_pipeline_id.clone(),
            is_cancelled: self.is_cancelled.clone(),
            active_benchmark: self.active_benchmark.clone(),
            active_in_flight: self.active_in_flight.clone(),
            current_config: self.current_config.clone(),
        }
    }
}
