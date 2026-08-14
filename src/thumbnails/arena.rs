use super::types::{THUMBNAIL_BYTE_SIZE, ThumbnailHandle};
use parking_lot::Mutex;

struct ArenaSlot {
    generation: u32,
    in_use: bool,
}

/// Contiguous non-fragmenting memory arena holding decoded RGB8 thumbnails.
pub struct ThumbnailArena {
    slot_count: usize,
    buffer: Vec<u8>,
    slots: Mutex<Vec<ArenaSlot>>,
}

impl ThumbnailArena {
    pub fn new(slot_count: usize) -> Self {
        let total_bytes = slot_count * THUMBNAIL_BYTE_SIZE;
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

    /// Acquires a slot, copying decoded 256x256 RGB8 pixels into the contiguous arena.
    pub fn write_slot(&self, slot_index: usize, rgb_data: &[u8]) -> Option<ThumbnailHandle> {
        if slot_index >= self.slot_count || rgb_data.len() != THUMBNAIL_BYTE_SIZE {
            return None;
        }

        let mut slots = self.slots.lock();
        let slot = &mut slots[slot_index];
        slot.in_use = true;
        let generation = slot.generation;

        let start = slot_index * THUMBNAIL_BYTE_SIZE;

        // Safety: Distinct slots are indexed without overlapping ranges.
        let dst_ptr = self.buffer.as_ptr() as *mut u8;
        unsafe {
            let slice = std::slice::from_raw_parts_mut(dst_ptr.add(start), THUMBNAIL_BYTE_SIZE);
            slice.copy_from_slice(rgb_data);
        }

        Some(ThumbnailHandle {
            slot_index: slot_index as u32,
            generation,
        })
    }

    /// Reads raw RGB8 pixel slice from a slot if the handle generation is valid.
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

        let start = idx * THUMBNAIL_BYTE_SIZE;
        let end = start + THUMBNAIL_BYTE_SIZE;
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
