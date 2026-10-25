//! Turns the checks of `rustview_core::diagnostics` into what the Debug page shows (`DebugUi`).

use crate::ui::{DebugCheckGroupView, DebugCheckView, DebugUi, SettingsWindow};
use rustview_core::diagnostics::{Check, CheckReport, Group, Status};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

/// The settings page a check may point to: (id of the check's link, index in the sidebar, name).
const SETTINGS_PAGES: [(&str, i32, &str); 1] = [("performance", 3, "Performance & Cache")];

pub(super) fn status_index(status: Status) -> i32 {
    match status {
        Status::Ok => 0,
        Status::Info => 1,
        Status::Warning => 2,
        Status::Problem => 3,
    }
}

pub(super) fn check_view(check: &Check) -> DebugCheckView {
    let page = check
        .settings_page
        .and_then(|id| SETTINGS_PAGES.iter().find(|(page_id, _, _)| *page_id == id));
    let lines: Vec<SharedString> = check
        .fix
        .iter()
        .flat_map(rustview_core::diagnostics::Fix::lines)
        .map(SharedString::from)
        .collect();
    DebugCheckView {
        id: check.id.into(),
        status: status_index(check.status),
        title: check.title.as_str().into(),
        detail: check.detail.as_str().into(),
        fix_lines: ModelRc::new(VecModel::from(lines)),
        settings_page: page.map_or(-1, |(_, index, _)| *index),
        settings_page_name: page.map_or("", |(_, _, name)| *name).into(),
    }
}

fn model(checks: &[&Check]) -> ModelRc<DebugCheckView> {
    ModelRc::new(VecModel::from(checks.iter().map(|c| check_view(c)).collect::<Vec<_>>()))
}

/// The header line of a card: what needs attention, or that everything passed.
pub(super) fn summary(checks: &[&Check]) -> (String, bool) {
    let count = |status: Status| checks.iter().filter(|c| c.status == status).count();
    let (problems, warnings) = (count(Status::Problem), count(Status::Warning));
    // Information with a command to run (a developer tool that is missing).
    let hints = checks
        .iter()
        .filter(|c| c.status == Status::Info && c.fix.is_some())
        .count();
    let plural = |n: usize, word: &str| {
        if n == 1 {
            format!("1 {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    match (problems, warnings) {
        (0, 0) if checks.is_empty() => (String::new(), false),
        (0, 0) if hints > 0 => (plural(hints, "hint"), false),
        (0, 0) => (format!("all {} fine", checks.len()), false),
        (0, w) => (plural(w, "warning"), true),
        (p, 0) => (plural(p, "problem"), true),
        (p, w) => (format!("{}, {}", plural(p, "problem"), plural(w, "warning")), true),
    }
}

/// The view of one card: attention first (problems before warnings), then what passed.
pub(super) fn group_view(checks: &[&Check], empty_summary: &str) -> DebugCheckGroupView {
    let mut attention: Vec<&Check> = checks.iter().copied().filter(|c| c.status.needs_attention()).collect();
    attention.sort_by_key(|check| std::cmp::Reverse(check.status));
    let passed: Vec<&Check> = checks.iter().copied().filter(|c| !c.status.needs_attention()).collect();
    let (text, alarming) = summary(checks);
    DebugCheckGroupView {
        attention: model(&attention),
        passed: model(&passed),
        summary: if text.is_empty() {
            empty_summary.into()
        } else {
            text.into()
        },
        alarming,
    }
}

/// The health checks to show: the last run's, with the ones made from memory replaced by their current state (or only those
/// before the first run).
pub(super) fn health_checks<'a>(report: Option<&'a CheckReport>, in_memory: &'a [Check]) -> Vec<&'a Check> {
    let Some(report) = report else {
        return in_memory.iter().collect();
    };
    let mut checks: Vec<&Check> = report
        .group(Group::Health)
        .filter(|check| !in_memory.iter().any(|fresh| fresh.id == check.id))
        .collect();
    checks.extend(in_memory.iter());
    checks
}

/// Shows the checks on the page (`report` = the last run, `None` before the first one).
pub(super) fn show(window: &SettingsWindow, report: Option<&CheckReport>, in_memory: &[Check]) {
    let ui = window.global::<DebugUi>();
    let empty: Vec<&Check> = Vec::new();
    let (dependencies, developer) = report.map_or_else(
        || (empty.clone(), empty.clone()),
        |r| {
            (
                r.group(Group::Dependencies).collect(),
                r.group(Group::Developer).collect(),
            )
        },
    );
    ui.set_dependencies(group_view(&dependencies, "not checked yet"));
    ui.set_developer(group_view(&developer, ""));
    ui.set_health(group_view(&health_checks(report, in_memory), ""));
    let install = report
        .filter(|r| r.missing_packages(Group::Dependencies).len() > 1)
        .and_then(|r| r.install_all(Group::Dependencies))
        .unwrap_or_default();
    ui.set_install_all(install.into());
    ui.set_checked_at(
        report
            .map(|r| r.finished.format("%H:%M:%S").to_string())
            .unwrap_or_default()
            .into(),
    );
}

/// The self-test's result as rows of the Tools card.
pub(super) fn self_test_rows(checks: &[Check]) -> ModelRc<DebugCheckView> {
    ModelRc::new(VecModel::from(checks.iter().map(check_view).collect::<Vec<_>>()))
}

/// A count for a summary line (`1,284`).
pub(super) fn count(value: usize) -> String {
    rustview_core::telemetry::format_count(value)
}

#[cfg(test)]
mod tests {
    use super::{health_checks, summary};
    use rustview_core::diagnostics::{Check, CheckReport, Group, Status};

    fn check(id: &'static str, status: Status) -> Check {
        Check::new(id, Group::Health, status, id, "")
    }

    #[test]
    fn summaries_count_problems_and_warnings() {
        let a = check("a", Status::Ok);
        let b = check("b", Status::Warning);
        let c = check("c", Status::Problem);
        assert_eq!(summary(&[&a]), ("all 1 fine".to_owned(), false));
        assert_eq!(summary(&[&a, &b]), ("1 warning".to_owned(), true));
        assert_eq!(summary(&[&b, &b, &c]), ("1 problem, 2 warnings".to_owned(), true));
        assert_eq!(summary(&[]), (String::new(), false));
        let hint = check("d", Status::Info).with_fix(rustview_core::diagnostics::Fix::packages(&["samply"]));
        assert_eq!(summary(&[&a, &hint]), ("1 hint".to_owned(), false));
    }

    #[test]
    fn fresh_in_memory_checks_replace_the_old_ones() {
        let report = CheckReport::new(vec![check("library", Status::Ok), check("graphics", Status::Info)]);
        let fresh = vec![check("graphics", Status::Warning)];
        let shown = health_checks(Some(&report), &fresh);
        let ids: Vec<(&str, Status)> = shown.iter().map(|c| (c.id, c.status)).collect();
        assert_eq!(ids, [("library", Status::Ok), ("graphics", Status::Warning)]);
        assert_eq!(health_checks(None, &fresh).len(), 1);
    }
}
