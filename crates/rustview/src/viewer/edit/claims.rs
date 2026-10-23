//! One edit per file: the tile that edits a medium holds a claim on it, and every other tile of the same medium (a duplicate in
//! the same Multiview, the same file in another tab or in the fullscreen) cannot start editing it until the claim is given
//! back. The claim is released when it is dropped, so a tile that goes away (closed, removed, its tab merged and dropped)
//! never leaves a medium locked.

use crate::viewer::multiview::follow::TileKey;
use parking_lot::Mutex;
use rustview_core::library::models::MediaId;
use std::collections::HashMap;
use std::sync::LazyLock;

static CLAIMS: LazyLock<Mutex<HashMap<MediaId, TileKey>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// The tile that edits `media`, if any.
pub fn owner(media: MediaId) -> Option<TileKey> {
    CLAIMS.lock().get(&media).copied()
}

/// The right of one tile to edit one medium; released on drop.
#[derive(Debug)]
pub struct EditClaim {
    media: MediaId,
    tile: TileKey,
}

impl EditClaim {
    /// Takes the claim on `media` for `tile`; `None` when another tile holds it.
    pub fn take(media: MediaId, tile: TileKey) -> Option<Self> {
        let mut claims = CLAIMS.lock();
        match claims.get(&media) {
            Some(holder) if *holder != tile => None,
            _ => {
                claims.insert(media, tile);
                Some(Self { media, tile })
            }
        }
    }

    pub fn media(&self) -> MediaId {
        self.media
    }
}

impl Drop for EditClaim {
    fn drop(&mut self) {
        let mut claims = CLAIMS.lock();
        if claims.get(&self.media) == Some(&self.tile) {
            claims.remove(&self.media);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ids far away from what other tests use (the claims are global).
    const MEDIA: MediaId = 9_000_001;

    #[test]
    fn one_tile_at_a_time_edits_a_medium_and_dropping_the_claim_frees_it() {
        let first = EditClaim::take(MEDIA, 1).expect("free");
        assert_eq!(owner(MEDIA), Some(1));
        assert!(EditClaim::take(MEDIA, 2).is_none());
        drop(first);
        assert_eq!(owner(MEDIA), None);
        let second = EditClaim::take(MEDIA, 2).expect("free again");
        assert_eq!(second.media(), MEDIA);
        drop(second);
        assert_eq!(owner(MEDIA), None);
    }
}
