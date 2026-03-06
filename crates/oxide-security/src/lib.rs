//! # Oxide Security
//!
//! Encryption, attestation, and security primitives for the Oxide edge AI runtime.
//! Supports AES-256-GCM encryption for model files at rest and in transit.

#![deny(unsafe_code)]

pub mod encryption;
pub mod integrity;

pub use encryption::{decrypt_model, encrypt_model, EncryptionKey};
pub use integrity::verify_sha256;
