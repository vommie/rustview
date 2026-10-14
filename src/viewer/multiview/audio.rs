//! Sound rules of the Multiview (pure functions, table-testable).
//!
//! The user's policy (Settings > Fullscreen > Sound) decides which videos are muted automatically. By default only one
//! video is audible: the leftmost one when the Multiview opens. `Native` means "mpv's own state is left alone"; a tile
//! that returns to `Native` gets the mute value mpv started with (captured at tile creation), never a blind "unmuted".
//! A mute set by the user (badge, `m`, mpv OSC) is never overridden by the policy.

/// Who carries the sound while several videos are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundFollows {
    /// The leftmost video; videos that join later start muted.
    First,
    /// The video that joined last takes the sound from the others.
    Newest,
    /// Every video starts muted.
    Nobody,
}

impl SoundFollows {
    pub fn from_setting(value: &str) -> Self {
        match value {
            "newest" => Self::Newest,
            "nobody" => Self::Nobody,
            _ => Self::First,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioPolicy {
    /// Off: no video is muted automatically, every video keeps its own state.
    pub auto_control: bool,
    pub follows: SoundFollows,
    /// The sound passes to another video when the video that had it leaves.
    pub hand_over: bool,
}

impl AudioPolicy {
    pub const DEFAULT: Self = Self {
        auto_control: true,
        follows: SoundFollows::First,
        hand_over: true,
    };
}

impl Default for AudioPolicy {
    fn default() -> Self {
        Self::DEFAULT
    }
}

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

/// When the Multiview opens: the leftmost video keeps its native state, every further video starts muted (`Nobody`: all
/// of them start muted, no automatic control: all keep their native state).
pub fn assign_on_open(slots: &mut AudioSlots, policy: &AudioPolicy) {
    for (rank, slot) in slots.iter_mut().flatten().enumerate() {
        let carries_sound = match policy.follows {
            _ if !policy.auto_control => true,
            SoundFollows::Nobody => false,
            SoundFollows::First | SoundFollows::Newest => rank == 0,
        };
        *slot = if carries_sound {
            TileAudio::native()
        } else {
            TileAudio::muted_by_policy()
        };
    }
}

/// After a reconcile (removals already applied to `slots`): what new videos do depends on the policy. By default they
/// start muted unless no video has sound anymore, then the first new video gets it; with `Newest` the first new video
/// takes the sound from every video the user did not touch. Without new videos, a removed sound carrier hands the sound to
/// the leftmost policy-muted video (user mutes stay) if `hand_over` is on.
pub fn after_reconcile(slots: &mut AudioSlots, added: &[usize], policy: &AudioPolicy) {
    let new_videos: Vec<usize> = added
        .iter()
        .copied()
        .filter(|&i| slots.get(i).is_some_and(Option::is_some))
        .collect();
    if !policy.auto_control {
        for &i in &new_videos {
            slots[i] = Some(TileAudio::native());
        }
        return;
    }
    for &i in &new_videos {
        slots[i] = Some(TileAudio::muted_by_policy());
    }
    match (policy.follows, new_videos.first()) {
        (SoundFollows::Nobody, _) => return,
        (SoundFollows::Newest, Some(&newest)) => {
            for slot in slots.iter_mut().flatten().filter(|s| s.origin == AudioOrigin::Policy) {
                *slot = TileAudio::muted_by_policy();
            }
            slots[newest] = Some(TileAudio::native());
            return;
        }
        _ => {}
    }
    if any_audible(slots) {
        return;
    }
    if let Some(&first_new) = new_videos.first() {
        slots[first_new] = Some(TileAudio::native());
    } else if policy.hand_over
        && let Some(slot) = slots
            .iter_mut()
            .flatten()
            .find(|s| s.origin == AudioOrigin::Policy)
    {
        *slot = TileAudio::native();
    }
}
