use super::error::ThumbnailError;
use super::types::ThumbnailLocation;
use memmap2::Mmap;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const PACK_MAX_SIZE: u64 = 64 * 1024 * 1024; // 64 MiB per binary pack file

pub struct PackStore {
    base_dir: PathBuf,
    active_pack_id: RwLock<u32>,
    active_pack_file: RwLock<File>,
    mmaps: RwLock<HashMap<u32, Mmap>>,
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
        let active_file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&pack_path)?;

        let store = Self {
            base_dir: base_dir.to_path_buf(),
            active_pack_id: RwLock::new(latest_id),
            active_pack_file: RwLock::new(active_file),
            mmaps: RwLock::new(HashMap::new()),
        };

        store.load_mmap(latest_id)?;
        Ok(store)
    }

    fn pack_path(&self, pack_id: u32) -> PathBuf {
        self.base_dir.join(format!("pack-{:06}.bin", pack_id))
    }

    fn load_mmap(&self, pack_id: u32) -> Result<(), ThumbnailError> {
        let path = self.pack_path(pack_id);
        if path.exists() {
            let file = File::open(&path)?;
            if file.metadata()?.len() > 0 {
                let mmap = unsafe { Mmap::map(&file)? };
                self.mmaps.write().insert(pack_id, mmap);
            }
        }
        Ok(())
    }

    /// Sequentially appends compressed thumbnail bytes to the current pack.
    pub fn append(&self, compressed_bytes: &[u8]) -> Result<ThumbnailLocation, ThumbnailError> {
        let len = compressed_bytes.len() as u32;
        let mut active_id = self.active_pack_id.write();
        let mut file = self.active_pack_file.write();

        let current_size = file.metadata()?.len();

        if current_size + (len as u64) > PACK_MAX_SIZE {
            *active_id += 1;
            let new_path = self.pack_path(*active_id);
            *file = OpenOptions::new()
                .create(true)
                .read(true)
                .append(true)
                .open(&new_path)?;
        }

        let offset = file.seek(SeekFrom::End(0))?;
        file.write_all(compressed_bytes)?;
        file.flush()?;

        let location = ThumbnailLocation {
            pack_id: *active_id,
            offset,
            length: len,
        };

        self.load_mmap(*active_id)?;

        Ok(location)
    }

    /// Reads compressed thumbnail slice via fast memory-mapped file access.
    pub fn read(&self, location: ThumbnailLocation) -> Result<Vec<u8>, ThumbnailError> {
        let mmaps = self.mmaps.read();
        if let Some(mmap) = mmaps.get(&location.pack_id) {
            let start = location.offset as usize;
            let end = start + location.length as usize;

            if end <= mmap.len() {
                return Ok(mmap[start..end].to_vec());
            }
        }
        drop(mmaps);

        self.load_mmap(location.pack_id)?;
        let mmaps = self.mmaps.read();
        if let Some(mmap) = mmaps.get(&location.pack_id) {
            let start = location.offset as usize;
            let end = start + location.length as usize;
            if end <= mmap.len() {
                return Ok(mmap[start..end].to_vec());
            }
        }

        Err(ThumbnailError::PackOutOfBounds {
            pack_id: location.pack_id,
            offset: location.offset,
            length: location.length,
        })
    }
}
