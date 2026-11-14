//! Fills a library with many tags, for tests of the tag tree with hundreds of tags (developers only).
//!
//! usage: `cargo run -p rustview-core --example seed_tags -- LIBRARY_DIR COUNT`
//!
//! `LIBRARY_DIR` is the `library.lmdb` folder of a TEST profile (`$XDG_DATA_HOME/rustview/library.lmdb`); the application must not
//! run on it meanwhile. Every eighth tag is a group at the top level, the others are its children, some of them nested once
//! more. A second run adds nothing: a tag that exists is kept.

use rustview_core::library::{Library, LibraryError};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(library_dir), Some(count)) = (args.next().map(PathBuf::from), args.next()) else {
        return Err("usage: seed_tags LIBRARY_DIR COUNT".into());
    };
    let count: u32 = count.to_string_lossy().parse()?;
    let library = Library::open(&library_dir)?;
    let tags = library.tags();

    let (mut created, mut group, mut last_child) = (0u32, None, None);
    for number in 0..count {
        let (name, parent) = if number % 8 == 0 {
            (format!("Group {:03}", number / 8 + 1), None)
        } else if number % 8 == 5 {
            (format!("Detail {number:04}"), last_child)
        } else {
            (format!("Tag {number:04}"), group)
        };
        match tags.create_tag(&name, parent) {
            Ok(record) => {
                created += 1;
                if number % 8 == 0 {
                    group = Some(record.id);
                } else if number % 8 != 5 {
                    last_child = Some(record.id);
                }
            }
            Err(LibraryError::DuplicateTag(_)) => {}
            Err(other) => return Err(other.into()),
        }
    }
    println!("{created} of {count} tags created in {}", library_dir.display());
    Ok(())
}
