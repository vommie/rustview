//! The steps of a probe run, read from a plan file: one step per line, `#` starts a comment.
//!
//! ```text
//! ready                      wait until the start is over and nothing works in the background
//! settle 800                 wait 800 ms
//! begin NAME / end           mark a scenario in the log (frames between the marks belong to it)
//! tree                       Refresh Tree (Full), wait until it is done
//! folder NAME [index|nowait] open the folder with that name and wait until its list and thumbnails are shown;
//!                            `index` waits for its scan and thumbnail generation, `nowait` goes on at once
//! view grid|table            switch the Browse view
//! sort COLUMN [stack]        click the header of a column (0 = name), wait until the list is shown again;
//!                            `stack` is a Shift + click (the column becomes the next sort level)
//! scroll PX_PER_S SECONDS    move the list like a drag of the scroll bar (negative: up)
//! wheel NOTCHES_PER_S SECONDS [here]  turn the mouse wheel over the list (negative: up); `here`: where the pointer is
//! jump FRACTION              jump to a place of the list (0 = top, 1 = end), wait until its thumbnails are shown
//! press @ANCHOR              press the left button on an element (see `anchor.rs` for the names)
//! sweep DX DY SECONDS CYCLES move the pressed pointer DX/DY away and back, CYCLES times
//! moveto @ANCHOR SECONDS     move the pointer to an element
//! drag DX DY SECONDS         move the pointer by DX/DY and leave it there
//! release                    release the button
//! rclick @ANCHOR             right click on an element
//! click DX DY                move the pointer by DX/DY and click there (an entry of an open menu)
//! dclick @ANCHOR             double click on an element
//! key NAME                   press and release a key (escape, return, f11, ...)
//! keydown NAME / keyup NAME  hold and let go of a key (control, shift: the clicks and wheel turns between them carry it)
//! bind ACTION KEY            give an action of the keymap a key for this run (`bind fullscreen.crop C`; nothing is saved)
//! window main|fullscreen     where the next pointer and key steps go; `fullscreen` waits until the Multiview fullscreen
//!                            shows its media. Elements of a Multiview: `@tile:N`, `@gutter:N` (the tab view of the main
//!                            window, or the fullscreen)
//! shot NAME                  save a picture of the window next to the log (LOG.NAME.png)
//! quit                       close the application
//! ```

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Grid,
    Table,
}

/// An element of the window a step points at, for example `@column:3` or `@left-resizer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub name: String,
    pub index: i32,
}

/// What a `folder` step waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderWait {
    Shown,
    Indexed,
    Nothing,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Ready,
    Settle(Duration),
    Begin(String),
    End,
    Tree,
    Folder {
        name: String,
        wait: FolderWait,
    },
    View(ViewKind),
    Sort {
        column: i32,
        stacked: bool,
    },
    Scroll {
        px_per_s: f32,
        seconds: f32,
    },
    Wheel {
        notches_per_s: f32,
        seconds: f32,
        here: bool,
    },
    Jump(f32),
    Press(Anchor),
    Sweep {
        dx: f32,
        dy: f32,
        seconds: f32,
        cycles: u32,
    },
    MoveTo {
        anchor: Anchor,
        seconds: f32,
    },
    Drag {
        dx: f32,
        dy: f32,
        seconds: f32,
    },
    Release,
    RightClick(Anchor),
    ClickBy {
        dx: f32,
        dy: f32,
    },
    DoubleClick(Anchor),
    Key(String),
    KeyDown(String),
    KeyUp(String),
    Bind {
        action: String,
        key: String,
    },
    /// `true`: the fullscreen window of the Multiview.
    Window(bool),
    Shot(String),
    Quit,
}

fn number<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    let word = word.ok_or_else(|| format!("{what} is missing"))?;
    word.parse().map_err(|_| format!("{what} is not a number: {word}"))
}

fn positive(word: Option<&str>, what: &str) -> Result<f32, String> {
    let value: f32 = number(word, what)?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(format!("{what} must be above 0: {value}"))
    }
}

fn anchor(word: Option<&str>) -> Result<Anchor, String> {
    let word = word.ok_or("the element is missing (for example @left-resizer)")?;
    let name = word
        .strip_prefix('@')
        .ok_or_else(|| format!("an element starts with @: {word}"))?;
    let (name, index) = match name.split_once(':') {
        Some((name, index)) => (
            name,
            index
                .parse()
                .map_err(|_| format!("the index is not a number: {index}"))?,
        ),
        None => (name, 0),
    };
    if name.is_empty() {
        return Err("the element has no name".to_owned());
    }
    Ok(Anchor {
        name: name.to_owned(),
        index,
    })
}

fn step(line: &str) -> Result<Option<Step>, String> {
    let line = line.split('#').next().unwrap_or_default().trim();
    let mut words = line.split_whitespace();
    let Some(verb) = words.next() else { return Ok(None) };
    let parsed = match verb {
        "ready" => Step::Ready,
        "settle" => Step::Settle(Duration::from_millis(number(words.next(), "the time in ms")?)),
        "begin" => Step::Begin(words.next().ok_or("the scenario name is missing")?.to_owned()),
        "end" => Step::End,
        "tree" => Step::Tree,
        "folder" => {
            let name = words.next().ok_or("the folder name is missing")?.to_owned();
            let wait = match words.next() {
                None => FolderWait::Shown,
                Some("index") => FolderWait::Indexed,
                Some("nowait") => FolderWait::Nothing,
                Some(other) => return Err(format!("unknown folder option: {other}")),
            };
            Step::Folder { name, wait }
        }
        "view" => match words.next() {
            Some("grid") => Step::View(ViewKind::Grid),
            Some("table") => Step::View(ViewKind::Table),
            other => return Err(format!("the view is grid or table, not {}", other.unwrap_or("nothing"))),
        },
        "sort" => {
            let column = number(words.next(), "the column")?;
            let stacked = match words.next() {
                None => false,
                Some("stack") => true,
                Some(other) => return Err(format!("unknown sort option: {other}")),
            };
            Step::Sort { column, stacked }
        }
        "rclick" => Step::RightClick(anchor(words.next())?),
        "click" => Step::ClickBy {
            dx: number(words.next(), "the horizontal distance")?,
            dy: number(words.next(), "the vertical distance")?,
        },
        "scroll" => Step::Scroll {
            px_per_s: number(words.next(), "the speed in px/s")?,
            seconds: positive(words.next(), "the duration in s")?,
        },
        "wheel" => {
            let notches_per_s = number(words.next(), "the speed in notches/s")?;
            let seconds = positive(words.next(), "the duration in s")?;
            let here = match words.next() {
                None => false,
                Some("here") => true,
                Some(other) => return Err(format!("unknown wheel option: {other}")),
            };
            Step::Wheel {
                notches_per_s,
                seconds,
                here,
            }
        }
        "jump" => {
            let fraction: f32 = number(words.next(), "the place (0 to 1)")?;
            if !(0.0..=1.0).contains(&fraction) {
                return Err(format!("the place is between 0 and 1: {fraction}"));
            }
            Step::Jump(fraction)
        }
        "press" => Step::Press(anchor(words.next())?),
        "sweep" => Step::Sweep {
            dx: number(words.next(), "the horizontal distance")?,
            dy: number(words.next(), "the vertical distance")?,
            seconds: positive(words.next(), "the duration in s")?,
            cycles: number::<u32>(words.next(), "the number of cycles")?.max(1),
        },
        "moveto" => Step::MoveTo {
            anchor: anchor(words.next())?,
            seconds: positive(words.next(), "the duration in s")?,
        },
        "drag" => Step::Drag {
            dx: number(words.next(), "the horizontal distance")?,
            dy: number(words.next(), "the vertical distance")?,
            seconds: positive(words.next(), "the duration in s")?,
        },
        "release" => Step::Release,
        "dclick" => Step::DoubleClick(anchor(words.next())?),
        "key" => Step::Key(words.next().ok_or("the key name is missing")?.to_owned()),
        "keydown" => Step::KeyDown(words.next().ok_or("the key name is missing")?.to_owned()),
        "keyup" => Step::KeyUp(words.next().ok_or("the key name is missing")?.to_owned()),
        "bind" => Step::Bind {
            action: words.next().ok_or("the action is missing")?.to_owned(),
            key: words.next().ok_or("the key is missing")?.to_owned(),
        },
        "window" => match words.next() {
            Some("main") => Step::Window(false),
            Some("fullscreen") => Step::Window(true),
            other => {
                return Err(format!(
                    "the window is main or fullscreen, not {}",
                    other.unwrap_or("nothing")
                ));
            }
        },
        "shot" => Step::Shot(words.next().ok_or("the picture's name is missing")?.to_owned()),
        "quit" => Step::Quit,
        other => return Err(format!("unknown step: {other}")),
    };
    if let Some(extra) = words.next() {
        return Err(format!("unexpected word: {extra}"));
    }
    Ok(Some(parsed))
}

/// The steps of `text`; an error names the line.
pub fn parse(text: &str) -> Result<Vec<Step>, String> {
    let mut steps = Vec::new();
    for (number, line) in text.lines().enumerate() {
        match step(line) {
            Ok(Some(parsed)) => steps.push(parsed),
            Ok(None) => {}
            Err(reason) => return Err(format!("line {}: {reason}", number + 1)),
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_is_read_line_by_line() {
        let steps = parse(
            "ready\n# a comment\n\nfolder Flat-5k index\nview table # after the folder\nbegin resize\npress @column:3\n\
             sweep 120 0 2.5 3\nrelease\nend\nscroll -3000 4\nwheel 20 2\njump 0.5\nmoveto @folder-tree 0.5\nkey escape\n\
             settle 250\ntree\nsort 4\nsort 0 stack\nrclick @header:2\nclick 20 60\nshot after-sort\nquit\n",
        )
        .expect("the plan is valid");
        assert_eq!(
            steps,
            vec![
                Step::Ready,
                Step::Folder {
                    name: "Flat-5k".to_owned(),
                    wait: FolderWait::Indexed
                },
                Step::View(ViewKind::Table),
                Step::Begin("resize".to_owned()),
                Step::Press(Anchor {
                    name: "column".to_owned(),
                    index: 3
                }),
                Step::Sweep {
                    dx: 120.0,
                    dy: 0.0,
                    seconds: 2.5,
                    cycles: 3
                },
                Step::Release,
                Step::End,
                Step::Scroll {
                    px_per_s: -3000.0,
                    seconds: 4.0
                },
                Step::Wheel {
                    notches_per_s: 20.0,
                    seconds: 2.0,
                    here: false
                },
                Step::Jump(0.5),
                Step::MoveTo {
                    anchor: Anchor {
                        name: "folder-tree".to_owned(),
                        index: 0
                    },
                    seconds: 0.5
                },
                Step::Key("escape".to_owned()),
                Step::Settle(Duration::from_millis(250)),
                Step::Tree,
                Step::Sort {
                    column: 4,
                    stacked: false
                },
                Step::Sort {
                    column: 0,
                    stacked: true
                },
                Step::RightClick(Anchor {
                    name: "header".to_owned(),
                    index: 2
                }),
                Step::ClickBy { dx: 20.0, dy: 60.0 },
                Step::Shot("after-sort".to_owned()),
                Step::Quit,
            ]
        );
    }

    #[test]
    fn the_steps_of_a_multiview_are_read() {
        let steps = parse(
            "keydown control\nwheel 5 1 here\nkeyup control\nwindow fullscreen\ndclick @tile:1\nwindow main\n\
             bind fullscreen.crop C\n",
        )
        .expect("the plan is valid");
        assert_eq!(
            steps,
            vec![
                Step::KeyDown("control".to_owned()),
                Step::Wheel {
                    notches_per_s: 5.0,
                    seconds: 1.0,
                    here: true
                },
                Step::KeyUp("control".to_owned()),
                Step::Window(true),
                Step::DoubleClick(Anchor {
                    name: "tile".to_owned(),
                    index: 1
                }),
                Step::Window(false),
                Step::Bind {
                    action: "fullscreen.crop".to_owned(),
                    key: "C".to_owned()
                },
            ]
        );
        assert_eq!(
            parse("window tab\n"),
            Err("line 1: the window is main or fullscreen, not tab".to_owned())
        );
    }

    #[test]
    fn a_mistake_names_its_line() {
        assert_eq!(parse("ready\nfly away\n"), Err("line 2: unknown step: fly".to_owned()));
        assert_eq!(
            parse("scroll fast 2\n"),
            Err("line 1: the speed in px/s is not a number: fast".to_owned())
        );
        assert_eq!(
            parse("scroll 100 0\n"),
            Err("line 1: the duration in s must be above 0: 0".to_owned())
        );
        assert_eq!(
            parse("press column\n"),
            Err("line 1: an element starts with @: column".to_owned())
        );
        assert_eq!(
            parse("jump 2\n"),
            Err("line 1: the place is between 0 and 1: 2".to_owned())
        );
        assert_eq!(
            parse("view list\n"),
            Err("line 1: the view is grid or table, not list".to_owned())
        );
        assert_eq!(parse("release now\n"), Err("line 1: unexpected word: now".to_owned()));
        assert_eq!(
            parse("folder A all\n"),
            Err("line 1: unknown folder option: all".to_owned())
        );
    }
}
