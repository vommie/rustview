pub mod fallback_view;
pub mod inspector;
pub mod open_with;
pub mod thumbnails;
pub mod video_preview;

use rustview_core::library::models::{MediaId, MediaRecord};
use rustview_core::query::{ListLayout, MediaStore};
use rustview_core::thumbnails::{
    ThumbnailLocation, VideoPreviewRecord,
};
use slint::Timer;
use std::cell::RefCell;

mod query_callbacks;
mod virtual_folder;
mod view;
mod metadata_refresh;
mod media_refresh;
mod records;
mod reload;

thread_local! {
    static ACTIVE_PREVIEW_TIMER: RefCell<Option<Timer>> = const { RefCell::new(None) };
    static ACTIVE_HOVER_MEDIA_ID: RefCell<Option<u64>> = const { RefCell::new(None) };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MediaReloadOptions {
    pub start_pipeline: bool,
    pub reload_lmdb: bool,
    pub anchor_index: Option<usize>,
}

impl MediaReloadOptions {
    pub fn new(start_pipeline: bool, reload_lmdb: bool) -> Self {
        Self {
            start_pipeline,
            reload_lmdb,
            anchor_index: None,
        }
    }

    pub fn with_anchor(start_pipeline: bool, reload_lmdb: bool, anchor_index: usize) -> Self {
        Self {
            start_pipeline,
            reload_lmdb,
            anchor_index: Some(anchor_index),
        }
    }
}

struct ProcessedRefreshedItem {
    media_id: MediaId,
    old_hash: rustview_core::media::FileHash,
    new_hash: rustview_core::media::FileHash,
    updated_record: MediaRecord,
    primary_loc: Option<ThumbnailLocation>,
    preview_rec: Option<VideoPreviewRecord>,
    primary_pixels: Vec<u8>,
    primary_width: u32,
    primary_height: u32,
    tags_count: usize,
}

struct RefreshedMetadataOnlyItem {
    media_id: MediaId,
    updated_record: MediaRecord,
    tags_count: usize,
}

pub struct BridgeMedia;

impl BridgeMedia {

}

/// A finished layout waiting to be committed; `store` carries freshly loaded records (folder switch) built off the UI thread.
pub struct PendingList {
    pub store: Option<MediaStore>,
    pub layout: ListLayout,
    pub query: String,
}
