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

#[cfg(test)]
mod tests {
    use super::*;

    fn videos(n: usize) -> Vec<Option<TileAudio>> {
        vec![Some(TileAudio::native()); n]
    }

    fn modes(slots: &[Option<TileAudio>]) -> Vec<Option<(AudioMode, AudioOrigin)>> {
        slots
            .iter()
            .map(|s| s.map(|a| (a.mode, a.origin)))
            .collect()
    }

    use AudioMode::{Muted, Native, Unmuted};
    use AudioOrigin::{Policy, User};

    #[test]
    fn opening_keeps_only_the_leftmost_video_audible() {
        let mut s = vec![
            None,
            Some(TileAudio::native()),
            Some(TileAudio::native()),
            Some(TileAudio::native()),
        ];
        assign_on_open(&mut s);
        assert_eq!(
            modes(&s),
            vec![
                None,
                Some((Native, Policy)),
                Some((Muted, Policy)),
                Some((Muted, Policy))
            ]
        );
        let mut single = vec![None, Some(TileAudio::muted_by_policy()), None];
        assign_on_open(&mut single);
        assert_eq!(
            single[1],
            Some(TileAudio::native()),
            "a single video is left completely alone"
        );
    }

    #[test]
    fn new_videos_start_muted_while_someone_has_sound() {
        let mut s = videos(3);
        assign_on_open(&mut s);
        s.push(Some(TileAudio::native()));
        after_reconcile(&mut s, &[3]);
        assert_eq!(s[3], Some(TileAudio::muted_by_policy()));
        assert_eq!(s[0], Some(TileAudio::native()));
    }

    #[test]
    fn removing_the_sound_carrier_hands_the_sound_to_the_leftmost_policy_video() {
        let mut s = videos(3);
        assign_on_open(&mut s);
        s.remove(0);
        after_reconcile(&mut s, &[]);
        assert_eq!(
            modes(&s),
            vec![Some((Native, Policy)), Some((Muted, Policy))]
        );
    }

    #[test]
    fn user_mutes_survive_and_a_new_video_takes_the_sound_when_nobody_has_it() {
        let mut s = videos(2);
        assign_on_open(&mut s);
        s[0] = Some(s[0].unwrap().toggled_by_user());
        assert_eq!(modes(&s), vec![Some((Muted, User)), Some((Muted, Policy))]);
        s.push(Some(TileAudio::native()));
        after_reconcile(&mut s, &[2]);
        assert_eq!(
            modes(&s),
            vec![
                Some((Muted, User)),
                Some((Muted, Policy)),
                Some((Native, Policy))
            ]
        );
    }

    #[test]
    fn user_mutes_are_never_handed_the_sound_by_the_policy() {
        let mut s = vec![
            Some(TileAudio::native().toggled_by_user()),
            Some(TileAudio::native()),
        ];
        s[1] = Some(TileAudio::muted_by_policy());
        s.remove(1);
        after_reconcile(&mut s, &[]);
        assert_eq!(modes(&s), vec![Some((Muted, User))]);
    }

    #[test]
    fn toggling_alternates_between_muted_and_unmuted() {
        let a = TileAudio::native().toggled_by_user();
        assert_eq!((a.mode, a.origin), (Muted, User));
        let b = a.toggled_by_user();
        assert_eq!((b.mode, b.origin), (Unmuted, User));
        assert_eq!(b.toggled_by_user().mode, Muted);
    }

    #[test]
    fn native_restores_the_state_mpv_started_with() {
        assert!(!TileAudio::native().target_mute(false));
        assert!(
            TileAudio::native().target_mute(true),
            "a user config with mute=yes stays muted"
        );
        assert!(TileAudio::muted_by_policy().target_mute(false));
        assert!(
            !TileAudio {
                mode: Unmuted,
                origin: User
            }
            .target_mute(true)
        );
    }
}
