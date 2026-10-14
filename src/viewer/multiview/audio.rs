//! Sound rules of the Multiview (pure functions, table-testable).
//!
//! With several videos only one of them is audible: the leftmost one when the Multiview opens. `Native` means "mpv's own
//! state is left alone"; a tile that returns to `Native` gets the mute value mpv started with (captured at tile creation),
//! never a blind "unmuted". A mute set by the user (badge, `m`, mpv OSC) is never overridden by the policy.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioMode {
    Native,
    Muted,
    Unmuted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioOrigin {
    Policy,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileAudio {
    pub mode: AudioMode,
    pub origin: AudioOrigin,
}

impl TileAudio {
    pub const fn native() -> Self {
        Self {
            mode: AudioMode::Native,
            origin: AudioOrigin::Policy,
        }
    }

    pub const fn muted_by_policy() -> Self {
        Self {
            mode: AudioMode::Muted,
            origin: AudioOrigin::Policy,
        }
    }

    /// True if the tile carries the sound (everything except an explicitly muted tile).
    pub fn is_audible(&self) -> bool {
        !matches!(self.mode, AudioMode::Muted)
    }

    /// The mute state mpv must have; `Native` restores the value mpv started with.
    pub fn target_mute(&self, native_mute: bool) -> bool {
        match self.mode {
            AudioMode::Native => native_mute,
            AudioMode::Muted => true,
            AudioMode::Unmuted => false,
        }
    }

    /// A click on the mute badge: audible becomes muted, muted becomes unmuted; either way the user decided.
    pub fn toggled_by_user(&self) -> Self {
        let mode = if self.is_audible() {
            AudioMode::Muted
        } else {
            AudioMode::Unmuted
        };
        Self {
            mode,
            origin: AudioOrigin::User,
        }
    }
}

/// One entry per tile in slot order; `None` for images (no sound).
pub type AudioSlots = [Option<TileAudio>];

fn any_audible(slots: &AudioSlots) -> bool {
    slots.iter().flatten().any(TileAudio::is_audible)
}

/// When the Multiview opens: the leftmost video keeps its native state, every further video starts muted.
pub fn assign_on_open(slots: &mut AudioSlots) {
    for (rank, slot) in slots.iter_mut().flatten().enumerate() {
        *slot = if rank == 0 {
            TileAudio::native()
        } else {
            TileAudio::muted_by_policy()
        };
    }
}

/// After a reconcile (removals already applied to `slots`): new videos start muted unless no video has sound anymore,
/// then the first new video gets it. Without new videos, a removed sound carrier hands the sound to the leftmost
/// policy-muted video (user mutes stay).
pub fn after_reconcile(slots: &mut AudioSlots, added: &[usize]) {
    let new_videos: Vec<usize> = added
        .iter()
        .copied()
        .filter(|&i| slots.get(i).is_some_and(Option::is_some))
        .collect();
    for &i in &new_videos {
        slots[i] = Some(TileAudio::muted_by_policy());
    }
    if any_audible(slots) {
        return;
    }
    if let Some(&first_new) = new_videos.first() {
        slots[first_new] = Some(TileAudio::native());
    } else if let Some(slot) = slots
        .iter_mut()
        .flatten()
        .find(|s| s.origin == AudioOrigin::Policy)
    {
        *slot = TileAudio::native();
    }
}
