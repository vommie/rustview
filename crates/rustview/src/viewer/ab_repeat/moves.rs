//! Saved A-B repeats go along when `RustView` renames or moves a file (user decision 2026-10-25): the script keys them by the
//! file's path. The library reports every moved file (`set_move_observer`); the moves of one user action are collected for a
//! moment and handed to a worker, which reads the script's JSON (an explicit action of the user, so reading is fine) and only
//! when an old path has saved repeats lets the script move them (`mpv_engine::ab_repeat::move_paths`): `RustView` never writes
//! the script's file itself.

use super::{prefs, support};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

/// How long moves are collected before they are handled together (a batch move reports one file after the other).
const COLLECT: Duration = Duration::from_millis(300);

static QUEUE: OnceLock<Mutex<Sender<(PathBuf, PathBuf)>>> = OnceLock::new();

/// The library moved a file from `from` to `to` (a rename or a move; called on the thread that moved it).
pub fn file_moved(from: &Path, to: &Path) {
    if !prefs::current().enabled {
        return;
    }
    let queue = QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("ab-repeat-moves".into())
            .spawn(move || run(&rx));
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[AbRepeat: Move] The worker could not start");
        }
        Mutex::new(tx)
    });
    let _ = queue.lock().send((from.to_path_buf(), to.to_path_buf()));
}

fn run(rx: &Receiver<(PathBuf, PathBuf)>) {
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        while let Ok(next) = rx.recv_timeout(COLLECT) {
            batch.push(next);
        }
        let pairs: Vec<(String, String)> = batch
            .iter()
            .map(|(from, to)| (from.to_string_lossy().into_owned(), to.to_string_lossy().into_owned()))
            .collect();
        let saved = saved_paths();
        let moving = with_saved_repeats(&pairs, &saved);
        if moving.is_empty() {
            continue;
        }
        tracing::info!(files = moving.len(), "[AbRepeat: Move] Moved files have saved repeats");
        crate::viewer::mpv_engine::ab_repeat::move_paths(&moving);
    }
}

/// The paths the script's JSON has saved repeats for (empty when it cannot be read).
fn saved_paths() -> HashSet<String> {
    let Some(db) = support::db_path().map(PathBuf::from).or_else(default_db) else {
        return HashSet::new();
    };
    let Ok(text) = std::fs::read_to_string(&db) else {
        return HashSet::new();
    };
    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Object(map)) => map.into_iter().map(|(key, _)| key).collect(),
        _ => HashSet::new(),
    }
}

/// Where the script saves when no player told yet: `$HOME/.config/mpv/ab_repeat.json` (the script's own rule).
fn default_db() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".config/mpv/ab_repeat.json"))
}

/// The moves whose old path has saved repeats.
fn with_saved_repeats(pairs: &[(String, String)], saved: &HashSet<String>) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter(|(from, to)| from != to && saved.contains(from))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_moves_of_files_with_saved_repeats_go_to_the_script() {
        let saved: HashSet<String> = ["/v/a.mp4".to_owned()].into_iter().collect();
        let pairs = vec![
            ("/v/a.mp4".to_owned(), "/v/b.mp4".to_owned()),
            ("/v/c.jpg".to_owned(), "/w/c.jpg".to_owned()),
            ("/v/a.mp4".to_owned(), "/v/a.mp4".to_owned()),
        ];
        assert_eq!(
            with_saved_repeats(&pairs, &saved),
            vec![("/v/a.mp4".to_owned(), "/v/b.mp4".to_owned())]
        );
    }
}
