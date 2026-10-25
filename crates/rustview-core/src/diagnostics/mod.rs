//! The checks of Settings > Debug and `rustview check` (`docs/debug_page.md`): what `RustView` needs from the system
//! (dependencies) and how its own data and environment are doing (health).
//!
//! The rules are pure: [`dependencies::evaluate`] and [`health::evaluate`] turn collected facts into [`Check`]s, so every
//! rule is unit tested. Only [`collect`] looks at the system, and only when the user asked for a check.

pub mod collect;
pub mod dependencies;
pub mod health;
pub mod render;

use chrono::{DateTime, Local};

/// How a check turned out, from good to bad (the order sorts problems last).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// In place and working.
    Ok,
    /// Worth knowing, nothing to do (the window system, a missing developer tool).
    Info,
    /// A feature works only partly (a missing optional program, software rendering).
    Warning,
    /// A feature does not work.
    Problem,
}

impl Status {
    /// Warnings and problems are listed first; everything else is "passed".
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Warning | Self::Problem)
    }

    /// The mark of the text report.
    pub fn mark(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Info => "info",
            Self::Warning => "WARNING",
            Self::Problem => "PROBLEM",
        }
    }
}

/// Where a check is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// Programs and libraries `RustView` uses.
    Dependencies,
    /// Tools for profiling `RustView` (collapsed; never a warning).
    Developer,
    /// The state of the library, the settings, the drive and the display.
    Health,
}

/// How to fix a check: packages to install with pacman, and/or commands to run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fix {
    pub packages: Vec<&'static str>,
    pub commands: Vec<String>,
}

impl Fix {
    pub fn packages(packages: &[&'static str]) -> Self {
        Self {
            packages: packages.to_vec(),
            commands: Vec::new(),
        }
    }

    pub fn commands(commands: &[&str]) -> Self {
        Self {
            packages: Vec::new(),
            commands: commands.iter().map(|c| (*c).to_owned()).collect(),
        }
    }

    /// Every command line of the fix: `sudo pacman -S <packages>` first, then the commands.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.packages.is_empty() {
            lines.push(format!("sudo pacman -S {}", self.packages.join(" ")));
        }
        lines.extend(self.commands.iter().cloned());
        lines
    }
}

/// One line of a check list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    /// Stable id (`clipboard`, `library`, ...).
    pub id: &'static str,
    pub group: Group,
    pub status: Status,
    pub title: String,
    /// What it means, with every name shown (the page; the user's own screen is never obfuscated).
    pub detail: String,
    /// The same with paths and names hidden, when the detail has any (the report while "Obfuscate" is on).
    pub private_detail: Option<String>,
    pub fix: Option<Fix>,
    /// A settings page that has more about it (`"performance"`).
    pub settings_page: Option<&'static str>,
}

impl Check {
    pub fn new(
        id: &'static str,
        group: Group,
        status: Status,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            id,
            group,
            status,
            title: title.into(),
            detail: detail.into(),
            private_detail: None,
            fix: None,
            settings_page: None,
        }
    }

    #[must_use]
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fix = Some(fix);
        self
    }

    #[must_use]
    pub fn with_private_detail(mut self, private: impl Into<String>) -> Self {
        self.private_detail = Some(private.into());
        self
    }

    #[must_use]
    pub fn with_settings_page(mut self, page: &'static str) -> Self {
        self.settings_page = Some(page);
        self
    }

    pub fn detail_for(&self, private: bool) -> &str {
        match (&self.private_detail, private) {
            (Some(hidden), true) => hidden,
            _ => &self.detail,
        }
    }
}

/// The result of one run of the checks.
#[derive(Debug, Clone)]
pub struct CheckReport {
    pub checks: Vec<Check>,
    pub finished: DateTime<Local>,
}

impl CheckReport {
    pub fn new(checks: Vec<Check>) -> Self {
        Self {
            checks,
            finished: Local::now(),
        }
    }

    pub fn group(&self, group: Group) -> impl Iterator<Item = &Check> {
        self.checks.iter().filter(move |check| check.group == group)
    }

    /// Warnings and problems of a group, problems first.
    pub fn attention(&self, group: Group) -> Vec<&Check> {
        let mut list: Vec<&Check> = self.group(group).filter(|c| c.status.needs_attention()).collect();
        list.sort_by_key(|check| std::cmp::Reverse(check.status));
        list
    }

    /// The checks of a group that need nothing.
    pub fn passed(&self, group: Group) -> Vec<&Check> {
        self.group(group).filter(|c| !c.status.needs_attention()).collect()
    }

    /// Whether a problem (not a warning) was found outside the developer tools.
    pub fn has_problems(&self) -> bool {
        self.checks
            .iter()
            .any(|c| c.status == Status::Problem && c.group != Group::Developer)
    }

    /// The packages missing in a group, each once, in the order of the checks.
    pub fn missing_packages(&self, group: Group) -> Vec<&'static str> {
        let mut packages: Vec<&'static str> = Vec::new();
        for check in self.group(group).filter(|c| c.status != Status::Ok) {
            for package in check.fix.iter().flat_map(|fix| fix.packages.iter()) {
                if !packages.contains(package) {
                    packages.push(package);
                }
            }
        }
        packages
    }

    /// One command that installs every package missing in a group (`None` when nothing is missing).
    pub fn install_all(&self, group: Group) -> Option<String> {
        let packages = self.missing_packages(group);
        (!packages.is_empty()).then(|| format!("sudo pacman -S --needed {}", packages.join(" ")))
    }
}

#[cfg(test)]
mod tests {
    use super::{Check, CheckReport, Fix, Group, Status};

    fn check(id: &'static str, group: Group, status: Status, packages: &[&'static str]) -> Check {
        let check = Check::new(id, group, status, id, "");
        if packages.is_empty() {
            check
        } else {
            check.with_fix(Fix::packages(packages))
        }
    }

    #[test]
    fn problems_come_first_and_packages_are_collected_once() {
        let report = CheckReport::new(vec![
            check("a", Group::Dependencies, Status::Ok, &[]),
            check("b", Group::Dependencies, Status::Warning, &["xclip"]),
            check(
                "c",
                Group::Dependencies,
                Status::Problem,
                &["xdg-desktop-portal", "xdg-desktop-portal-kde"],
            ),
            check("d", Group::Dependencies, Status::Warning, &["xclip"]),
            check("e", Group::Developer, Status::Info, &["samply"]),
            check("f", Group::Health, Status::Info, &[]),
        ]);
        let ids: Vec<&str> = report.attention(Group::Dependencies).iter().map(|c| c.id).collect();
        assert_eq!(ids, ["c", "b", "d"]);
        assert_eq!(report.passed(Group::Dependencies).len(), 1);
        assert_eq!(
            report.install_all(Group::Dependencies).as_deref(),
            Some("sudo pacman -S --needed xclip xdg-desktop-portal xdg-desktop-portal-kde")
        );
        assert_eq!(
            report.install_all(Group::Developer).as_deref(),
            Some("sudo pacman -S --needed samply")
        );
        assert_eq!(report.install_all(Group::Health), None);
        assert!(report.has_problems());
    }

    #[test]
    fn a_fix_lists_the_pacman_line_first() {
        let fix = Fix {
            packages: vec!["samply"],
            commands: vec!["sudo sysctl kernel.perf_event_paranoid=1".into()],
        };
        assert_eq!(
            fix.lines(),
            ["sudo pacman -S samply", "sudo sysctl kernel.perf_event_paranoid=1"]
        );
    }

    #[test]
    fn the_private_detail_is_used_only_when_asked_for() {
        let check = Check::new("x", Group::Health, Status::Ok, "t", "/media/Secret").with_private_detail("[Path:1]");
        assert_eq!(check.detail_for(false), "/media/Secret");
        assert_eq!(check.detail_for(true), "[Path:1]");
    }
}
