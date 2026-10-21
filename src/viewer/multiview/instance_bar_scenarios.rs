//! Scenarios for the media bars that stay open (zoom slider) while media come and go in a row or in a 2x2 grid.
//! The bars must always belong to their tile (by key), sit in the canvas of that tile and never outlive it.

use super::events::{MultiviewEventSink, PlayerEvent};
use super::follow::TileKey;
use super::instance::{MultiviewInstance, SharedInstance, WeakInstance};
use super::instance_host::HostSurface;
use super::layout::Rect;
use super::media_actions::{BarClick, MediaActionId};
use super::model::TileKind;
use super::tile::TileSeed;
use super::view::MediaBarTarget;
use crate::viewer::zoom::prefs as zoom_prefs;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::sync::Arc;

struct NullSink;

impl MultiviewEventSink for NullSink {
    fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
}

fn seed(id: u64, aspect: f32) -> TileSeed {
    let mut seed = TileSeed::new(id, TileKind::Image, aspect, PathBuf::from(format!("/x/{id}.jpg")), format!("{id}.jpg"));
    seed.pixels = Some(SharedPixelBuffer::<Rgb8Pixel>::new(1600, 900));
    seed.orig_size = Some((4000, 2250));
    seed
}

fn instance(count: u64, aspect: f32) -> SharedInstance {
    let mut settings = crate::settings::schema::FullscreenSettings::default();
    settings.zoom.smooth = false;
    settings.zoom.out_percent = 900;
    zoom_prefs::apply(&settings);
    let seeds = (1..=count).map(|id| seed(id, aspect)).collect();
    let instance = MultiviewInstance::create(Arc::new(NullSink), seeds);
    instance.lock().set_host(&HostSurface::fullscreen(1, 1600, 900, 1.0));
    instance
}

fn keys(instance: &SharedInstance) -> Vec<TileKey> {
    instance.lock().tiles.iter().map(|t| t.key).collect()
}

fn open_slider(instance: &SharedInstance, tile: usize) {
    instance.lock().toggle_zoom_slider(tile);
}

/// What must hold after every change.
fn check(instance: &SharedInstance, open: &[TileKey], what: &str) {
    let mut guard = instance.lock();
    guard.poll_zoom();
    let present = keys_of(&guard);
    // Sliders belong to tiles that exist, and only to them.
    let expected: Vec<TileKey> = open.iter().copied().filter(|k| present.contains(k)).collect();
    let mut have = guard.zoom_slider.clone();
    have.sort_unstable();
    let mut want = expected.clone();
    want.sort_unstable();
    assert_eq!(have, want, "{what}: sliders of the tiles that are there");
    let targets: Vec<MediaBarTarget> = guard.media_bar_targets(&[]);
    let mut target_keys: Vec<TileKey> = targets.iter().map(|t| t.key).collect();
    target_keys.sort_unstable();
    assert_eq!(target_keys, want, "{what}: one bar per open tile, none for tiles that left");
    for target in &targets {
        let index = present.iter().position(|k| *k == target.key).expect("the bar's tile exists");
        assert_eq!(target.tile, index, "{what}: the bar knows where its tile is now");
        let room: Rect = guard.host_room(index).expect("room");
        assert_eq!(target.rect, room, "{what}: the bar sits in the canvas of its tile");
        assert!(target.forced, "{what}: an open slider keeps its bar");
    }
    let bubbles = guard.zoom_bubbles();
    assert_eq!(bubbles.len(), want.len(), "{what}: one slider bubble per open tile");
    for bubble in bubbles {
        assert_eq!(present[bubble.tile], bubble.key, "{what}: the bubble's index is its tile's index");
    }
    // The rooms of the tiles do not overlap (so the bars of different tiles never share a corner).
    for a in 0..present.len() {
        for b in a + 1..present.len() {
            let (ra, rb) = (guard.host_room(a).unwrap(), guard.host_room(b).unwrap());
            let apart = ra.x + ra.w <= rb.x || rb.x + rb.w <= ra.x || ra.y + ra.h <= rb.y || rb.y + rb.h <= ra.y;
            assert!(apart, "{what}: rooms {a} and {b} overlap: {ra:?} {rb:?}");
        }
    }
}

fn keys_of(guard: &MultiviewInstance) -> Vec<TileKey> {
    guard.tiles.iter().map(|t| t.key).collect()
}

fn remove(instance: &SharedInstance, index: usize) {
    let key = instance.lock().tiles[index].key;
    instance.lock().perform_click(key, BarClick::Action(MediaActionId::Remove));
}

fn duplicate(instance: &SharedInstance, index: usize) {
    let key = instance.lock().tiles[index].key;
    assert!(instance.lock().duplicate_tile(key));
}

#[test]
fn a_row_keeps_its_open_bars_with_their_tiles_when_media_leave() {
    let instance = instance(4, 4.0 / 3.0);
    let all = keys(&instance);
    for tile in 0..4 {
        open_slider(&instance, tile);
    }
    check(&instance, &all, "four open");
    remove(&instance, 1);
    check(&instance, &all, "after removing the second");
    remove(&instance, 0);
    check(&instance, &all, "after removing the first");
    remove(&instance, 1);
    check(&instance, &all, "after removing the last of three");
    // One tile left: it cannot leave (the button closes the view), its bar stays.
    assert_eq!(instance.lock().tiles.len(), 1);
    check(&instance, &all, "one tile");
}

#[test]
fn a_row_keeps_its_open_bars_with_their_tiles_when_media_join() {
    let instance = instance(1, 4.0 / 3.0);
    let first = keys(&instance);
    open_slider(&instance, 0);
    check(&instance, &first, "one open");
    duplicate(&instance, 0);
    check(&instance, &first, "a duplicate joined next to it");
    // The duplicate takes the slot next to its source: the open tile may have moved; its bar follows.
    let after = keys(&instance);
    assert!(after.contains(&first[0]));
    open_slider(&instance, after.iter().position(|k| *k != first[0]).unwrap());
    let both = keys(&instance);
    check(&instance, &both, "both open");
    duplicate(&instance, 0);
    duplicate(&instance, 0);
    check(&instance, &both, "four tiles, two bars open");
    assert_eq!(instance.lock().tiles.len(), 4);
}

#[test]
fn a_two_by_two_grid_keeps_its_bars_across_the_change_to_a_row_and_back() {
    let instance = instance(1, 16.0 / 9.0);
    duplicate(&instance, 0);
    duplicate(&instance, 0);
    duplicate(&instance, 0);
    assert_eq!(instance.lock().tiles.len(), 4);
    assert_eq!(instance.lock().layout.rows.len(), 2, "four widescreen media are a 2x2 grid");
    let all = keys(&instance);
    for tile in 0..4 {
        open_slider(&instance, tile);
    }
    check(&instance, &all, "2x2 with four open bars");
    // The bars are in the canvas of their cell: the two upper ones at the top of the view, the lower ones below.
    {
        let guard = instance.lock();
        let tops: Vec<i32> = guard.media_bar_targets(&[]).iter().map(|t| t.rect.y).collect();
        assert!(tops.iter().filter(|y| **y == 0).count() == 2 && tops.iter().filter(|y| **y > 0).count() == 2, "{tops:?}");
    }
    remove(&instance, 2);
    check(&instance, &all, "three left: a row");
    assert_eq!(instance.lock().layout.rows.len(), 1);
    duplicate(&instance, 0);
    check(&instance, &all, "four again: a grid");
    assert_eq!(instance.lock().layout.rows.len(), 2);
    remove(&instance, 3);
    remove(&instance, 0);
    check(&instance, &all, "two left");
}

#[test]
fn swapping_tiles_moves_the_bars_with_them() {
    let instance = instance(3, 4.0 / 3.0);
    let all = keys(&instance);
    open_slider(&instance, 0);
    open_slider(&instance, 2);
    let open = vec![all[0], all[2]];
    check(&instance, &open, "before");
    let room_of = |instance: &SharedInstance, key: TileKey| {
        let guard = instance.lock();
        let index = keys_of(&guard).iter().position(|k| *k == key).unwrap();
        guard.host_room(index).unwrap()
    };
    let before = room_of(&instance, all[0]);
    instance.lock().swap(0);
    check(&instance, &open, "after a swap");
    assert_ne!(room_of(&instance, all[0]), before, "the tile moved, and its bar went along (the check above compares with its room)");
}

#[test]
fn the_bar_of_the_hovered_tile_comes_with_the_pointer_next_to_the_open_ones() {
    let instance = instance(3, 4.0 / 3.0);
    let all = keys(&instance);
    open_slider(&instance, 0);
    let room2 = instance.lock().host_room(2).unwrap();
    instance.lock().poll_pointer(Some((room2.x + room2.w - 10, room2.y + 10)));
    let targets = instance.lock().media_bar_targets(&[]);
    let by_key = |key: TileKey| targets.iter().find(|t| t.key == key);
    assert!(by_key(all[0]).is_some_and(|t| t.forced), "the open one stays");
    assert!(by_key(all[2]).is_some_and(|t| !t.forced), "the hovered one follows the pointer");
    assert!(by_key(all[1]).is_none());
    // A pointer far from every corner: only the open one.
    instance.lock().poll_pointer(Some((800, 800)));
    assert_eq!(instance.lock().media_bar_targets(&[]).len(), 1);
    // The host keeps a bar for a tile that has a menu open (its key), without a slider.
    assert_eq!(instance.lock().media_bar_targets(&[all[1]]).len(), 2);
    // And a stale key (the tile left) is ignored.
    assert_eq!(instance.lock().media_bar_targets(&[9999]).len(), 1);
}

#[test]
fn a_removed_tile_with_an_open_slider_leaves_nothing_behind_and_a_new_tile_starts_closed() {
    let instance = instance(2, 4.0 / 3.0);
    let all = keys(&instance);
    open_slider(&instance, 1);
    remove(&instance, 1);
    check(&instance, &[all[1]], "the open tile left");
    assert!(instance.lock().zoom_slider.is_empty());
    duplicate(&instance, 0);
    let now = keys(&instance);
    let new = *now.iter().find(|k| !all.contains(k)).unwrap();
    assert!(!instance.lock().zoom_slider.contains(&new), "a new tile never inherits a slider");
    // Keys are not reused for another tile while the old key could still be open somewhere.
    assert!(!all.contains(&new));
}
