//! The checks as text: `rustview check` prints it, the diagnostic report contains it.

use super::{CheckReport, Group, Status};
use std::fmt::Write as _;

const SECTIONS: [(Group, &str); 3] = [
    (Group::Dependencies, "Dependencies"),
    (Group::Health, "Health"),
    (Group::Developer, "Developer"),
];

/// Every check with its mark, detail and fix; `private` hides paths and names in the details.
pub fn render(report: &CheckReport, private: bool) -> String {
    let mut out = String::new();
    for (group, title) in SECTIONS {
        if report.group(group).next().is_none() {
            continue;
        }
        let _ = writeln!(out, "{title}");
        for check in report.group(group) {
            let detail = check.detail_for(private);
            let separator = if detail.is_empty() { "" } else { " - " };
            let _ = writeln!(out, "  [{:<7}] {}{separator}{detail}", check.status.mark(), check.title);
            for line in check.fix.iter().flat_map(super::Fix::lines) {
                let _ = writeln!(out, "              $ {line}");
            }
        }
        if group != Group::Developer
            && report.missing_packages(group).len() > 1
            && let Some(all) = report.install_all(group)
        {
            let _ = writeln!(out, "  Install everything that is missing: {all}");
        }
        out.push('\n');
    }
    let count = |status: Status| {
        report
            .checks
            .iter()
            .filter(|c| c.status == status && c.group != Group::Developer)
            .count()
    };
    let (problems, warnings) = (count(Status::Problem), count(Status::Warning));
    if problems + warnings == 0 {
        out.push_str("Everything is in place.\n");
    } else {
        let _ = writeln!(out, "{problems} problem(s), {warnings} warning(s).");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::diagnostics::{Check, CheckReport, Fix, Group, Status};

    #[test]
    fn marks_fixes_and_the_combined_command() {
        let report = CheckReport::new(vec![
            Check::new("a", Group::Dependencies, Status::Ok, "libmpv", "plays the videos"),
            Check::new("b", Group::Dependencies, Status::Warning, "xclip is missing", "copy")
                .with_fix(Fix::packages(&["xclip"])),
            Check::new(
                "c",
                Group::Dependencies,
                Status::Problem,
                "The desktop portal is missing",
                "dialogs",
            )
            .with_fix(Fix::packages(&["xdg-desktop-portal"])),
            Check::new("d", Group::Health, Status::Ok, "Root", "/media/Secret").with_private_detail("[Path:1]"),
        ]);
        let text = render(&report, true);
        assert!(text.contains("  [ok     ] libmpv - plays the videos\n"), "{text}");
        assert!(text.contains("  [WARNING] xclip is missing - copy\n              $ sudo pacman -S xclip\n"));
        assert!(text.contains("Install everything that is missing: sudo pacman -S --needed xclip xdg-desktop-portal"));
        assert!(text.contains("[Path:1]") && !text.contains("Secret"));
        assert!(text.ends_with("1 problem(s), 1 warning(s).\n"));
        let clean = CheckReport::new(vec![Check::new("a", Group::Health, Status::Info, "X11 session", "")]);
        assert_eq!(
            render(&clean, false),
            "Health\n  [info   ] X11 session\n\nEverything is in place.\n"
        );
    }
}
