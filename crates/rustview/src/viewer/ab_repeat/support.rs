//! Whether a player runs a compatible `ab_repeat.lua`, and the last answer any player gave (Settings > Viewer > Videos shows
//! it, Settings > Debug checks it).

use super::protocol::{API_VERSION, ScriptInfo};
use parking_lot::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// The binding names of the script (also of old versions without the interface).
const BINDING_NAMES: [&str; 6] = [
    "set-a-point",
    "set-b-point",
    "reset-points",
    "save-ab-range",
    "load-ab-range-menu",
    "delete-ab-range-menu",
];

/// What a player found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Support {
    /// Nothing looked yet (no video was opened, or the support is off).
    #[default]
    Unknown,
    /// The script runs with the interface `RustView` speaks.
    Compatible { version: String, client: String },
    /// The script runs, but `RustView` cannot work with it.
    Incompatible(Incompatibility),
    /// No such script in mpv.
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incompatibility {
    /// The script runs without the interface (recognized by its key bindings): an older version, or `api=no` in its settings.
    NoInterface,
    /// Another interface version.
    OtherApi(u32),
}

impl Support {
    /// Decides from the published `info` (if any) and the owners and commands of mpv's `input-bindings`.
    pub fn decide(info: Option<&ScriptInfo>, bindings: &[(String, String)]) -> Self {
        if let Some(info) = info {
            return if info.api == API_VERSION && !info.client.is_empty() {
                Self::Compatible {
                    version: info.version.clone(),
                    client: info.client.clone(),
                }
            } else {
                Self::Incompatible(Incompatibility::OtherApi(info.api))
            };
        }
        let found = BINDING_NAMES.iter().all(|name| {
            bindings
                .iter()
                .any(|(owner, cmd)| !owner.is_empty() && cmd.ends_with(&format!("{owner}/{name}")))
        });
        if found {
            Self::Incompatible(Incompatibility::NoInterface)
        } else {
            Self::NotFound
        }
    }

    pub fn client(&self) -> Option<&str> {
        match self {
            Self::Compatible { client, .. } => Some(client),
            _ => None,
        }
    }

    /// The status line of the settings card.
    pub fn describe(&self) -> String {
        match self {
            Self::Unknown => "Not checked yet: it is checked when a video opens, or with Check again.".to_owned(),
            Self::Compatible { version, .. } => format!("Found: ab_repeat.lua {version} (interface {API_VERSION})"),
            Self::Incompatible(Incompatibility::NoInterface) => {
                "Incompatible: ab_repeat.lua runs without its interface (an older version, or api=no in its settings)."
                    .to_owned()
            }
            Self::Incompatible(Incompatibility::OtherApi(api)) => {
                format!("Incompatible: ab_repeat.lua speaks interface {api}, RustView interface {API_VERSION}.")
            }
            Self::NotFound => "Not found: mpv does not load ab_repeat.lua.".to_owned(),
        }
    }

    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Incompatible(_))
    }
}

static LAST: RwLock<Support> = RwLock::new(Support::Unknown);
/// The JSON file the script saves in, as a compatible script reported it.
static DB: RwLock<Option<String>> = RwLock::new(None);

/// Remembers the file a compatible script saves its ranges in.
pub fn record_db(info: &ScriptInfo) {
    if info.api == API_VERSION && !info.db.is_empty() {
        let mut db = DB.write();
        if db.as_deref() != Some(info.db.as_str()) {
            *db = Some(info.db.clone());
        }
    }
}

/// The JSON file the script saves in (`None`: no compatible script reported one yet).
pub fn db_path() -> Option<String> {
    DB.read().clone()
}
/// Counts the changes of `LAST`, so a settings page shown meanwhile can refresh itself.
static EPOCH: AtomicU64 = AtomicU64::new(0);

/// Remembers what a player (or a check) found; the newest answer wins.
pub fn record(support: &Support) {
    let mut last = LAST.write();
    if *last != *support {
        tracing::info!(?support, "[AbRepeat: Support] Script support changed");
        *last = support.clone();
        EPOCH.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn last() -> Support {
    LAST.read().clone()
}

pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bindings(owner: &str) -> Vec<(String, String)> {
        BINDING_NAMES
            .iter()
            .map(|name| (owner.to_owned(), format!("nonscalable script-binding {owner}/{name}")))
            .collect()
    }

    #[test]
    fn the_published_info_decides() {
        let info = ScriptInfo {
            api: API_VERSION,
            client: "ab_repeat_v2".into(),
            version: "2.0".into(),
            ..ScriptInfo::default()
        };
        let support = Support::decide(Some(&info), &[]);
        assert_eq!(support.client(), Some("ab_repeat_v2"));
        let other = ScriptInfo { api: 7, ..info };
        assert_eq!(
            Support::decide(Some(&other), &[]),
            Support::Incompatible(Incompatibility::OtherApi(7))
        );
    }

    #[test]
    fn an_old_script_is_known_by_its_bindings_whatever_its_file_is_called() {
        assert_eq!(
            Support::decide(None, &bindings("my_ab_loop")),
            Support::Incompatible(Incompatibility::NoInterface)
        );
        let mut partial = bindings("x");
        partial.pop();
        assert_eq!(Support::decide(None, &partial), Support::NotFound);
        assert_eq!(Support::decide(None, &[]), Support::NotFound);
        assert!(Support::decide(None, &bindings("x")).is_error());
    }
}
