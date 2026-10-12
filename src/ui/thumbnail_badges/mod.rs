//! Thumbnail badges: small overlays drawn on top of media thumbnails in the grid
//! (tag indicator, rating, media type, video duration, ...).
//!
//! Architecture:
//! - `badge`     - `ThumbnailBadge` spec and builder API (content, orientation, slot, tone, color).
//! - `color`     - optional per-badge hex color override.
//! - `ordering`  - automatic / explicit slot ordering inside one badge area.
//! - `providers` - one function per badge type; add new badges here.
//! - `engine`    - pipeline glue: collect, group by corner, order, convert to Slint views.
//! - `bridge`    - registration of the Slint callback and settings -> UI configuration.
//!
//! Rust only decides *which* badges exist and in *which order*. Measuring, wrapping (CSS-like
//! flex-wrap), hiding and drawing are done in Slint (`ui/thumbnail_badge.slint`), which also holds
//! the single base design shared by all badges. No text widths are estimated in Rust.
//!
//! Badge areas: the left corners (`top-left`, `bottom-left`) form the primary, left-aligned area
//! and the right corners (`top-right`, `bottom-right`) the secondary, right-aligned area. Each
//! area gets half of the thumbnail width. Inside an area the order is automatic (provider
//! registration order) unless a badge requests a `slot`.

pub mod badge;
pub mod bridge;
pub mod color;
pub mod engine;
pub mod ordering;
pub mod providers;

pub use badge::{BadgeContent, ThumbnailBadge};
pub use bridge::ThumbnailBadgeBridge;
pub use color::{BadgeColor, BadgeColorError};
