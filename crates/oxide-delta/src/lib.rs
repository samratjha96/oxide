#![deny(unsafe_code)]

//! ML-aware delta compression for model files.
//!
//! Understands ONNX and SafeTensors structure to produce minimal
//! patches when model weights change. Falls back to binary delta
//! for unknown formats.

mod format;
mod manifest;
/// Patch types and delta strategies.
pub mod patch;

pub use format::{build_manifest, ModelFormat, TensorInfo};
pub use manifest::TensorManifest;
pub use patch::{ChunkOp, DeltaPatch, PatchChunk, PatchStrategy};

/// Test utilities (public so integration tests can use them).
#[doc(hidden)]
pub mod test_util;


/// Errors produced by the delta engine.
#[derive(Debug, thiserror::Error)]
pub enum DeltaError {
    #[error("unsupported model format")]
    UnsupportedFormat,

    #[error("ONNX decode error: {0}")]
    OnnxDecode(#[from] prost::DecodeError),

    #[error("ONNX encode error: {0}")]
    OnnxEncode(#[from] prost::EncodeError),

    #[error("SafeTensors parse error: {0}")]
    SafeTensors(String),

    #[error("patch verification failed: expected SHA-256 {expected}, got {actual}")]
    VerifyFailed { expected: String, actual: String },

    #[error("patch base mismatch: expected {expected}, got {actual}")]
    BaseMismatch { expected: String, actual: String },

    #[error("invalid patch: {0}")]
    InvalidPatch(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, DeltaError>;

/// Compute the best delta between two model files.
///
/// Tries tensor-level delta (if format is recognized) and binary delta.
/// Returns whichever is smaller, or `None` if the full file is smaller
/// than any delta.
pub fn compute_delta(base: &[u8], target: &[u8]) -> Result<Option<DeltaPatch>> {
    let tensor_delta = format::try_tensor_delta(base, target)?;
    let binary_delta = patch::binary_delta(base, target)?;

    // Pick the smallest option
    let best = match tensor_delta {
        Some(td) if td.encoded_size() < binary_delta.encoded_size() => td,
        _ => binary_delta,
    };

    // If the delta is larger than the target, don't bother
    if best.encoded_size() >= target.len() {
        return Ok(None);
    }

    Ok(Some(best))
}

/// Apply a delta patch to a base file, producing the target.
///
/// Verifies SHA-256 of the result. Returns an error if verification fails.
pub fn apply_delta(base: &[u8], patch: &DeltaPatch) -> Result<Vec<u8>> {
    patch::apply(base, patch)
}
