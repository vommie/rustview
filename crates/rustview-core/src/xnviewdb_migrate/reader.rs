use super::error::MigrationError;
use super::types::*;
use crate::settings::LogObscurer;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

pub struct XnViewDbReader;

impl XnViewDbReader {
    /// Connects to the XnViewMP SQLite database in read-only mode and extracts all relevant entities.
    pub fn read_database(db_path: &Path) -> Result<XnViewRawData, MigrationError> {
        if !db_path.exists() {
            return Err(MigrationError::FileNotFound(
                db_path.to_string_lossy().to_string(),
            ));
        }

        let t0 = Instant::now();
        tracing::info!(
            path = %LogObscurer::path(db_path),
            "[XnViewReader] Opening XnViewMP SQLite database in read-only mode..."
        );

        let conn = Connection::open_with_flags(
            db_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;

        // Validate required core tables
        Self::validate_schema(&conn)?;

        // 1. Read DBInfo
        let (version, sub_version) = Self::read_db_info(&conn);
        tracing::info!(
            version = ?version,
            sub_version = ?sub_version,
            "[XnViewReader] Detected XnView database version"
        );

        // 2. Read Folders
        let folders = Self::read_folders(&conn)?;
        tracing::info!(
            count = folders.len(),
            "[XnViewReader] Extracted Folders table records"
        );

        // 3. Read Tags
        let tags = Self::read_tags(&conn)?;
        tracing::info!(
            count = tags.len(),
            "[XnViewReader] Extracted Tags table records"
        );

        // 4. Read Hashes (Optional external table)
        let (hashes, hashes_table_found) = Self::read_hashes(&conn)?;
        if hashes_table_found {
            tracing::info!(
                count = hashes.len(),
                "[XnViewReader] Extracted Hashes table records"
            );
        } else {
            tracing::info!(
                "[XnViewReader] Optional 'Hashes' table not found in SQLite database. Skipping external hashes extraction."
            );
        }

        // 5. Read ImagesInfo
        let image_info = Self::read_images_info(&conn)?;
        tracing::info!(
            count = image_info.len(),
            "[XnViewReader] Extracted ImagesInfo table records"
        );

        // 6. Read Images (Handles optional HashID column gracefully)
        let images = Self::read_images(&conn)?;
        tracing::info!(
            count = images.len(),
            "[XnViewReader] Extracted Images table records"
        );

        // 7. Read TagsTree (Links between images and tags)
        let tag_links = Self::read_tag_links(&conn)?;
        tracing::info!(
            count = tag_links.len(),
            "[XnViewReader] Extracted TagsTree association records"
        );

        tracing::info!(
            elapsed_ms = t0.elapsed().as_millis(),
            "[XnViewReader] Complete XnView database extraction finished successfully"
        );

        Ok(XnViewRawData {
            version,
            sub_version,
            folders,
            tags,
            hashes,
            hashes_table_found,
            images,
            image_info,
            tag_links,
        })
    }

    fn validate_schema(conn: &Connection) -> Result<(), MigrationError> {
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type='table'")?;
        let table_names: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let required = ["Folders", "Images", "Tags"];
        for req in required {
            if !table_names.iter().any(|t| t.eq_ignore_ascii_case(req)) {
                return Err(MigrationError::InvalidDatabase(format!(
                    "Required table '{}' not found in SQLite database",
                    req
                )));
            }
        }
        Ok(())
    }

    fn read_db_info(conn: &Connection) -> (Option<String>, Option<i64>) {
        if let Ok(mut stmt) = conn.prepare("SELECT Version, Sub FROM DBInfo LIMIT 1")
            && let Ok(row) = stmt.query_row([], |r| {
                let v: Option<String> = r.get(0).ok();
                let s: Option<i64> = r.get(1).ok();
                Ok((v, s))
            })
        {
            return row;
        }
        (None, None)
    }

    fn read_folders(conn: &Connection) -> Result<Vec<XnViewFolder>, MigrationError> {
        let mut stmt = conn.prepare("SELECT FolderID, Pathname FROM Folders ORDER BY FolderID ASC")?;
        let rows = stmt.query_map([], |row| {
            Ok(XnViewFolder {
                folder_id: row.get(0)?,
                pathname: row.get(1)?,
            })
        })?;

        let mut folders = Vec::new();
        for r in rows {
            folders.push(r?);
        }
        Ok(folders)
    }

    fn read_tags(conn: &Connection) -> Result<Vec<XnViewTag>, MigrationError> {
        let mut stmt = conn.prepare("SELECT TagID, Label, ParentID FROM Tags ORDER BY TagID ASC")?;
        let rows = stmt.query_map([], |row| {
            let tag_id: i64 = row.get(0)?;
            let label: String = row.get(1)?;
            let parent_id_raw: Option<i64> = row.get(2).ok();
            let parent_id = match parent_id_raw {
                Some(p) if p > 0 => Some(p),
                _ => None, // -1, 0, or NULL are Root Tags
            };

            Ok(XnViewTag {
                tag_id,
                label,
                parent_id,
            })
        })?;

        let mut tags = Vec::new();
        for r in rows {
            tags.push(r?);
        }
        Ok(tags)
    }

    fn read_hashes(conn: &Connection) -> Result<(Vec<XnViewHash>, bool), MigrationError> {
        let table_exists: bool = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='Hashes' COLLATE NOCASE LIMIT 1",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);

        if !table_exists {
            return Ok((Vec::new(), false));
        }

        let mut stmt = conn.prepare("SELECT HashID, Hash, DateTime FROM Hashes ORDER BY HashID ASC")?;
        let rows = stmt.query_map([], |row| {
            let hash_id: i64 = row.get(0)?;
            let hash: Option<String> = row.get(1).ok();
            let date_time: Option<String> = row.get(2).ok();

            Ok(XnViewHash {
                hash_id,
                hash: hash.unwrap_or_default(),
                date_time: date_time.unwrap_or_default(),
            })
        })?;

        let mut hashes = Vec::new();
        for r in rows {
            hashes.push(r?);
        }
        Ok((hashes, true))
    }

    fn read_images_info(
        conn: &Connection,
    ) -> Result<HashMap<i64, XnViewImageInfo>, MigrationError> {
        let mut stmt = conn.prepare(
            "SELECT ImageID, Width, Height, Format, Duration FROM ImagesInfo",
        )?;
        let rows = stmt.query_map([], |row| {
            let image_id: i64 = row.get(0)?;
            let width: i64 = row.get(1).unwrap_or(0);
            let height: i64 = row.get(2).unwrap_or(0);
            let format: String = row.get(3).unwrap_or_default();
            let duration: i64 = row.get(4).unwrap_or(0);

            Ok((
                image_id,
                XnViewImageInfo {
                    image_id,
                    width: width.max(0) as u32,
                    height: height.max(0) as u32,
                    format,
                    duration: duration.max(0) as u64,
                },
            ))
        })?;

        let mut map = HashMap::new();
        for r in rows {
            let (id, info) = r?;
            map.insert(id, info);
        }
        Ok(map)
    }

    fn read_images(conn: &Connection) -> Result<Vec<XnViewImage>, MigrationError> {
        // Inspect Images table columns to check if HashID column exists
        let mut pragma_stmt = conn.prepare("PRAGMA table_info(Images)")?;
        let cols: Vec<String> = pragma_stmt
            .query_map([], |row| row.get(1))?
            .filter_map(|r| r.ok())
            .collect();

        let has_hash_id = cols.iter().any(|c| c.eq_ignore_ascii_case("HashID"));

        let query = if has_hash_id {
            "SELECT ImageID, FolderID, Filename, Size, ModifiedDate, Rating, Color, HashID FROM Images ORDER BY ImageID ASC"
        } else {
            "SELECT ImageID, FolderID, Filename, Size, ModifiedDate, Rating, Color, NULL AS HashID FROM Images ORDER BY ImageID ASC"
        };

        let mut stmt = conn.prepare(query)?;

        let rows = stmt.query_map([], |row| {
            let image_id: i64 = row.get(0)?;
            let folder_id: i64 = row.get(1)?;
            let filename: String = row.get(2)?;
            let size: i64 = row.get(3).unwrap_or(0);
            let modified_date: i64 = row.get(4).unwrap_or(0);
            let rating: i64 = row.get(5).unwrap_or(0);
            let color: i64 = row.get(6).unwrap_or(0);

            let hash_id_val: rusqlite::types::Value = row.get(7).unwrap_or(rusqlite::types::Value::Null);
            let hash_id = match hash_id_val {
                rusqlite::types::Value::Integer(i) if i > 0 => Some(i),
                rusqlite::types::Value::Text(s) => s.parse::<i64>().ok().filter(|&i| i > 0),
                _ => None,
            };

            Ok(XnViewImage {
                image_id,
                folder_id,
                filename,
                size,
                modified_date,
                rating,
                color,
                hash_id,
            })
        })?;

        let mut images = Vec::new();
        for r in rows {
            images.push(r?);
        }
        Ok(images)
    }

    fn read_tag_links(conn: &Connection) -> Result<Vec<XnViewTagLink>, MigrationError> {
        let mut stmt = conn.prepare("SELECT ImageID, TagID FROM TagsTree")?;
        let rows = stmt.query_map([], |row| {
            Ok(XnViewTagLink {
                image_id: row.get(0)?,
                tag_id: row.get(1)?,
            })
        })?;

        let mut links = Vec::new();
        for r in rows {
            links.push(r?);
        }
        Ok(links)
    }
}
