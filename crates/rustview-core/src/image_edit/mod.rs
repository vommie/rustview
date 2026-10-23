//! Editing pictures (no user interface): the crop tool, the list of edits of a picture, deciding how a picture is saved, writing
//! it without ever damaging the original, and checking that the file on disk is still the one that was edited.
//!
//! Everything here is pure or works on explicit files the user asked to save; nothing in this module touches the drive on its own.

pub mod conflict;
pub mod crop_tool;
pub mod encode;
pub mod exif_patch;
pub mod formats;
pub mod geometry;
pub mod indexed;
pub mod metadata;
pub mod options;
pub mod probe;
pub mod ratio;
pub mod render;
pub mod save;
pub mod stack;
pub mod writer;

#[cfg(test)]
mod roundtrip_tests;
