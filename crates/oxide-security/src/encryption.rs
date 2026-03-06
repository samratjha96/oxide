//! AES-256-GCM encryption for model files.
//!
//! Models are encrypted at rest with AES-256-GCM, which provides both
//! confidentiality and authenticity. Each encryption uses a random nonce
//! which is prepended to the ciphertext.

use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use oxide_core::error::{OxideError, Result};
use rand::RngCore;
use std::path::Path;
use tracing::info;

/// Nonce size for AES-256-GCM (96 bits = 12 bytes).
const NONCE_SIZE: usize = 12;

/// An AES-256-GCM encryption key.
#[derive(Clone)]
pub struct EncryptionKey {
    key: Key<Aes256Gcm>,
}

impl EncryptionKey {
    /// Generate a new random encryption key.
    pub fn generate() -> Self {
        let key = Aes256Gcm::generate_key(OsRng);
        EncryptionKey { key }
    }

    /// Create a key from raw bytes (must be exactly 32 bytes).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(OxideError::Encryption(format!(
                "Key must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let key = Key::<Aes256Gcm>::from_slice(bytes).clone();
        Ok(EncryptionKey { key })
    }

    /// Export the key as raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.key.as_slice()
    }

    /// Save the key to a file (hex-encoded).
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let hex = hex_encode(self.key.as_slice());
        std::fs::write(path, hex)?;
        Ok(())
    }

    /// Load a key from a file (hex-encoded).
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let hex = std::fs::read_to_string(path)?;
        let bytes = hex_decode(hex.trim())?;
        Self::from_bytes(&bytes)
    }
}

impl std::fmt::Debug for EncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EncryptionKey")
            .field("key", &"[REDACTED]")
            .finish()
    }
}

/// Encrypt raw data using AES-256-GCM.
///
/// Returns `nonce || ciphertext || tag` as a single byte vector.
pub fn encrypt_data(key: &EncryptionKey, plaintext: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(&key.key);

    // Generate a random nonce
    let mut nonce_bytes = [0u8; NONCE_SIZE];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| OxideError::Encryption(format!("Encryption failed: {}", e)))?;

    // Prepend nonce to ciphertext
    let mut result = Vec::with_capacity(NONCE_SIZE + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(result)
}

/// Decrypt data that was encrypted with `encrypt_data`.
///
/// Expects `nonce || ciphertext || tag` format.
pub fn decrypt_data(key: &EncryptionKey, data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < NONCE_SIZE {
        return Err(OxideError::Decryption(
            "Data too short to contain nonce".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new(&key.key);
    let nonce = Nonce::from_slice(&data[..NONCE_SIZE]);
    let ciphertext = &data[NONCE_SIZE..];

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| OxideError::Decryption(format!("Decryption failed: {}", e)))?;

    Ok(plaintext)
}

/// Encrypt a model file to a destination path.
pub fn encrypt_model(key: &EncryptionKey, source: &Path, dest: &Path) -> Result<u64> {
    let plaintext = std::fs::read(source)?;
    info!(
        "Encrypting model: {} -> {} ({} bytes)",
        source.display(),
        dest.display(),
        plaintext.len()
    );

    let encrypted = encrypt_data(key, &plaintext)?;
    let size = encrypted.len() as u64;
    std::fs::write(dest, &encrypted)?;

    info!("Model encrypted: {} bytes", size);
    Ok(size)
}

/// Decrypt a model file to a destination path.
pub fn decrypt_model(key: &EncryptionKey, source: &Path, dest: &Path) -> Result<u64> {
    let encrypted = std::fs::read(source)?;
    info!(
        "Decrypting model: {} -> {} ({} bytes)",
        source.display(),
        dest.display(),
        encrypted.len()
    );

    let plaintext = decrypt_data(key, &encrypted)?;
    let size = plaintext.len() as u64;
    std::fs::write(dest, &plaintext)?;

    info!("Model decrypted: {} bytes", size);
    Ok(size)
}

/// Simple hex encoding (no external dependency).
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Simple hex decoding.
fn hex_decode(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return Err(OxideError::Encryption("Invalid hex string length".to_string()));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| OxideError::Encryption(format!("Invalid hex: {}", e)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_generation() {
        let key = EncryptionKey::generate();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_key_from_bytes() {
        let bytes = [42u8; 32];
        let key = EncryptionKey::from_bytes(&bytes).unwrap();
        assert_eq!(key.as_bytes(), &bytes);
    }

    #[test]
    fn test_key_from_bytes_wrong_size() {
        let bytes = [0u8; 16];
        assert!(EncryptionKey::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = EncryptionKey::generate();
        let plaintext = b"Hello, Oxide edge AI runtime!";

        let encrypted = encrypt_data(&key, plaintext).unwrap();
        assert_ne!(&encrypted, plaintext.as_slice());
        assert!(encrypted.len() > plaintext.len()); // nonce + tag overhead

        let decrypted = decrypt_data(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_encrypt_decrypt_large() {
        let key = EncryptionKey::generate();
        // Simulate a model file (1MB)
        let plaintext: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

        let encrypted = encrypt_data(&key, &plaintext).unwrap();
        let decrypted = decrypt_data(&key, &encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_wrong_key_fails() {
        let key1 = EncryptionKey::generate();
        let key2 = EncryptionKey::generate();
        let plaintext = b"Secret model data";

        let encrypted = encrypt_data(&key1, plaintext).unwrap();
        let result = decrypt_data(&key2, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_tampered_data_fails() {
        let key = EncryptionKey::generate();
        let plaintext = b"Secret model data";

        let mut encrypted = encrypt_data(&key, plaintext).unwrap();
        // Tamper with the ciphertext
        if let Some(byte) = encrypted.last_mut() {
            *byte ^= 0xFF;
        }
        let result = decrypt_data(&key, &encrypted);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_decrypt_model_file() {
        let dir = TempDir::new().unwrap();
        let key = EncryptionKey::generate();

        let source = dir.path().join("model.onnx");
        let encrypted_path = dir.path().join("model.onnx.enc");
        let decrypted_path = dir.path().join("model_decrypted.onnx");

        let original_data = b"fake model data for testing";
        std::fs::write(&source, original_data).unwrap();

        // Encrypt
        let enc_size = encrypt_model(&key, &source, &encrypted_path).unwrap();
        assert!(enc_size > original_data.len() as u64);

        // Decrypt
        let dec_size = decrypt_model(&key, &encrypted_path, &decrypted_path).unwrap();
        assert_eq!(dec_size, original_data.len() as u64);

        // Verify
        let decrypted = std::fs::read(&decrypted_path).unwrap();
        assert_eq!(decrypted, original_data);
    }

    #[test]
    fn test_key_file_roundtrip() {
        let dir = TempDir::new().unwrap();
        let key_path = dir.path().join("key.hex");

        let key = EncryptionKey::generate();
        key.save_to_file(&key_path).unwrap();

        let loaded = EncryptionKey::load_from_file(&key_path).unwrap();
        assert_eq!(key.as_bytes(), loaded.as_bytes());
    }

    #[test]
    fn test_hex_roundtrip() {
        let data = vec![0, 1, 127, 255, 42];
        let encoded = hex_encode(&data);
        let decoded = hex_decode(&encoded).unwrap();
        assert_eq!(data, decoded);
    }
}
