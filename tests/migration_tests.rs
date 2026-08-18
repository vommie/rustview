use rusqlite::Connection;
use rustview::library::Library;
use rustview::settings::{LogObscurer, SettingsManager, DEFAULT_OBFUSCATE_LOGS};
use rustview::xnviewdb_migrate::cli::run_cli;
use rustview::xnviewdb_migrate::logger::MigrationLogger;
use rustview::xnviewdb_migrate::mapper::XnViewMapper;
use rustview::xnviewdb_migrate::reader::XnViewDbReader;
use rustview::xnviewdb_migrate::stats::MigrationStats;
use rustview::xnviewdb_migrate::types::XnViewTag;
use rustview::xnviewdb_migrate::writer::XnViewDbWriter;
use std::fs;
use tempfile::tempdir;

fn setup_mock_xnview_db(
    db_path: &std::path::Path,
    root_folder: &std::path::Path,
    sub_folder: &std::path::Path,
    outside_folder: &std::path::Path,
) {
    let conn = Connection::open(db_path).unwrap();

    // 1. Create XnViewMP schema tables
    conn.execute_batch(
        r#"
        CREATE TABLE DBInfo(Version VARCHAR(16), Sub INTEGER DEFAULT 0);
        INSERT INTO DBInfo VALUES ('2.08', 1);

        CREATE TABLE Folders(FolderID INTEGER PRIMARY KEY AUTOINCREMENT, Pathname TEXT NOT NULL UNIQUE);
        CREATE TABLE Hashes(HashID INTEGER PRIMARY KEY AUTOINCREMENT, Hash TEXT, DateTime TEXT NOT NULL);
        CREATE TABLE Images(ImageID INTEGER PRIMARY KEY AUTOINCREMENT, FolderID INTEGER NOT NULL REFERENCES Folders(FolderID), Filename TEXT NOT NULL, Size INTEGER, ModifiedDate DATE, Rating INTEGER DEFAULT 0, Color INTEGER DEFAULT 0, Meta BLOB, HashID INTEGER);
        CREATE TABLE ImagesInfo(ImageID INTEGER PRIMARY KEY REFERENCES Images(ImageID), Width INTEGER, Height INTEGER, Orient INTEGER, Format TEXT, Depth INTEGER, Duration INTEGER DEFAULT 0, Ratio INTEGER DEFAULT 0, ColorProfile TEXT, Bits INTEGER DEFAULT 0);
        CREATE TABLE Tags(TagID INTEGER PRIMARY KEY AUTOINCREMENT, Label CHAR(64), ParentID INTEGER, ID INTEGER, Hidden INTEGER DEFAULT 0, Description CHAR(96), Shortcut CHAR(32));
        CREATE TABLE TagsTree(ImageID INTEGER NOT NULL, TagID INTEGER NOT NULL);
        "#,
    ).unwrap();

    // 2. Create sample physical files on disk
    let img1_path = root_folder.join("root_photo.jpg");
    std::fs::write(&img1_path, b"root photo content").unwrap();

    let img2_path = sub_folder.join("sub_video.mp4");
    std::fs::write(&img2_path, b"sub video content").unwrap();

    let outside_img = outside_folder.join("outside_photo.jpg");
    std::fs::write(&outside_img, b"outside photo content").unwrap();

    // 3. Insert Folders (Folder 1 = Root, Folder 2 = Subfolder, Folder 3 = Outside Root)
    let f1_str = format!("{}/", root_folder.to_string_lossy());
    let f2_str = format!("{}/", sub_folder.to_string_lossy());
    let f3_str = format!("{}/", outside_folder.to_string_lossy());

    conn.execute("INSERT INTO Folders (FolderID, Pathname) VALUES (1, ?1)", [&f1_str]).unwrap();
    conn.execute("INSERT INTO Folders (FolderID, Pathname) VALUES (2, ?1)", [&f2_str]).unwrap();
    conn.execute("INSERT INTO Folders (FolderID, Pathname) VALUES (3, ?1)", [&f3_str]).unwrap();

    // 4. Insert Hashes
    conn.execute(
        "INSERT INTO Hashes (HashID, Hash, DateTime) VALUES (10, 'd5dd11d265f453dee9d51a40ba717efb', '2026-07-27 09:31:35')",
        [],
    ).unwrap();

    // 5. Insert Images
    conn.execute(
        "INSERT INTO Images (ImageID, FolderID, Filename, Size, ModifiedDate, Rating, Color, HashID) VALUES (101, 1, 'root_photo.jpg', 1024, 1700000000, 5, 0, 10)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO Images (ImageID, FolderID, Filename, Size, ModifiedDate, Rating, Color, HashID) VALUES (102, 2, 'sub_video.mp4', 2048, 1700000100, 4, 0, NULL)",
        [],
    ).unwrap();
    conn.execute(
        "INSERT INTO Images (ImageID, FolderID, Filename, Size, ModifiedDate, Rating, Color, HashID) VALUES (103, 3, 'outside_photo.jpg', 3072, 1700000200, 1, 0, NULL)",
        [],
    ).unwrap();

    // 6. Insert ImagesInfo
    conn.execute("INSERT INTO ImagesInfo (ImageID, Width, Height, Format, Duration) VALUES (101, 1920, 1080, 'JPEG', 0)", []).unwrap();
    conn.execute("INSERT INTO ImagesInfo (ImageID, Width, Height, Format, Duration) VALUES (102, 3840, 2160, 'MP4', 10000)", []).unwrap();
    conn.execute("INSERT INTO ImagesInfo (ImageID, Width, Height, Format, Duration) VALUES (103, 800, 600, 'JPEG', 0)", []).unwrap();

    // 7. Insert Tags
    conn.execute("INSERT INTO Tags (TagID, Label, ParentID) VALUES (1, 'Favorites', -1)", []).unwrap();
    conn.execute("INSERT INTO Tags (TagID, Label, ParentID) VALUES (2, 'Work', -1)", []).unwrap();

    // 8. Insert Tag Links
    conn.execute("INSERT INTO TagsTree (ImageID, TagID) VALUES (101, 1)", []).unwrap();
    conn.execute("INSERT INTO TagsTree (ImageID, TagID) VALUES (103, 2)", []).unwrap();
}

#[test]
fn test_migration_filtered_by_root_folder() {
    let tmp_src = tempdir().unwrap();
    let tmp_root = tempdir().unwrap();
    let sub_folder = tmp_root.path().join("vacation");
    std::fs::create_dir_all(&sub_folder).unwrap();

    let tmp_outside = tempdir().unwrap();
    let tmp_target = tempdir().unwrap();

    let sqlite_db_file = tmp_src.path().join("XnView.db");
    setup_mock_xnview_db(&sqlite_db_file, tmp_root.path(), &sub_folder, tmp_outside.path());

    // 1. Read SQLite
    let raw = XnViewDbReader::read_database(&sqlite_db_file).expect("Failed to read SQLite db");
    assert_eq!(raw.folders.len(), 3);
    assert_eq!(raw.images.len(), 3);

    let log_file_path = tmp_target.path().join("root_filter_audit.log");
    let mut logger = MigrationLogger::new(true, Some(log_file_path.clone())).unwrap();

    // 2. Map Data enforcing root filter = tmp_root.path()
    let mut stats = MigrationStats::new(
        &sqlite_db_file.to_string_lossy(),
        true,
        Some(&tmp_root.path().to_string_lossy()),
    );
    let mapped = XnViewMapper::map_data(
        raw,
        true,
        Some(tmp_root.path()),
        &mut stats,
        &mut logger,
    );

    // Verify filter results:
    // • Outside folder was skipped
    assert_eq!(stats.skipped_folders_outside_root, 1);
    assert_eq!(stats.skipped_media_outside_root, 1);

    // • Exactly 2 folders migrated (Root + Vacation subfolder)
    assert_eq!(mapped.folders.len(), 2);
    assert_eq!(mapped.folders[0].id, 1);
    assert_eq!(mapped.folders[1].id, 2);

    // • Exactly 2 media items migrated
    assert_eq!(mapped.media.len(), 2);
    assert_eq!(mapped.media[0].id, 1);
    assert_eq!(mapped.media[0].rating, 5);
    assert_eq!(mapped.media[1].id, 2);
    assert_eq!(mapped.media[1].rating, 4);

    // 3. Write to LMDB
    XnViewDbWriter::wipe_and_write(tmp_target.path(), mapped, &mut stats).expect("Write failed");
    logger.log_summary(&stats);

    // 4. Verify LMDB Database
    let lib = Library::open(&tmp_target.path().join("library.lmdb")).unwrap();
    let folders = lib.folders().list_folders().unwrap();
    assert_eq!(folders.len(), 2);

    let media_root = lib.media().get_by_folder(1).unwrap();
    assert_eq!(media_root.len(), 1);
    assert_eq!(media_root[0].filename, "root_photo.jpg");

    let media_sub = lib.media().get_by_folder(2).unwrap();
    assert_eq!(media_sub.len(), 1);
    assert_eq!(media_sub[0].filename, "sub_video.mp4");

    // 5. Verify Tag Tree is present in audit log
    let log_content = fs::read_to_string(&log_file_path).unwrap();
    assert!(log_content.contains("MAPPED TAG TREE HIERARCHY (XnViewMP -> RustView):"));
    assert!(log_content.contains("├── [#1 -> #1] Favorites") || log_content.contains("└── [#1 -> #1] Favorites"));
    assert!(log_content.contains("├── [#2 -> #2] Work") || log_content.contains("└── [#2 -> #2] Work"));
}

#[test]
fn test_cli_explicit_root_flag_and_settings_persistence() {
    let tmp_src = tempdir().unwrap();
    let tmp_root = tempdir().unwrap();
    let sub_folder = tmp_root.path().join("vacation");
    std::fs::create_dir_all(&sub_folder).unwrap();

    let tmp_outside = tempdir().unwrap();
    let tmp_target = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let sqlite_db_file = tmp_src.path().join("XnView.db");
    setup_mock_xnview_db(&sqlite_db_file, tmp_root.path(), &sub_folder, tmp_outside.path());

    // Run CLI with `--root <PATH>` flag and `--plain-logs`
    let args = vec![
        "rustview".to_string(),
        "--migrate-xnview".to_string(),
        sqlite_db_file.to_string_lossy().to_string(),
        "--root".to_string(),
        tmp_root.path().to_string_lossy().to_string(),
        "--plain-logs".to_string(),
        "--yes".to_string(),
    ];

    let res = run_cli(&args, tmp_target.path(), tmp_config.path());
    assert!(res.is_ok());

    // Verify settings.json was updated with the root folder
    let settings = SettingsManager::new(tmp_config.path());
    assert_eq!(
        settings.get().general.root_folder.as_deref(),
        Some(tmp_root.path())
    );

    // Verify migrated database contains only the filtered subtree
    let lib = Library::open(&tmp_target.path().join("library.lmdb")).unwrap();
    let folders = lib.folders().list_folders().unwrap();
    assert_eq!(folders.len(), 2);
}

#[test]
fn test_migration_with_log_obfuscation_flag() {
    let tmp_src = tempdir().unwrap();
    let tmp_root = tempdir().unwrap();
    let sub_folder = tmp_root.path().join("secret_vacation");
    std::fs::create_dir_all(&sub_folder).unwrap();

    let tmp_outside = tempdir().unwrap();
    let tmp_target = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let sqlite_db_file = tmp_src.path().join("XnView.db");
    setup_mock_xnview_db(&sqlite_db_file, tmp_root.path(), &sub_folder, tmp_outside.path());

    let custom_log_path = tmp_target.path().join("obfuscated_migration.log");

    // Run CLI with `--obfuscate-logs` flag
    let args = vec![
        "rustview".to_string(),
        "--migrate-xnview".to_string(),
        sqlite_db_file.to_string_lossy().to_string(),
        "--root".to_string(),
        tmp_root.path().to_string_lossy().to_string(),
        "--log-file".to_string(),
        custom_log_path.to_string_lossy().to_string(),
        "--obfuscate-logs".to_string(),
        "--yes".to_string(),
    ];

    let res = run_cli(&args, tmp_target.path(), tmp_config.path());
    assert!(res.is_ok());

    // Check log file content: filenames and path components must be masked with [File:...] / [Path:...]
    let log_content = fs::read_to_string(&custom_log_path).expect("Failed to read audit log");
    assert!(!log_content.contains("root_photo.jpg"));
    assert!(!log_content.contains("secret_vacation"));
    assert!(log_content.contains("Media #1"));
    assert!(log_content.contains("Folder #1"));

    // Verify Tag Tree in obfuscated mode: Tag names must be masked as Tag#<new_id>
    assert!(log_content.contains("MAPPED TAG TREE HIERARCHY (XnViewMP -> RustView):"));
    assert!(log_content.contains("[#1 -> #1] Tag#1"));
    assert!(log_content.contains("[#2 -> #2] Tag#2"));
    assert!(!log_content.contains("Favorites"));
    assert!(!log_content.contains("Work"));

    // Cleanup: restore SPoT default state
    LogObscurer::set_obfuscate(DEFAULT_OBFUSCATE_LOGS);
}

#[test]
fn test_migration_hierarchical_tag_tree_audit_logging() {
    let tmp_target = tempdir().unwrap();
    let log_file_path = tmp_target.path().join("tag_tree_audit.log");

    // Build multi-level hierarchical raw tags
    let raw_tags = vec![
        XnViewTag {
            tag_id: 10,
            label: "Nature".to_string(),
            parent_id: None,
        },
        XnViewTag {
            tag_id: 20,
            label: "Animals".to_string(),
            parent_id: Some(10),
        },
        XnViewTag {
            tag_id: 30,
            label: "Mammals".to_string(),
            parent_id: Some(20),
        },
        XnViewTag {
            tag_id: 40,
            label: "Dogs".to_string(),
            parent_id: Some(30),
        },
        XnViewTag {
            tag_id: 25,
            label: "Plants".to_string(),
            parent_id: Some(10),
        },
        XnViewTag {
            tag_id: 50,
            label: "Events".to_string(),
            parent_id: None,
        },
        XnViewTag {
            tag_id: 60,
            label: "Holidays".to_string(),
            parent_id: Some(50),
        },
    ];

    let raw_data = rustview::xnviewdb_migrate::types::XnViewRawData {
        tags: raw_tags,
        ..Default::default()
    };

    let mut logger = MigrationLogger::new(true, Some(log_file_path.clone())).unwrap();
    let mut stats = MigrationStats::default();

    LogObscurer::set_obfuscate(false);
    let mapped = XnViewMapper::map_data(raw_data, true, None, &mut stats, &mut logger);

    assert_eq!(mapped.tags.len(), 7);
    drop(logger);

    let log_content = fs::read_to_string(&log_file_path).unwrap();
    assert!(log_content.contains("MAPPED TAG TREE HIERARCHY (XnViewMP -> RustView):"));
    assert!(log_content.contains("├── [#10 -> #1] Nature"));
    assert!(log_content.contains("│   ├── [#20 -> #2] Animals"));
    assert!(log_content.contains("│   │   └── [#30 -> #3] Mammals"));
    assert!(log_content.contains("│   │       └── [#40 -> #4] Dogs"));
    assert!(log_content.contains("│   └── [#25 -> #5] Plants"));
    assert!(log_content.contains("└── [#50 -> #6] Events"));
    assert!(log_content.contains("    └── [#60 -> #7] Holidays"));
}

#[test]
fn test_migration_empty_tags_audit_logging() {
    let tmp_target = tempdir().unwrap();
    let log_file_path = tmp_target.path().join("empty_tags_audit.log");

    let raw_data = rustview::xnviewdb_migrate::types::XnViewRawData {
        tags: Vec::new(),
        ..Default::default()
    };

    let mut logger = MigrationLogger::new(true, Some(log_file_path.clone())).unwrap();
    let mut stats = MigrationStats::default();

    let mapped = XnViewMapper::map_data(raw_data, true, None, &mut stats, &mut logger);
    assert_eq!(mapped.tags.len(), 0);
    drop(logger);

    let log_content = fs::read_to_string(&log_file_path).unwrap();
    assert!(log_content.contains("MAPPED TAG TREE HIERARCHY (XnViewMP -> RustView):"));
    assert!(log_content.contains("<No tags found in XnViewMP database>"));
}
