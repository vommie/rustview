use super::error::ThumbnailError;
use super::types::{ThumbnailFormat, ThumbnailLocation, ThumbnailStorageStats};
use crate::library::Library;
use crate::util::cast::{Az, SaturatingAs};
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::FileExt;

#[cfg(windows)]
use std::os::windows::fs::FileExt;

pub const PACK_MAX_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB per binary pack file

struct PackWriter {
    pack_id: u32,
    file: File,
    current_size: u64,
}

pub struct PackStore {
    base_dir: PathBuf,
    writer: Mutex<PackWriter>,
    readers: RwLock<HashMap<u32, Arc<File>>>,
}

/// The id part of a pack file name (`pack-<id>.bin`); `None` for every other file in the cache directory.
fn pack_stem(file_name: &str) -> Option<&str> {
    file_name.strip_prefix("pack-")?.strip_suffix(".bin")
}

impl PackStore {
    pub fn open(base_dir: &Path) -> Result<Self, ThumbnailError> {
        std::fs::create_dir_all(base_dir)?;

        let mut latest_id = 0;
        for entry in std::fs::read_dir(base_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if let Some(stem) = pack_stem(&name)
                && let Ok(id) = stem.parse::<u32>()
            {
                latest_id = latest_id.max(id);
            }
        }

        let pack_path = base_dir.join(format!("pack-{latest_id:06}.bin"));
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&pack_path)?;

        let current_size = file.seek(SeekFrom::End(0))?;

        let writer = PackWriter {
            pack_id: latest_id,
            file,
            current_size,
        };

        Ok(Self {
            base_dir: base_dir.to_path_buf(),
            writer: Mutex::new(writer),
            readers: RwLock::new(HashMap::new()),
        })
    }

    fn pack_path(&self, pack_id: u32) -> PathBuf {
        self.base_dir.join(format!("pack-{pack_id:06}.bin"))
    }

    fn get_or_open_reader(&self, pack_id: u32) -> Result<Arc<File>, ThumbnailError> {
        {
            let readers = self.readers.read();
            if let Some(file) = readers.get(&pack_id) {
                return Ok(file.clone());
            }
        }

        let path = self.pack_path(pack_id);
        let file = Arc::new(File::open(&path)?);
        self.readers.write().insert(pack_id, file.clone());
        Ok(file)
    }

    /// Thread-safe high-throughput append without per-item disk flush lock contention.
    pub fn append(&self, compressed_bytes: &[u8]) -> Result<ThumbnailLocation, ThumbnailError> {
        let len = compressed_bytes.len().saturating_as::<u32>();

        let mut writer = self.writer.lock();

        if writer.current_size + u64::from(len) > PACK_MAX_SIZE {
            writer.pack_id += 1;
            let new_path = self.pack_path(writer.pack_id);
            writer.file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&new_path)?;
            writer.current_size = 0;
            tracing::info!(new_pack_id = writer.pack_id, "Rotated to new Pack container.");
        }

        let offset = writer.current_size;
        writer.file.write_all(compressed_bytes)?;
        writer.current_size += u64::from(len);

        let location = ThumbnailLocation {
            pack_id: writer.pack_id,
            offset,
            length: len,
        };

        Ok(location)
    }

    /// Reads compressed thumbnail slice via high-speed atomic pread syscalls.
    pub fn read(&self, location: ThumbnailLocation) -> Result<Vec<u8>, ThumbnailError> {
        let reader = self.get_or_open_reader(location.pack_id)?;

        let file_len = reader.metadata()?.len();
        let required_end = location.offset + u64::from(location.length);

        if required_end > file_len {
            return Err(ThumbnailError::PackOutOfBounds {
                pack_id: location.pack_id,
                offset: location.offset,
                length: location.length,
            });
        }

        let mut buffer = vec![0u8; location.length as usize];
        read_exact_at(&reader, &mut buffer, location.offset)?;

        // Sniff and validate format header (WebP or JPEG)
        if ThumbnailFormat::detect(&buffer).is_none() {
            return Err(ThumbnailError::Ffmpeg(format!(
                "Corrupted thumbnail header in pack {} at offset {}: magic bytes {:02X}{:02X}",
                location.pack_id,
                location.offset,
                buffer.first().copied().unwrap_or(0),
                buffer.get(1).copied().unwrap_or(0)
            )));
        }

        Ok(buffer)
    }

    /// Calculates the current storage footprint of all binary pack files on disk in bytes.
    pub fn total_storage_bytes(&self) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if pack_stem(&name).is_some()
                    && let Ok(meta) = entry.metadata()
                {
                    total += meta.len();
                }
            }
        }
        total
    }

    /// Counts the total number of binary pack containers currently in the cache directory.
    pub fn pack_file_count(&self) -> usize {
        let mut count = 0;
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if pack_stem(&name).is_some() {
                    count += 1;
                }
            }
        }
        count
    }

    /// Computes full storage metrics across binary pack containers and LMDB database tables.
    pub fn storage_stats(&self, library: &Library) -> ThumbnailStorageStats {
        let pack_total_bytes = self.total_storage_bytes();
        let pack_files_count = self.pack_file_count();
        let pack_total_mb = pack_total_bytes.az::<f64>() / (1024.0 * 1024.0);

        let (lmdb_thumbnails_count, lmdb_previews_count, lmdb_preview_frames_count) = {
            if let Ok(rtxn) = library.raw_db().env.read_txn() {
                let thumb_count = library
                    .raw_db()
                    .thumbnails
                    .len(&rtxn)
                    .unwrap_or(0)
                    .saturating_as::<usize>();
                let prev_count = library
                    .raw_db()
                    .video_previews
                    .len(&rtxn)
                    .unwrap_or(0)
                    .saturating_as::<usize>();
                let mut preview_frames = 0usize;
                if let Ok(iter) = library.raw_db().video_previews.iter(&rtxn) {
                    for entry in iter.flatten() {
                        let (_, rec) = entry;
                        preview_frames += rec.frame_locations.len();
                    }
                }
                (thumb_count, prev_count, preview_frames)
            } else {
                (0, 0, 0)
            }
        };

        // Determine LMDB data file size on disk
        let lmdb_data_bytes = {
            let lmdb_path = library.raw_db().env.path();
            let data_file = lmdb_path.join("data.mdb");
            if let Ok(meta) = std::fs::metadata(&data_file) {
                meta.len()
            } else if let Ok(meta) = std::fs::metadata(lmdb_path) {
                meta.len()
            } else {
                0
            }
        };

        let lmdb_data_mb = lmdb_data_bytes.az::<f64>() / (1024.0 * 1024.0);
        let total_cache_bytes = pack_total_bytes + lmdb_data_bytes;
        let total_cache_mb = total_cache_bytes.az::<f64>() / (1024.0 * 1024.0);

        ThumbnailStorageStats {
            pack_files_count,
            pack_total_bytes,
            pack_total_mb,
            lmdb_thumbnails_count,
            lmdb_previews_count,
            lmdb_preview_frames_count,
            lmdb_data_bytes,
            lmdb_data_mb,
            total_cache_bytes,
            total_cache_mb,
        }
    }
}

fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    {
        file.read_exact_at(buf, offset)
    }
    #[cfg(windows)]
    {
        file.seek_read(buf, offset)?;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        use std::io::Read;
        let mut f = file;
        f.seek(SeekFrom::Start(offset))?;
        f.read_exact(buf)
    }
}
