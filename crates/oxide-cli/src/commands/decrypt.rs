//! `oxide decrypt` — Decrypt a model file.

use oxide_security::{decrypt_model, EncryptionKey};
use std::path::Path;

pub fn execute(model_path: &str, output: Option<&str>, key_path: &str) -> anyhow::Result<()> {
    let source = Path::new(model_path);
    if !source.exists() {
        anyhow::bail!("Encrypted model file not found: {}", model_path);
    }

    let key_file = Path::new(key_path);
    if !key_file.exists() {
        anyhow::bail!("Encryption key file not found: {}", key_path);
    }

    println!("🔑 Loading encryption key from: {}", key_path);
    let key = EncryptionKey::load_from_file(key_file)?;

    let dest_path = output
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            model_path
                .strip_suffix(".enc")
                .unwrap_or(model_path)
                .to_string()
                + ".dec"
        });
    let dest = Path::new(&dest_path);

    println!("🔓 Decrypting: {} → {}", model_path, dest_path);
    let size = decrypt_model(&key, source, dest)?;
    println!("✅ Decrypted model: {:.2} KB", size as f64 / 1024.0);

    Ok(())
}
