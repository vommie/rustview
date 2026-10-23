use crate::settings::LogObscurer;
use md5::{Digest, Md5};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HashError {
    #[error("Failed to read file for hashing: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse hexadecimal hash string: {0}")]
    InvalidHex(#[from] hex::FromHexError),

    #[error("Invalid MD5 hash length: expected {expected} bytes, found {found} bytes")]
    InvalidLength { expected: usize, found: usize },
}

/// 128-bit MD5 binary digest (16 bytes). Compatible with standard RFC 1321 & Python `hashlib.md5()`.
pub type FileHash = [u8; 16];

pub struct MediaHasher;

impl MediaHasher {
    /// Hashes an entire file using RFC 1321 MD5 with a 64 KiB buffer.
    /// Produces the exact binary equivalent of Python's `hashlib.md5().digest()`.
    pub fn hash_file(path: &Path) -> Result<FileHash, HashError> {
        let file = File::open(path)?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut hasher = Md5::new();
        let mut buffer = vec![0u8; 64 * 1024];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            hasher.update(&buffer[..bytes_read]);
        }

        let digest = hasher.finalize();
        let mut result = [0u8; 16];
        result.copy_from_slice(&digest);

        tracing::trace!(
            file = %LogObscurer::path(path),
            md5_hex = %Self::to_hex(&result),
            "[MediaHasher: MD5] Successfully computed file checksum"
        );

        Ok(result)
    }

    /// Hashes a memory slice directly into a 16-byte MD5 digest.
    pub fn hash_bytes(data: &[u8]) -> FileHash {
        let mut hasher = Md5::new();
        hasher.update(data);
        let digest = hasher.finalize();
        let mut result = [0u8; 16];
        result.copy_from_slice(&digest);
        result
    }

    /// Converts a 16-byte MD5 digest into a 32-character lowercase hexadecimal string
    /// matching Python's `hashlib.md5().hexdigest()`.
    pub fn to_hex(hash: &FileHash) -> String {
        hex::encode(hash)
    }

    /// Parses a 32-character hexadecimal string into a 16-byte `FileHash`.
    pub fn from_hex(hex_str: &str) -> Result<FileHash, HashError> {
        let bytes = hex::decode(hex_str.trim())?;
        if bytes.len() != 16 {
            return Err(HashError::InvalidLength {
                expected: 16,
                found: bytes.len(),
            });
        }
        let mut hash = [0u8; 16];
        hash.copy_from_slice(&bytes);
        Ok(hash)
    }
}
