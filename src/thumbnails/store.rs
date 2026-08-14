use super::error::ThumbnailError;
use super::types::ThumbnailLocation;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

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

impl PackStore {
    pub fn open(base_dir: &Path) -> Result<Self, ThumbnailError> {
        std::fs::create_dir_all(base_dir)?;

        let mut latest_id = 0;
        for entry in std::fs::read_dir(base_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("pack-")
                && name.ends_with(".bin")
                && let Ok(id) = name[5..name.len() - 4].parse::<u32>()
            {
                latest_id = latest_id.max(id);
            }
        }

        let pack_path = base_dir.join(format!("pack-{:06}.bin", latest_id));
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

        tracing::info!(
            latest_pack_id = latest_id,
            current_pack_bytes = current_size,
            "Opened L2 Multi-threaded Atomic PackStore."
        );

        Ok(Self {
            base_dir: base_dir.to_path_buf(),
            writer: Mutex::new(writer),
            readers: RwLock::new(HashMap::new()),
        })
    }

    fn pack_path(&self, pack_id: u32) -> PathBuf {
        self.base_dir.join(format!("pack-{:06}.bin", pack_id))
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

    /// Thread-safe atomic append to active pack file with strict offset guarantee.
    pub fn append(&self, compressed_bytes: &[u8]) -> Result<ThumbnailLocation, ThumbnailError> {
        let t0 = Instant::now();
        let len = compressed_bytes.len() as u32;

        let mut writer = self.writer.lock();

        if writer.current_size + (len as u64) > PACK_MAX_SIZE {
            writer.pack_id += 1;
            let new_path = self.pack_path(writer.pack_id);
            writer.file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&new_path)?;
            writer.current_size = 0;
            tracing::info!(
                new_pack_id = writer.pack_id,
                "Rotated to new Pack container."
            );
        }

        let offset = writer.current_size;
        writer.file.write_all(compressed_bytes)?;
        writer.file.flush()?;
        writer.current_size += len as u64;

        let location = ThumbnailLocation {
            pack_id: writer.pack_id,
            offset,
            length: len,
        };

        tracing::trace!(
            pack_id = location.pack_id,
            offset = location.offset,
            length = location.length,
            append_time_us = t0.elapsed().as_micros(),
            "[PackStore] Appended bytes atomically to container"
        );

        Ok(location)
    }

    /// Reads compressed thumbnail slice via high-speed atomic pread syscalls.
    pub fn read(&self, location: ThumbnailLocation) -> Result<Vec<u8>, ThumbnailError> {
        let t0 = Instant::now();
        let reader = self.get_or_open_reader(location.pack_id)?;

        let file_len = reader.metadata()?.len();
        let required_end = location.offset + (location.length as u64);

        if required_end > file_len {
            return Err(ThumbnailError::PackOutOfBounds {
                pack_id: location.pack_id,
                offset: location.offset,
                length: location.length,
            });
        }

        let mut buffer = vec![0u8; location.length as usize];

        read_exact_at(&reader, &mut buffer, location.offset)?;

        // Validate JPEG magic bytes (0xFF, 0xD8) to catch corruptions early
        if buffer.len() < 2 || buffer[0] != 0xFF || buffer[1] != 0xD8 {
            return Err(ThumbnailError::Ffmpeg(format!(
                "Corrupted JPEG header in pack {} at offset {}: start bytes {:02X}{:02X}",
                location.pack_id, location.offset, buffer[0], buffer[1]
            )));
        }

        tracing::trace!(
            pack_id = location.pack_id,
            bytes = buffer.len(),
            read_time_us = t0.elapsed().as_micros(),
            "[PackStore] Atomic read_exact_at slice completed"
        );

        Ok(buffer)
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
