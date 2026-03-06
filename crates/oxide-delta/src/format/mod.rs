//! Model format parsers (ONNX, SafeTensors).
//!
//! Extracts tensor names and raw bytes so the delta engine can compare
//! and diff at tensor granularity.

pub(crate) mod onnx;
mod safetensors;

use crate::manifest::TensorManifest;
use crate::patch::{DeltaPatch, PatchStrategy};
use crate::{DeltaError, Result};

/// A recognized model format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    Onnx,
    SafeTensors,
    Unknown,
}

/// Metadata about a single tensor within a model file.
#[derive(Debug, Clone)]
pub struct TensorInfo {
    pub name: String,
    pub data: Vec<u8>,
}

/// Detect format from file contents.
pub fn detect_format(data: &[u8]) -> ModelFormat {
    if onnx::is_onnx(data) {
        ModelFormat::Onnx
    } else if safetensors::is_safetensors(data) {
        ModelFormat::SafeTensors
    } else {
        ModelFormat::Unknown
    }
}

/// Extract tensors from a model file (if format is recognized).
pub fn extract_tensors(data: &[u8]) -> Result<Vec<TensorInfo>> {
    match detect_format(data) {
        ModelFormat::Onnx => onnx::extract_tensors(data),
        ModelFormat::SafeTensors => safetensors::extract_tensors(data),
        ModelFormat::Unknown => Err(DeltaError::UnsupportedFormat),
    }
}

/// Build a tensor manifest from raw model bytes.
pub fn build_manifest(data: &[u8]) -> Result<TensorManifest> {
    let tensors = extract_tensors(data)?;
    Ok(TensorManifest::from_tensors(&tensors))
}

/// Try to compute a tensor-level delta between two model files.
///
/// Returns `None` if either file is not a recognized format, or if
/// they don't have compatible tensor layouts.
pub fn try_tensor_delta(base: &[u8], target: &[u8]) -> Result<Option<DeltaPatch>> {
    let base_format = detect_format(base);
    let target_format = detect_format(target);

    // Both must be the same recognized format for tensor-level delta
    if base_format == ModelFormat::Unknown
        || target_format == ModelFormat::Unknown
        || base_format != target_format
    {
        return Ok(None);
    }

    let base_tensors = extract_tensors(base)?;
    let target_tensors = extract_tensors(target)?;

    let base_manifest = TensorManifest::from_tensors(&base_tensors);
    let target_manifest = TensorManifest::from_tensors(&target_tensors);

    // Index base tensors and their hashes by name
    let base_by_name: std::collections::HashMap<&str, (&TensorInfo, &[u8; 32])> = base_tensors
        .iter()
        .zip(base_manifest.entries.iter())
        .map(|(t, m)| (t.name.as_str(), (t, &m.sha256)))
        .collect();

    let base_sha = sha256(base);
    let target_sha = sha256(target);

    let mut chunks = Vec::new();

    for (i, target_tensor) in target_tensors.iter().enumerate() {
        let target_hash = &target_manifest.entries[i].sha256;

        if let Some(&(base_tensor, base_hash)) = base_by_name.get(target_tensor.name.as_str()) {
            if base_hash == target_hash {
                // Tensor unchanged — COPY
                chunks.push(crate::patch::PatchChunk {
                    name: target_tensor.name.clone(),
                    op: crate::patch::ChunkOp::Copy,
                    data: Vec::new(),
                    uncompressed_len: target_tensor.data.len(),
                });
            } else {
                // Tensor changed — XOR delta
                let xor_data = xor_bytes(&base_tensor.data, &target_tensor.data);
                let compressed =
                    zstd::bulk::compress(&xor_data, 3).map_err(DeltaError::Io)?;
                chunks.push(crate::patch::PatchChunk {
                    name: target_tensor.name.clone(),
                    op: crate::patch::ChunkOp::Xor,
                    data: compressed,
                    uncompressed_len: target_tensor.data.len(),
                });
            }
        } else {
            // New tensor — REPLACE
            let compressed = zstd::bulk::compress(&target_tensor.data, 3)
                .map_err(DeltaError::Io)?;
            chunks.push(crate::patch::PatchChunk {
                name: target_tensor.name.clone(),
                op: crate::patch::ChunkOp::Replace,
                data: compressed,
                uncompressed_len: target_tensor.data.len(),
            });
        }
    }

    // Also need to handle non-tensor data (graph structure, metadata).
    // Serialize the target with zeroed tensor data and store as a REPLACE
    // chunk so we can reconstruct the full file.
    let skeleton = match target_format {
        ModelFormat::Onnx => onnx::serialize_skeleton(target)?,
        ModelFormat::SafeTensors => safetensors::serialize_skeleton(target)?,
        ModelFormat::Unknown => unreachable!(),
    };

    chunks.push(crate::patch::PatchChunk {
        name: "__skeleton__".to_string(),
        op: crate::patch::ChunkOp::Replace,
        data: zstd::bulk::compress(&skeleton, 3).map_err(DeltaError::Io)?,
        uncompressed_len: skeleton.len(),
    });

    Ok(Some(DeltaPatch {
        strategy: PatchStrategy::Tensor,
        base_sha256: base_sha,
        target_sha256: target_sha,
        target_size: target.len(),
        format: target_format,
        chunks,
    }))
}

/// XOR two byte slices. If lengths differ, the shorter is zero-extended.
pub(crate) fn xor_bytes(a: &[u8], b: &[u8]) -> Vec<u8> {
    let len = a.len().max(b.len());
    let mut result = vec![0u8; len];
    for i in 0..a.len().min(b.len()) {
        result[i] = a[i] ^ b[i];
    }
    // If b is longer, remaining bytes are just b[i] ^ 0 = b[i]
    if b.len() > a.len() {
        result[a.len()..].copy_from_slice(&b[a.len()..]);
    } else if a.len() > b.len() {
        result[b.len()..].copy_from_slice(&a[b.len()..]);
    }
    result
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}
