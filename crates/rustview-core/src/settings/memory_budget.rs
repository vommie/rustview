//! How much RAM the application may use: resolved from the setting and from what the machine has.
//!
//! The setting is "automatic" (0) or a number of megabytes. Automatic means half of the usable RAM, where usable is the smaller
//! of the installed RAM and the limit of the control group the process runs in (a systemd scope, a container). Reading
//! `/proc` and the cgroup files costs microseconds and touches no drive, so the check runs on every start and whenever the
//! setting changes. The budget is split between the pieces that grow with use: the decoded thumbnails (an LRU that is trimmed
//! at once when the budget shrinks) and the records of the open folder.

use procfs::process::Process;
use procfs::{Current, Meminfo};
use std::fs;

pub const BYTES_PER_MB: u64 = 1024 * 1024;
/// Automatic budget: this share of the usable RAM.
pub const AUTO_BUDGET_PERCENT: u64 = 50;
/// The share of the budget that the decoded thumbnails may fill.
pub const THUMBNAIL_CACHE_PERCENT: u64 = 50;
/// The automatic thumbnail cache never exceeds this: scrolling needs a few thousand tiles, and the page cache of the
/// operating system already keeps the compressed thumbnails of everything seen recently.
pub const AUTO_THUMBNAIL_CACHE_CAP_BYTES: u64 = 4 * 1024 * BYTES_PER_MB;
pub const MIN_THUMBNAIL_CACHE_BYTES: u64 = 64 * BYTES_PER_MB;
/// A manual budget above this share of the usable RAM raises a warning.
pub const WARN_TOTAL_PERCENT: u64 = 75;

/// What the machine offers (all values in bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemMemory {
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// The memory limit of the control group of this process, if it has one.
    pub cgroup_limit_bytes: Option<u64>,
}

impl SystemMemory {
    /// Reads `/proc/meminfo` and the cgroup limits (through procfs). Unreadable files yield zeros/`None` (the budget then falls back to a default).
    pub fn read() -> Self {
        let (total_bytes, available_bytes) = Meminfo::current()
            .map(|info| (info.mem_total, info.mem_available.unwrap_or(0)))
            .unwrap_or((0, 0));
        let cgroup_limit_bytes = Process::myself()
            .and_then(|process| process.cgroups())
            .ok()
            .and_then(|groups| groups.0.into_iter().find(|group| group.hierarchy == 0))
            .and_then(|group| smallest_cgroup_limit(&group.pathname));
        Self {
            total_bytes,
            available_bytes,
            cgroup_limit_bytes,
        }
    }

    /// Installed RAM, reduced by the cgroup limit.
    pub fn usable_total_bytes(&self) -> u64 {
        match self.cgroup_limit_bytes {
            Some(limit) if self.total_bytes > 0 => self.total_bytes.min(limit),
            Some(limit) => limit,
            None => self.total_bytes,
        }
    }
}

/// The value of a `memory.max` file: a number of bytes, or `max` for no limit.
pub fn parse_cgroup_limit(text: &str) -> Option<u64> {
    text.trim().parse::<u64>().ok()
}

/// The smallest `memory.max` on the way from the process's cgroup up to the root (a limit of any ancestor applies too).
fn smallest_cgroup_limit(path: &str) -> Option<u64> {
    let mut limit: Option<u64> = None;
    let mut current = path.trim_end_matches('/').to_string();
    loop {
        let file = format!("/sys/fs/cgroup{current}/memory.max");
        if let Some(value) = fs::read_to_string(&file)
            .ok()
            .and_then(|text| parse_cgroup_limit(&text))
        {
            limit = Some(limit.map_or(value, |known| known.min(value)));
        }
        match current.rfind('/') {
            Some(0) | None => return limit,
            Some(cut) => current.truncate(cut),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryWarning {
    /// The manual budget is below what the thumbnail cache needs to be useful.
    BelowMinimum { budget_bytes: u64 },
    /// The budget is more than the RAM that is free right now.
    AboveAvailable { budget_bytes: u64, available_bytes: u64 },
    /// The budget is a large share of the usable RAM; the system may start swapping.
    AboveTotalShare { budget_bytes: u64, usable_bytes: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryPlan {
    pub automatic: bool,
    pub budget_bytes: u64,
    pub thumbnail_cache_bytes: u64,
    pub system: SystemMemory,
    pub warnings: Vec<MemoryWarning>,
}

impl MemoryPlan {
    /// `setting_mb` is the persisted value: `0` = automatic.
    pub fn resolve(setting_mb: u32, system: SystemMemory) -> Self {
        let usable = system.usable_total_bytes();
        let automatic = setting_mb == 0;
        let budget_bytes = if automatic {
            percent_of(usable, AUTO_BUDGET_PERCENT)
        } else {
            u64::from(setting_mb) * BYTES_PER_MB
        };
        let mut thumbnail_cache_bytes =
            percent_of(budget_bytes, THUMBNAIL_CACHE_PERCENT).max(MIN_THUMBNAIL_CACHE_BYTES);
        if automatic {
            thumbnail_cache_bytes = thumbnail_cache_bytes.min(AUTO_THUMBNAIL_CACHE_CAP_BYTES);
        }

        let mut warnings = Vec::new();
        if !automatic {
            if budget_bytes < 2 * MIN_THUMBNAIL_CACHE_BYTES {
                warnings.push(MemoryWarning::BelowMinimum { budget_bytes });
            }
            if system.available_bytes > 0 && budget_bytes > system.available_bytes {
                warnings.push(MemoryWarning::AboveAvailable {
                    budget_bytes,
                    available_bytes: system.available_bytes,
                });
            }
            if usable > 0 && budget_bytes > percent_of(usable, WARN_TOTAL_PERCENT) {
                warnings.push(MemoryWarning::AboveTotalShare {
                    budget_bytes,
                    usable_bytes: usable,
                });
            }
        }
        Self {
            automatic,
            budget_bytes,
            thumbnail_cache_bytes,
            system,
            warnings,
        }
    }

    /// One line for the settings page and the log.
    pub fn summary(&self) -> String {
        format!(
            "{} RAM detected ({} available). Budget {}{}; decoded thumbnails up to {}.",
            format_bytes(self.system.usable_total_bytes()),
            format_bytes(self.system.available_bytes),
            format_bytes(self.budget_bytes),
            if self.automatic {
                " (automatic: half of the RAM)"
            } else {
                ""
            },
            format_bytes(self.thumbnail_cache_bytes),
        )
    }

    /// The warnings as text for the settings page; empty without warnings.
    pub fn warning_text(&self) -> String {
        self.warnings
            .iter()
            .map(|warning| match warning {
                MemoryWarning::BelowMinimum { budget_bytes } => format!(
                    "{} is very little: the thumbnail cache cannot hold a screenful of tiles and scrolling will reload them constantly.",
                    format_bytes(*budget_bytes)
                ),
                MemoryWarning::AboveAvailable { budget_bytes, available_bytes } => format!(
                    "The budget ({}) is more than the RAM that is free right now ({}). Other programs may be pushed out of memory.",
                    format_bytes(*budget_bytes),
                    format_bytes(*available_bytes)
                ),
                MemoryWarning::AboveTotalShare { budget_bytes, usable_bytes } => format!(
                    "The budget ({}) is more than {}% of the usable RAM ({}). The system may start swapping.",
                    format_bytes(*budget_bytes),
                    WARN_TOTAL_PERCENT,
                    format_bytes(*usable_bytes)
                ),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `value * percent / 100` without losing the remainder of an early division.
fn percent_of(value: u64, percent: u64) -> u64 {
    value.saturating_mul(percent) / 100
}

fn format_bytes(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    if bytes as f64 >= GB {
        format!("{:.1} GB", bytes as f64 / GB)
    } else {
        format!("{} MB", bytes / BYTES_PER_MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_machine_reports_its_memory() {
        let memory = SystemMemory::read();
        assert!(memory.total_bytes > 0);
        assert!(memory.available_bytes > 0);
        assert!(memory.usable_total_bytes() > 0);
    }

    #[test]
    fn a_cgroup_limit_caps_the_installed_ram() {
        let memory = SystemMemory {
            total_bytes: 16 * 1024 * BYTES_PER_MB,
            available_bytes: 8 * 1024 * BYTES_PER_MB,
            cgroup_limit_bytes: Some(2 * 1024 * BYTES_PER_MB),
        };
        assert_eq!(memory.usable_total_bytes(), 2 * 1024 * BYTES_PER_MB);
    }

    #[test]
    fn cgroup_limit_text_is_a_number_or_max() {
        assert_eq!(parse_cgroup_limit("1073741824\n"), Some(1_073_741_824));
        assert_eq!(parse_cgroup_limit("max\n"), None);
    }
}
