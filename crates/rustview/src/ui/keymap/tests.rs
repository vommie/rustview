use slint::platform::Key;

use super::*;

fn press(text: &str) -> KeyPress<'_> {
    KeyPress {
        text,
        control: false,
        alt: false,
        shift: false,
        meta: false,
        repeat: false,
    }
}

fn special(key: Key) -> String {
    char::from(key).to_string()
}

fn chord(text: &str) -> Chord {
    Chord::parse(text).unwrap()
}

#[test]
fn the_defaults_parse_and_do_not_conflict() {
    let (keymap, notes) = Keymap::from_table(&KeymapTable::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(keymap.conflicts().is_empty(), "{:?}", keymap.conflicts());
    for &action in Action::ALL {
        let expected = if action.has_label_keys() {
            0
        } else {
            action.default_keys().len()
        };
        assert_eq!(keymap.chords(action).len(), expected, "{action:?}");
        assert_eq!(Action::from_id(action.id()), Some(action));
        assert!(
            action.id().starts_with(&format!("{}.", action.area().id())),
            "{action:?}"
        );
    }
    assert_eq!(keymap, Keymap::default());
}

#[test]
fn ids_are_unique() {
    let mut ids: Vec<&str> = Action::ALL.iter().map(|a| a.id()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), Action::ALL.len());
}

#[test]
fn the_same_key_means_different_things_per_area() {
    let keymap = Keymap::default();
    let f2 = special(Key::F2);
    assert_eq!(
        keymap.lookup(&press(&f2), &[Area::General, Area::Browse]),
        Some(Action::BrowseRename)
    );
    assert_eq!(
        keymap.lookup(&press(&f2), &[Area::General, Area::Tab]),
        Some(Action::TabRename)
    );
    assert_eq!(
        keymap.lookup(&press(&f2), &[Area::Fullscreen]),
        Some(Action::FullscreenRename)
    );
    let backspace = special(Key::Backspace);
    assert_eq!(
        keymap.lookup(&press(&backspace), &[Area::General, Area::Browse]),
        Some(Action::BrowseParent)
    );
    assert_eq!(
        keymap.lookup(&press(&backspace), &[Area::General, Area::Tab]),
        Some(Action::TabZoomReset)
    );
    // Paging and the video player share the arrows; the areas the caller passes decide.
    let left = special(Key::LeftArrow);
    assert_eq!(
        keymap.lookup(&press(&left), &[Area::Fullscreen]),
        Some(Action::FullscreenPrevious)
    );
    assert_eq!(
        keymap.lookup(&press(&left), &[Area::Player]),
        Some(Action::PlayerJumpBack)
    );
    // A tab never pages.
    assert_eq!(keymap.lookup(&press(&left), &[Area::General, Area::Tab]), None);
    assert!(!Action::FullscreenPrevious.excludes(Action::PlayerJumpBack));
    assert!(Action::TabRename.excludes(Action::PlayerMute));
    assert!(!Action::BrowseRename.excludes(Action::TabRename));
}

#[test]
fn modifiers_and_layout_keys_match_like_slint() {
    let keymap = Keymap::default();
    let ctrl = |text| KeyPress {
        control: true,
        ..press(text)
    };
    let areas = [Area::General, Area::Tab];
    assert_eq!(keymap.lookup(&ctrl("s"), &areas), Some(Action::TabSave));
    assert_eq!(
        keymap.lookup(
            &KeyPress {
                shift: true,
                ..ctrl("S")
            },
            &areas
        ),
        Some(Action::TabSaveAs)
    );
    assert_eq!(keymap.lookup(&ctrl("3"), &areas), Some(Action::TabRate3));
    // `+` is Shift+= on a US layout and its own key on a German one: both zoom in.
    assert_eq!(keymap.lookup(&ctrl("+"), &areas), Some(Action::TabZoomIn));
    assert_eq!(
        keymap.lookup(
            &KeyPress {
                shift: true,
                ..ctrl("+")
            },
            &areas
        ),
        Some(Action::TabZoomIn)
    );
    assert_eq!(keymap.lookup(&ctrl("="), &areas), Some(Action::TabZoomIn));
    // The rating keys need Ctrl.
    assert_eq!(keymap.lookup(&press("3"), &areas), None);
    let tab = special(Key::Tab);
    assert_eq!(keymap.lookup(&ctrl(&tab), &areas), Some(Action::NextTab));
    assert_eq!(
        keymap.lookup(
            &KeyPress {
                shift: true,
                ..ctrl(&special(Key::Backtab))
            },
            &areas
        ),
        Some(Action::PreviousTab),
        "Backtab is Shift+Tab"
    );
    // Shift extends the cursor movement of Browse.
    assert_eq!(
        keymap.lookup(
            &KeyPress {
                shift: true,
                ..press(&special(Key::DownArrow))
            },
            &[Area::General, Area::Browse]
        ),
        Some(Action::BrowseCursor)
    );
}

#[test]
fn fixed_keys_stay_and_cannot_be_taken() {
    let mut keymap = Keymap::default();
    assert!(!keymap.unbind(Action::BrowseCursor, "Shift?+UpArrow"));
    let conflict = keymap.bind(Action::BrowseRename, chord("UpArrow")).unwrap_err();
    assert_eq!(conflict.other, Action::BrowseCursor);
    let mut stored = KeymapTable::new();
    stored.insert("browse.move_cursor".into(), Some(Vec::new()));
    let (from_file, notes) = Keymap::from_table(&stored);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(from_file, Keymap::default());
    assert!(!keymap.to_table().contains_key("browse.move_cursor"));
}

#[test]
fn a_stored_table_round_trips_and_is_repaired() {
    let mut keymap = Keymap::default();
    assert!(keymap.unbind(Action::TabRename, "F2"));
    keymap.bind(Action::TabRename, chord("F6")).unwrap();
    let (again, notes) = Keymap::from_table(&keymap.to_table());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(again, keymap);

    let mut broken = KeymapTable::new();
    broken.insert("nothing.here".into(), Some(vec!["F9".into()]));
    broken.insert("tab.save".into(), None);
    broken.insert("tab.undo".into(), Some(vec!["Control+".into(), "F7".into()]));
    // The user's key wins over the default it collides with.
    broken.insert("tab.close".into(), Some(vec!["F2".into()]));
    let (repaired, notes) = Keymap::from_table(&broken);
    assert_eq!(notes.len(), 4, "{notes:?}");
    assert_eq!(repaired.labels(Action::TabSave), vec!["Ctrl+S".to_owned()]);
    assert_eq!(repaired.labels(Action::TabUndo), vec!["F7".to_owned()]);
    assert_eq!(repaired.labels(Action::TabClose), vec!["F2".to_owned()]);
    assert_eq!(repaired.chords(Action::TabRename), []);
    assert_eq!(repaired.conflicts(), Vec::new());
}

#[test]
fn json_values_that_are_not_key_lists_keep_the_defaults() {
    let mut stored = std::collections::BTreeMap::new();
    stored.insert("tab.save".to_owned(), serde_json::json!(["F9"]));
    stored.insert("tab.undo".to_owned(), serde_json::json!("Control+Z"));
    stored.insert("tab.redo".to_owned(), serde_json::json!([1, 2]));
    let table = table_from_json(&stored);
    assert_eq!(table.get("tab.save"), Some(&Some(vec!["F9".to_owned()])));
    assert_eq!(table.get("tab.undo"), Some(&None));
    assert_eq!(table.get("tab.redo"), Some(&None));
    let back = table_to_json(&table);
    assert_eq!(back.len(), 1);
    let (keymap, notes) = Keymap::from_table(&table);
    assert_eq!(notes.len(), 2);
    assert_eq!(keymap.labels(Action::TabUndo), vec!["Ctrl+Z".to_owned()]);
}

#[test]
fn labels_for_menus_and_tooltips() {
    let keymap = Keymap::default();
    assert_eq!(keymap.first_label(Action::BrowseRename), "F2");
    assert_eq!(keymap.tooltip_keys(Action::TabRedo), "Ctrl+Shift+Z or Ctrl+Y");
    assert_eq!(
        keymap.labels(Action::BrowseTypeAhead),
        vec!["A-Z".to_owned(), "0-9".to_owned()]
    );
}

#[test]
fn presses_become_chords() {
    let ctrl_s = KeyPress {
        control: true,
        ..press("s")
    };
    assert_eq!(Chord::of_press(&ctrl_s).unwrap().text(), "Control+S");
    let left = special(Key::LeftArrow);
    let shift_left = KeyPress {
        shift: true,
        ..press(&left)
    };
    assert_eq!(Chord::of_press(&shift_left).unwrap().text(), "Shift+LeftArrow");
    let comma = KeyPress {
        control: true,
        ..press(",")
    };
    assert_eq!(Chord::of_press(&comma).unwrap().text(), "Control+Comma");
    assert_eq!(Chord::of_press(&press(&special(Key::F9))).unwrap().text(), "F9");
    assert_eq!(Chord::of_press(&press("[")).unwrap().text(), "OpenBracket");
    let shift = special(Key::Shift);
    assert_eq!(Chord::of_press(&press(&shift)), None);
    assert_eq!(Chord::of_press(&press("")), None);
    assert!(chord("S").is_plain_printable());
    assert!(chord("Space").is_plain_printable());
    assert!(chord("Comma").is_plain_printable());
    assert!(!chord("F2").is_plain_printable());
    assert!(!chord("Control+S").is_plain_printable());
    assert!(!chord("Return").is_plain_printable());
}

#[test]
fn rating_keys_need_ctrl_and_never_alt_or_meta() {
    let keymap = Keymap::default();
    for (areas, first) in [
        (&[Area::General, Area::Browse][..], Action::BrowseRate5),
        (&[Area::General, Area::Tab][..], Action::TabRate5),
        (&[Area::Fullscreen][..], Action::FullscreenRate5),
    ] {
        for n in 0..=5u8 {
            let digit = n.to_string();
            let with = |control, alt, meta| KeyPress {
                control,
                alt,
                meta,
                ..press(&digit)
            };
            let rated = keymap.lookup(&with(true, false, false), areas);
            assert_eq!(rated.and_then(Action::rating), Some(n), "{areas:?} Ctrl+{n}");
            assert_eq!(rated.map(Action::area), Some(first.area()));
            for other in [
                with(false, false, false),
                with(true, true, false),
                with(true, false, true),
                with(false, true, false),
            ] {
                assert_eq!(
                    keymap.lookup(&other, areas).and_then(Action::rating),
                    None,
                    "{areas:?} {other:?}"
                );
            }
        }
        for digit in ["6", "9"] {
            let ctrl = KeyPress {
                control: true,
                ..press(digit)
            };
            assert_eq!(keymap.lookup(&ctrl, areas), None, "{areas:?} Ctrl+{digit}");
        }
    }
}

#[test]
fn labels_use_the_short_names_of_keyboard_legends() {
    assert_eq!(chord("Return").label(), "Enter");
    assert_eq!(chord("Control+Return").label(), "Ctrl+Enter");
    assert_eq!(chord("Shift+LeftArrow").label(), "Shift+Left");
    assert_eq!(chord("Control+PageDown").label(), "Ctrl+PgDown");
    assert_eq!(chord("Delete").label(), "Del");
    assert_eq!(chord("Escape").label(), "Esc");
    assert_eq!(chord("Control+Plus").label(), "Ctrl++");
    assert_eq!(chord("Control+Shift+Z").label(), "Ctrl+Shift+Z");
    assert_eq!(chord("F2").label(), "F2");
}

#[test]
fn the_video_player_keys_come_first_where_a_video_is() {
    let keymap = Keymap::default();
    let areas = [Area::General, Area::Player, Area::Tab];
    assert_eq!(keymap.lookup(&press("m"), &areas), Some(Action::PlayerMute));
    assert_eq!(keymap.lookup(&press(" "), &areas), Some(Action::PlayerPlayPause));
    let left = special(Key::LeftArrow);
    assert_eq!(
        keymap.lookup(&press(&left), &[Area::Player, Area::Fullscreen]),
        Some(Action::PlayerJumpBack)
    );
    assert_eq!(
        keymap.lookup(&press(&left), &[Area::Fullscreen]),
        Some(Action::FullscreenPrevious)
    );
}
