//! SHA-256 integrity verification.

use oxide_core::error::Result;
use sha2::{Digest, Sha256};
use std::path::Path;

/// Compute SHA-256 hash of a file and return hex string.
pub fn sha256_file(path: &Path) -> Result<String> {
    let data = std::fs::read(path)?;
    Ok(sha256_bytes(&data))
}

/// Compute SHA-256 hash of bytes and return hex string.
pub fn sha256_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Verify that a file matches an expected SHA-256 hash.
pub fn verify_sha256(path: &Path, expected_hash: &str) -> Result<bool> {
    let actual = sha256_file(path)?;
    Ok(actual == expected_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_sha256_known_value() {
        // SHA-256 of empty string
        let hash = sha256_bytes(b"");
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"hello world").unwrap();
        let hash = sha256_file(&path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_verify_sha256() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"hello world").unwrap();

        assert!(verify_sha256(
            &path,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        )
        .unwrap());

        assert!(!verify_sha256(&path, "wrong_hash").unwrap());
    }
}
