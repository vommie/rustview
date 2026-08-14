use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HashError {
    #[error("Failed to read file for hashing: {0}")]
    Io(#[from] std::io::Error),
}

pub type FileHash = [u8; 32];

pub struct MediaHasher;

impl MediaHasher {
    /// Hashes an entire file using BLAKE3 with a 64 KiB buffer.
    pub fn hash_file(path: &Path) -> Result<FileHash, HashError> {
        let file = File::open(path)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0u8; 64 * 1024];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        Ok(*hasher.finalize().as_bytes())
    }

    /// Hashes memory slice directly.
    pub fn hash_bytes(data: &[u8]) -> FileHash {
        *blake3::hash(data).as_bytes()
    }
}
