use super::types::{DEFAULT_THUMBNAIL_BOUND, THUMBNAIL_CHANNELS, ThumbnailHandle};
use parking_lot::Mutex;

struct ArenaSlot {
    generation: u32,
    in_use: bool,
}

pub const FIXED_ARENA_BYTES: usize =
    (DEFAULT_THUMBNAIL_BOUND * DEFAULT_THUMBNAIL_BOUND * THUMBNAIL_CHANNELS as u32) as usize;

/// Fixed-size memory arena retained for backward compatibility with existing benchmarks.
pub struct ThumbnailArena {
    slot_count: usize,
    buffer: Vec<u8>,
    slots: Mutex<Vec<ArenaSlot>>,
}

impl ThumbnailArena {
    pub fn new(slot_count: usize) -> Self {
        let total_bytes = slot_count * FIXED_ARENA_BYTES;
        let mut buffer = vec![0u8; total_bytes];
        buffer.fill(0);

        let mut slots = Vec::with_capacity(slot_count);
        for _ in 0..slot_count {
            slots.push(ArenaSlot {
                generation: 1,
                in_use: false,
            });
        }

        Self {
            slot_count,
            buffer,
            slots: Mutex::new(slots),
        }
    }

    pub fn slot_count(&self) -> usize {
        self.slot_count
    }

    pub fn write_slot(&self, slot_index: usize, rgb_data: &[u8]) -> Option<ThumbnailHandle> {
        if slot_index >= self.slot_count || rgb_data.len() > FIXED_ARENA_BYTES {
            return None;
        }

        let mut slots = self.slots.lock();
        let slot = &mut slots[slot_index];
        slot.in_use = true;
        let generation = slot.generation;

        let start = slot_index * FIXED_ARENA_BYTES;
        let dst_ptr = self.buffer.as_ptr() as *mut u8;
        unsafe {
            let slice = std::slice::from_raw_parts_mut(dst_ptr.add(start), rgb_data.len());
            slice.copy_from_slice(rgb_data);
        }

        Some(ThumbnailHandle {
            slot_index: slot_index as u32,
            generation,
        })
    }

    pub fn read_slot(&self, handle: ThumbnailHandle) -> Option<&[u8]> {
        let idx = handle.slot_index as usize;
        if idx >= self.slot_count {
            return None;
        }

        let slots = self.slots.lock();
        let slot = &slots[idx];
        if !slot.in_use || slot.generation != handle.generation {
            return None;
        }

        let start = idx * FIXED_ARENA_BYTES;
        let end = start + FIXED_ARENA_BYTES;
        Some(&self.buffer[start..end])
    }

    pub fn invalidate_slot(&self, slot_index: usize) {
        if slot_index < self.slot_count {
            let mut slots = self.slots.lock();
            let slot = &mut slots[slot_index];
            slot.in_use = false;
            slot.generation = slot.generation.wrapping_add(1);
        }
    }
}
