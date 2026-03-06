//! Delta patch types and binary delta strategy.

use crate::format::ModelFormat;
use crate::{DeltaError, Result};
use sha2::{Digest, Sha256};

/// Which delta strategy produced this patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchStrategy {
    /// Tensor-level: COPY unchanged tensors, XOR changed ones.
    Tensor,
    /// Binary: zstd dictionary compression (base as dictionary).
    Binary,
}

/// Operation for a single chunk in the patch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChunkOp {
    /// Region identical in base and target. Zero data bytes.
    Copy,
    /// Region is entirely new or replaced. Data = compressed target bytes.
    Replace,
    /// Region differs. Data = compressed (base XOR target).
    Xor,
}

/// A single chunk in the delta patch.
#[derive(Debug, Clone)]
pub struct PatchChunk {
    /// Tensor name (or "" for binary chunks, "__skeleton__" for graph structure).
    pub name: String,
    /// What operation to apply.
    pub op: ChunkOp,
    /// Compressed data (empty for COPY).
    pub data: Vec<u8>,
    /// Uncompressed size of this chunk's contribution to the target.
    pub uncompressed_len: usize,
}

/// A complete delta patch.
#[derive(Debug, Clone)]
pub struct DeltaPatch {
    pub strategy: PatchStrategy,
    pub base_sha256: [u8; 32],
    pub target_sha256: [u8; 32],
    pub target_size: usize,
    pub format: ModelFormat,
    pub chunks: Vec<PatchChunk>,
}

impl DeltaPatch {
    /// Total encoded size of this patch (approximate wire size).
    pub fn encoded_size(&self) -> usize {
        // Header overhead + sum of chunk data
        80 + self
            .chunks
            .iter()
            .map(|c| 16 + c.name.len() + c.data.len())
            .sum::<usize>()
    }
}

// --- Serialization (OXDL wire format) ---

const MAGIC: &[u8; 4] = b"OXDL";
const VERSION: u8 = 1;

impl DeltaPatch {
    /// Serialize to OXDL wire format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.encoded_size());

        // Header
        buf.extend_from_slice(MAGIC);
        buf.push(VERSION);
        buf.push(match self.strategy {
            PatchStrategy::Binary => 0,
            PatchStrategy::Tensor => 1,
        });
        buf.extend_from_slice(&self.base_sha256);
        buf.extend_from_slice(&self.target_sha256);
        buf.extend_from_slice(&(self.target_size as u64).to_le_bytes());
        buf.extend_from_slice(&(self.chunks.len() as u32).to_le_bytes());
        buf.push(match self.format {
            ModelFormat::Onnx => 1,
            ModelFormat::SafeTensors => 2,
            ModelFormat::Unknown => 0,
        });
        // Pad to consistent header size (80 bytes total)
        // 4 + 1 + 1 + 32 + 32 + 8 + 4 + 1 = 83... let me recalculate
        // We have 83 bytes so far. Trim padding or adjust.
        // Actually let's use a simpler variable-length header.

        // Chunks
        for chunk in &self.chunks {
            let name_bytes = chunk.name.as_bytes();
            buf.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            buf.extend_from_slice(name_bytes);
            buf.extend_from_slice(&(chunk.uncompressed_len as u32).to_le_bytes());
            buf.push(match chunk.op {
                ChunkOp::Copy => 0,
                ChunkOp::Replace => 1,
                ChunkOp::Xor => 2,
            });
            buf.extend_from_slice(&(chunk.data.len() as u32).to_le_bytes());
            buf.extend_from_slice(&chunk.data);
        }

        buf
    }

    /// Deserialize from OXDL wire format.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let mut pos = 0;

        let read = |pos: &mut usize, n: usize| -> Result<&[u8]> {
            if *pos + n > data.len() {
                return Err(DeltaError::InvalidPatch("unexpected end of patch".into()));
            }
            let slice = &data[*pos..*pos + n];
            *pos += n;
            Ok(slice)
        };

        // Header
        let magic = read(&mut pos, 4)?;
        if magic != MAGIC {
            return Err(DeltaError::InvalidPatch("bad magic".into()));
        }
        let version = read(&mut pos, 1)?[0];
        if version != VERSION {
            return Err(DeltaError::InvalidPatch(format!(
                "unsupported version {version}"
            )));
        }
        let strategy_byte = read(&mut pos, 1)?[0];
        let strategy = match strategy_byte {
            0 => PatchStrategy::Binary,
            1 => PatchStrategy::Tensor,
            _ => {
                return Err(DeltaError::InvalidPatch(format!(
                    "unknown strategy {strategy_byte}"
                )))
            }
        };

        let base_sha256: [u8; 32] = read(&mut pos, 32)?
            .try_into()
            .map_err(|_| DeltaError::InvalidPatch("bad base sha256".into()))?;
        let target_sha256: [u8; 32] = read(&mut pos, 32)?
            .try_into()
            .map_err(|_| DeltaError::InvalidPatch("bad target sha256".into()))?;
        let target_size =
            u64::from_le_bytes(read(&mut pos, 8)?.try_into().unwrap()) as usize;
        let num_chunks =
            u32::from_le_bytes(read(&mut pos, 4)?.try_into().unwrap()) as usize;
        let format_byte = read(&mut pos, 1)?[0];
        let format = match format_byte {
            1 => ModelFormat::Onnx,
            2 => ModelFormat::SafeTensors,
            _ => ModelFormat::Unknown,
        };

        // Chunks
        let mut chunks = Vec::with_capacity(num_chunks);
        for _ in 0..num_chunks {
            let name_len =
                u16::from_le_bytes(read(&mut pos, 2)?.try_into().unwrap()) as usize;
            let name = String::from_utf8(read(&mut pos, name_len)?.to_vec())
                .map_err(|e| DeltaError::InvalidPatch(format!("invalid chunk name: {e}")))?;
            let uncompressed_len =
                u32::from_le_bytes(read(&mut pos, 4)?.try_into().unwrap()) as usize;
            let op_byte = read(&mut pos, 1)?[0];
            let op = match op_byte {
                0 => ChunkOp::Copy,
                1 => ChunkOp::Replace,
                2 => ChunkOp::Xor,
                _ => return Err(DeltaError::InvalidPatch(format!("unknown op {op_byte}"))),
            };
            let data_len =
                u32::from_le_bytes(read(&mut pos, 4)?.try_into().unwrap()) as usize;
            let chunk_data = read(&mut pos, data_len)?.to_vec();
            chunks.push(PatchChunk {
                name,
                op,
                data: chunk_data,
                uncompressed_len,
            });
        }

        Ok(Self {
            strategy,
            base_sha256,
            target_sha256,
            target_size,
            format,
            chunks,
        })
    }
}

// --- Binary delta (Strategy B) ---

/// Compute a binary delta: zstd-compress target using base as a dictionary.
pub fn binary_delta(base: &[u8], target: &[u8]) -> Result<DeltaPatch> {
    let base_sha = sha256(base);
    let target_sha = sha256(target);

    // Use base as a zstd dictionary to compress target
    let mut compressor = zstd::bulk::Compressor::with_dictionary(3, base)
        .map_err(|e| DeltaError::Io(std::io::Error::other(e)))?;
    let compressed = compressor
        .compress(target)
        .map_err(|e| DeltaError::Io(std::io::Error::other(e)))?;

    let chunks = vec![PatchChunk {
        name: String::new(),
        op: ChunkOp::Replace,
        data: compressed,
        uncompressed_len: target.len(),
    }];

    Ok(DeltaPatch {
        strategy: PatchStrategy::Binary,
        base_sha256: base_sha,
        target_sha256: target_sha,
        target_size: target.len(),
        format: ModelFormat::Unknown,
        chunks,
    })
}

// --- Patch application ---

/// Apply a delta patch to a base file, producing the target.
pub fn apply(base: &[u8], patch: &DeltaPatch) -> Result<Vec<u8>> {
    // Verify base
    let base_sha = sha256(base);
    if base_sha != patch.base_sha256 {
        return Err(DeltaError::BaseMismatch {
            expected: hex_encode(&patch.base_sha256),
            actual: hex_encode(&base_sha),
        });
    }

    let result = match patch.strategy {
        PatchStrategy::Binary => apply_binary(base, patch)?,
        PatchStrategy::Tensor => apply_tensor(base, patch)?,
    };

    // Verify target
    let result_sha = sha256(&result);
    if result_sha != patch.target_sha256 {
        return Err(DeltaError::VerifyFailed {
            expected: hex_encode(&patch.target_sha256),
            actual: hex_encode(&result_sha),
        });
    }

    Ok(result)
}

fn apply_binary(base: &[u8], patch: &DeltaPatch) -> Result<Vec<u8>> {
    // Binary delta: single REPLACE chunk = zstd-compressed with base as dict
    if patch.chunks.len() != 1 {
        return Err(DeltaError::InvalidPatch(
            "binary patch must have exactly 1 chunk".into(),
        ));
    }

    let chunk = &patch.chunks[0];
    let mut decompressor = zstd::bulk::Decompressor::with_dictionary(base)
        .map_err(|e| DeltaError::Io(std::io::Error::other(e)))?;
    let decompressed = decompressor
        .decompress(&chunk.data, chunk.uncompressed_len)
        .map_err(|e| DeltaError::Io(std::io::Error::other(e)))?;

    Ok(decompressed)
}

fn apply_tensor(base: &[u8], patch: &DeltaPatch) -> Result<Vec<u8>> {
    // Extract base tensors
    let base_tensors = crate::format::extract_tensors(base)?;
    let base_by_name: std::collections::HashMap<&str, &crate::format::TensorInfo> =
        base_tensors.iter().map(|t| (t.name.as_str(), t)).collect();

    // Reconstruct target tensors
    let mut target_tensors = Vec::new();
    let mut skeleton_data = None;

    for chunk in &patch.chunks {
        if chunk.name == "__skeleton__" {
            let decompressed = zstd::bulk::decompress(&chunk.data, chunk.uncompressed_len)
                .map_err(DeltaError::Io)?;
            skeleton_data = Some(decompressed);
            continue;
        }

        match chunk.op {
            ChunkOp::Copy => {
                let base_tensor = base_by_name.get(chunk.name.as_str()).ok_or_else(|| {
                    DeltaError::InvalidPatch(format!(
                        "COPY chunk references unknown tensor '{}'",
                        chunk.name
                    ))
                })?;
                target_tensors.push(crate::format::TensorInfo {
                    name: chunk.name.clone(),
                    data: base_tensor.data.clone(),
                });
            }
            ChunkOp::Replace => {
                let decompressed =
                    zstd::bulk::decompress(&chunk.data, chunk.uncompressed_len)
                        .map_err(DeltaError::Io)?;
                target_tensors.push(crate::format::TensorInfo {
                    name: chunk.name.clone(),
                    data: decompressed,
                });
            }
            ChunkOp::Xor => {
                let base_tensor = base_by_name.get(chunk.name.as_str()).ok_or_else(|| {
                    DeltaError::InvalidPatch(format!(
                        "XOR chunk references unknown tensor '{}'",
                        chunk.name
                    ))
                })?;
                let xor_data =
                    zstd::bulk::decompress(&chunk.data, chunk.uncompressed_len)
                        .map_err(DeltaError::Io)?;
                let target_data = crate::format::xor_bytes(&base_tensor.data, &xor_data);
                target_tensors.push(crate::format::TensorInfo {
                    name: chunk.name.clone(),
                    data: target_data,
                });
            }
        }
    }

    // Reconstruct full file from skeleton + tensors
    let skeleton = skeleton_data
        .ok_or_else(|| DeltaError::InvalidPatch("tensor patch missing __skeleton__".into()))?;

    match patch.format {
        ModelFormat::Onnx => {
            crate::format::onnx::reconstruct(&skeleton, &target_tensors)
        }
        _ => Err(DeltaError::InvalidPatch(
            "tensor patch reconstruction not implemented for this format".into(),
        )),
    }
}

fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().into()
}

fn hex_encode(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
