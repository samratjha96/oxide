//! `oxide encrypt` — Encrypt a model file.

use oxide_security::{encrypt_model, EncryptionKey};
use std::path::Path;

pub fn execute(model_path: &str, output: Option<&str>, key_path: &str) -> anyhow::Result<()> {
    let source = Path::new(model_path);
    if !source.exists() {
        anyhow::bail!("Model file not found: {}", model_path);
    }

    let key_file = Path::new(key_path);

    // Load or generate key
    let key = if key_file.exists() {
        println!("  loading key from {}", key_path);
        EncryptionKey::load_from_file(key_file)?
    } else {
        println!("  generating new key: {}", key_path);
        let key = EncryptionKey::generate();
        key.save_to_file(key_file)?;
        key
    };

    let dest_path = output
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}.enc", model_path));
    let dest = Path::new(&dest_path);

    println!("oxide encrypt {} -> {}", model_path, dest_path);
    let size = encrypt_model(&key, source, dest)?;
    println!("  done ({:.2} KB)", size as f64 / 1024.0);

    Ok(())
}
