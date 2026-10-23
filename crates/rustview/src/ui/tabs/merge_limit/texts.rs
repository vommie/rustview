//! The wording of the merge warning (pure).

/// How many tabs the breakdown names before it says "and N more tabs".
const MAX_ROWS: usize = 6;

/// "1 medium", "3 media".
pub fn media(count: usize) -> String {
    if count == 1 {
        "1 medium".to_string()
    } else {
        format!("{count} media")
    }
}

/// The main text: what is too much and how many media have to leave the tabs before the merge fits.
pub fn message(total: usize, cap: usize, excess: usize) -> String {
    format!(
        "The selected tabs hold {} together, but a Multiview shows at most {cap}.\n\nRemove {} from these tabs first, then merge them again.",
        media(total),
        media(excess),
    )
}

/// One line per tab (title and its media), in tab order, so the user sees where to remove media from.
pub fn breakdown(rows: &[(String, usize)]) -> String {
    let mut lines: Vec<String> = rows
        .iter()
        .take(MAX_ROWS)
        .map(|(title, count)| format!("{title}: {}", media(*count)))
        .collect();
    if rows.len() > MAX_ROWS {
        lines.push(format!("and {} more tabs", rows.len() - MAX_ROWS));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_is_singular_for_one() {
        assert_eq!(media(1), "1 medium");
        assert_eq!(media(0), "0 media");
        assert_eq!(media(3), "3 media");
    }

    #[test]
    fn the_message_names_the_total_the_limit_and_the_excess() {
        let text = message(7, 4, 3);
        assert!(text.contains("7 media"), "{text}");
        assert!(text.contains("at most 4"), "{text}");
        assert!(text.contains("Remove 3 media"), "{text}");
    }

    #[test]
    fn one_medium_too_many_is_worded_in_the_singular() {
        assert!(message(5, 4, 1).contains("Remove 1 medium from"));
    }

    #[test]
    fn the_breakdown_lists_the_tabs_in_order() {
        let rows = vec![("a.jpg".to_string(), 1), ("Multiview".to_string(), 3)];
        assert_eq!(breakdown(&rows), "a.jpg: 1 medium\nMultiview: 3 media");
    }

    #[test]
    fn a_long_breakdown_is_cut_after_six_tabs() {
        let rows: Vec<(String, usize)> = (1..=9).map(|n| (format!("t{n}"), 1)).collect();
        let text = breakdown(&rows);
        assert_eq!(text.lines().count(), 7);
        assert!(text.ends_with("and 3 more tabs"), "{text}");
        assert_eq!(breakdown(&[]), "");
    }
}
