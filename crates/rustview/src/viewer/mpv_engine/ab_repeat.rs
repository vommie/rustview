//! The player's side of the `ab_repeat.lua` support (`viewer::ab_repeat`): it finds out whether the script runs in this player,
//! observes what the script publishes (`user-data/ab_repeat/{info,state,ranges}`, as JSON strings: no polling) and sends its
//! commands. When this player changed the saved ranges of a file, every other player showing that file is asked to read them
//! again (event-driven: the script publishes the change, the observer thread of this player forwards it).

use super::{MpvNotice, MpvPlayer};
use crate::viewer::ab_repeat::protocol::{
    self, Cause, Command, INFO_PATH, RANGES_PATH, RangeList, STATE_PATH, ScriptInfo, ScriptState,
};
use crate::viewer::ab_repeat::{prefs, support, support::Support};
use crate::viewer::error::ViewerError;
use libmpv2::{Format, Mpv};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

/// The ids of the observed paths (`reply_userdata` of their events).
pub(super) const INFO_ID: u64 = 0xab01;
pub(super) const STATE_ID: u64 = 0xab02;
pub(super) const RANGES_ID: u64 = 0xab03;

/// What changed (the callback's argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbChange {
    Support,
    State,
    Ranges,
}

/// What the player knows about the script right now.
#[derive(Debug, Clone, Default)]
pub struct AbSnapshot {
    pub support: Support,
    pub state: ScriptState,
    pub ranges: Arc<RangeList>,
    /// The paths are observed (the support is switched on).
    pub watching: bool,
}

impl AbSnapshot {
    /// The script runs with its interface and the support is on.
    pub fn active(&self) -> bool {
        self.watching && self.support.client().is_some()
    }
}

/// Every player, for forwarding a change of the saved ranges and for applying changed settings.
struct Registered {
    id: u64,
    mpv: Weak<Mpv>,
    ab: Arc<RwLock<AbSnapshot>>,
    listener: Listener,
}

/// A registered player that is still alive.
struct Live {
    mpv: Arc<Mpv>,
    ab: Arc<RwLock<AbSnapshot>>,
    listener: Listener,
}

impl Live {
    fn notify(&self, change: AbChange) {
        let callback = self.listener.read().clone();
        if let Some(callback) = callback {
            callback(MpvNotice::AbRepeat(change));
        }
    }
}

fn live_players() -> Vec<Live> {
    PLAYERS
        .lock()
        .iter()
        .filter_map(|p| {
            p.mpv.upgrade().map(|mpv| Live {
                mpv,
                ab: Arc::clone(&p.ab),
                listener: Arc::clone(&p.listener),
            })
        })
        .collect()
}

/// The `script-opts` key of the script's on-screen messages (`prefs::AbRepeatPrefs::script_opt` sets it to `no`).
const OSD_MESSAGES_KEY: &str = "ab_repeat-osd_messages";

/// The listener slot of a player (`MpvPlayer::on_notice`).
type Listener = Arc<RwLock<Option<super::MpvNoticeCallback>>>;

static PLAYERS: Mutex<Vec<Registered>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The script's own on-screen messages follow Settings > Multiview: switched off, `ab_repeat-osd_messages=no` is added to the
/// player's `script-opts`; switched on, the key is removed again (the script's own configuration decides). Other script options
/// of the user stay as they are.
pub(super) fn apply_script_opts(mpv: &Mpv) {
    let result = match prefs::current().script_opt() {
        Some(opt) => mpv.command("change-list", &["script-opts", "append", opt]),
        None => mpv.command("change-list", &["script-opts", "remove", OSD_MESSAGES_KEY]),
    };
    if let Err(e) = result {
        tracing::warn!(error = ?e, "[AbRepeat: Player] The script options could not be set");
    }
}

pub(super) fn register(mpv: &Arc<Mpv>, ab: &Arc<RwLock<AbSnapshot>>, listener: &Listener) -> u64 {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let mut players = PLAYERS.lock();
    players.retain(|p| p.mpv.strong_count() > 0);
    players.push(Registered {
        id,
        mpv: Arc::downgrade(mpv),
        ab: Arc::clone(ab),
        listener: Arc::clone(listener),
    });
    id
}

pub(super) fn unregister(id: u64) {
    PLAYERS.lock().retain(|p| p.id != id && p.mpv.strong_count() > 0);
}

/// Settings > Multiview changed: every player watches the script or stops, and gets the script's message switch.
pub fn apply_prefs() {
    let enabled = prefs::current().enabled;
    for player in live_players() {
        apply_script_opts(&player.mpv);
        if player.ab.read().watching != enabled {
            watch(&player.mpv, &player.ab, enabled);
            player.notify(AbChange::Support);
        }
    }
}

/// Starts or stops observing the script in one player; starting also decides whether the script is there.
pub(super) fn watch(mpv: &Mpv, ab: &RwLock<AbSnapshot>, on: bool) {
    if ab.read().watching == on {
        return;
    }
    if !on {
        for id in [INFO_ID, STATE_ID, RANGES_ID] {
            let _ = mpv.unobserve_property(id);
        }
        *ab.write() = AbSnapshot::default();
        tracing::debug!("[AbRepeat: Player] Stopped watching the script");
        return;
    }
    // `mpv_initialize` returns before the scripts ran (mpv only waits for them before playback): without the info yet, the
    // answer waits for the first loaded file (`settle`), or comes with the info itself.
    let support = read_info(mpv).map_or(Support::Unknown, |info| Support::decide(Some(&info), &[]));
    if support != Support::Unknown {
        support::record(&support);
    }
    {
        let mut snapshot = ab.write();
        snapshot.support = support.clone();
        snapshot.watching = true;
    }
    for (path, id) in [(INFO_PATH, INFO_ID), (STATE_PATH, STATE_ID), (RANGES_PATH, RANGES_ID)] {
        if let Err(e) = mpv.observe_property(path, Format::String, id) {
            tracing::warn!(path, error = ?e, "[AbRepeat: Player] A path of the script could not be observed");
        }
    }
    tracing::info!(?support, "[AbRepeat: Player] Watching the script");
}

/// A file was loaded (observer thread): every script has run by now, so a support that is still unknown is decided (the
/// script without its info: an old version, or none).
pub(super) fn settle(mpv: &Mpv, ab: &RwLock<AbSnapshot>) -> Option<AbChange> {
    {
        let snapshot = ab.read();
        if !snapshot.watching || snapshot.support != Support::Unknown {
            return None;
        }
    }
    let info = read_info(mpv);
    let support = if info.is_some() {
        Support::decide(info.as_ref(), &[])
    } else {
        Support::decide(None, &bindings(mpv))
    };
    support::record(&support);
    ab.write().support = support;
    Some(AbChange::Support)
}

/// The info the script published in `mpv` (also remembers where it saves).
fn read_info(mpv: &Mpv) -> Option<ScriptInfo> {
    let info = mpv
        .get_property::<String>(INFO_PATH)
        .ok()
        .and_then(|json| protocol::parse::<ScriptInfo>(&json))?;
    support::record_db(&info);
    Some(info)
}

/// The owners and commands of mpv's key bindings (an old script without the interface is known by them).
fn bindings(mpv: &Mpv) -> Vec<(String, String)> {
    let Ok(json) = mpv.get_property::<String>("input-bindings") else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str::<serde_json::Value>(&json) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let owner = entry.get("owner")?.as_str()?;
            let cmd = entry.get("cmd")?.as_str()?;
            Some((owner.to_owned(), cmd.to_owned()))
        })
        .collect()
}

/// An observed path changed (observer thread). Returns what changed for the listener.
pub(super) fn on_property(mpv: &Mpv, ab: &RwLock<AbSnapshot>, id: u64, json: &str) -> Option<AbChange> {
    match id {
        INFO_ID => {
            let info = protocol::parse::<ScriptInfo>(json);
            if let Some(info) = &info {
                support::record_db(info);
            }
            let support = Support::decide(info.as_ref(), &[]);
            support::record(&support);
            let mut snapshot = ab.write();
            if !snapshot.watching || snapshot.support == support {
                return None;
            }
            snapshot.support = support;
            Some(AbChange::Support)
        }
        STATE_ID => {
            let state = protocol::parse::<ScriptState>(json).unwrap_or_default();
            let mut snapshot = ab.write();
            if !snapshot.watching || snapshot.state == state {
                return None;
            }
            snapshot.state = state;
            Some(AbChange::State)
        }
        RANGES_ID => {
            let ranges = protocol::parse::<RangeList>(json).unwrap_or_default();
            let (cause, path) = (ranges.cause, ranges.path.clone());
            {
                let mut snapshot = ab.write();
                if !snapshot.watching {
                    return None;
                }
                snapshot.ranges = Arc::new(ranges);
            }
            if cause == Cause::Write {
                reload_others(mpv, &path);
            }
            Some(AbChange::Ranges)
        }
        _ => None,
    }
}

/// Asks every other player that shows `path` to publish its saved ranges again (this one changed them).
fn reload_others(source: &Mpv, path: &str) {
    if path.is_empty() {
        return;
    }
    let others: Vec<(Arc<Mpv>, String)> = live_players()
        .into_iter()
        .filter_map(|p| {
            if std::ptr::eq(Arc::as_ptr(&p.mpv), source) {
                return None;
            }
            let snapshot = p.ab.read();
            let client = snapshot.support.client()?.to_owned();
            (snapshot.watching && snapshot.ranges.path == path).then(|| (Arc::clone(&p.mpv), client))
        })
        .collect();
    for (mpv, client) in &others {
        send(mpv, client, &Command::Reload);
    }
    if !others.is_empty() {
        tracing::debug!(
            players = others.len(),
            "[AbRepeat: Sync] Other players of the file read its saved repeats again"
        );
    }
}

fn send(mpv: &Mpv, client: &str, command: &Command) -> bool {
    let args = command.args(client);
    let mut all: Vec<&str> = vec!["script-message-to"];
    all.extend(args.iter().map(String::as_str));
    match mpv.command(all[0], &all[1..]) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!(?command, error = ?e, "[AbRepeat: Player] The script did not take a command");
            false
        }
    }
}

impl MpvPlayer {
    /// What the player knows about the script now.
    pub fn ab_repeat(&self) -> AbSnapshot {
        self.ab.read().clone()
    }

    /// Sends `command` to the script. Fails when the script is not there (or the support is off).
    pub fn ab_command(&self, command: &Command) -> Result<(), ViewerError> {
        let client = {
            let snapshot = self.ab.read();
            if !snapshot.active() {
                return Err(ViewerError::Mpv(
                    "the ab_repeat script is not active in this player".to_owned(),
                ));
            }
            snapshot.support.client().unwrap_or_default().to_owned()
        };
        tracing::debug!(?command, "[AbRepeat: Player] Command");
        if send(&self.mpv, &client, command) {
            Ok(())
        } else {
            Err(ViewerError::Mpv(format!("ab_repeat did not take {command:?}")))
        }
    }
}

/// A fresh, invisible mpv with the user's configuration and scripts (no video, no window).
fn headless() -> Option<Mpv> {
    let config_dir = super::self_test::user_config_dir();
    Mpv::with_initializer(|init| {
        init.set_property("config", true)?;
        if let Some(dir) = &config_dir {
            init.set_property("config-dir", dir.to_string_lossy().as_ref())?;
        }
        init.set_property("load-scripts", true)?;
        init.set_property("vo", "null")?;
        init.set_property("ao", "null")?;
        init.set_property("idle", "yes")?;
        init.set_property("force-window", "no")?;
        init.set_property("input-vo-keyboard", false)?;
        Ok(())
    })
    .map_err(|e| tracing::warn!(error = ?e, "[AbRepeat: Probe] mpv did not start"))
    .ok()
}

/// The info of the script in a headless mpv: its scripts start after `mpv_initialize` returns, so this waits up to 3 s.
fn wait_for_info(mpv: &Mpv) -> Option<ScriptInfo> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        if let Some(info) = read_info(mpv) {
            return Some(info);
        }
        if std::time::Instant::now() >= until {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Looks for the script in a fresh, invisible mpv with the user's configuration (Settings > Multiview, Check again):
/// a replaced `ab_repeat.lua` is what every video opened from now on gets, without restarting `RustView`. Blocks while mpv starts
/// and loads its scripts (call it on a worker thread); the answer is recorded for the settings card.
pub fn probe() -> Support {
    let support = match headless() {
        Some(mpv) => match wait_for_info(&mpv) {
            Some(info) => Support::decide(Some(&info), &[]),
            None => Support::decide(None, &bindings(&mpv)),
        },
        None => Support::Unknown,
    };
    tracing::info!(?support, "[AbRepeat: Probe] Script checked");
    support::record(&support);
    support
}

/// The saved ranges of renamed or moved files go to their new paths (`pairs` of old and new path), through the script: a
/// player that runs it, else a headless mpv started for it. Blocks until the script is done (call it on a worker thread).
pub fn move_paths(pairs: &[(String, String)]) {
    if pairs.is_empty() {
        return;
    }
    let live = live_players().into_iter().find_map(|p| {
        let client = p.ab.read().support.client().map(str::to_owned)?;
        Some((p.mpv, client))
    });
    if let Some((mpv, client)) = live {
        for (from, to) in pairs {
            send(&mpv, &client, &Command::MovePath(from.clone(), to.clone()));
        }
        tracing::info!(
            files = pairs.len(),
            "[AbRepeat: Move] Saved repeats moved along through a running player"
        );
        return;
    }
    let Some(mpv) = headless() else {
        return;
    };
    let Some(info) = wait_for_info(&mpv) else {
        tracing::info!("[AbRepeat: Move] No compatible script runs: the saved repeats stay under the old paths");
        return;
    };
    let client = info.client.clone();
    for (from, to) in pairs {
        send(&mpv, &client, &Command::MovePath(from.clone(), to.clone()));
    }
    let token = format!("moved-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
    send(&mpv, &client, &Command::Ping(token.clone()));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < until {
        // A user-data value comes as JSON (a string in quotes).
        let pong = mpv
            .get_property::<String>(protocol::PONG_PATH)
            .ok()
            .and_then(|json| protocol::parse::<String>(&json));
        if pong.as_deref() == Some(token.as_str()) {
            tracing::info!(
                files = pairs.len(),
                "[AbRepeat: Move] Saved repeats moved along through a headless mpv"
            );
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    tracing::warn!("[AbRepeat: Move] The script did not confirm the move in time");
}
