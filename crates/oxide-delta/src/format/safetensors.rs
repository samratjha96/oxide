//! SafeTensors format parser.
//!
//! Format: [8-byte header length (LE u64)] [JSON header] [tensor data]
//!
//! The JSON header maps tensor names to `{ dtype, shape, data_offsets: [start, end] }`.
//! Offsets are relative to the start of the tensor data region (after the header).

use crate::format::TensorInfo;
use crate::{DeltaError, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

const HEADER_SIZE_LEN: usize = 8;

/// Check if data looks like SafeTensors format.
/// SafeTensors starts with an 8-byte LE u64 header length, followed by '{'.
pub fn is_safetensors(data: &[u8]) -> bool {
    if data.len() < HEADER_SIZE_LEN + 1 {
        return false;
    }
    let header_len = u64::from_le_bytes(data[..8].try_into().unwrap_or_default()) as usize;
    // Sanity: header should be reasonable size and JSON
    header_len > 2
        && header_len < 100_000_000
        && data.len() >= HEADER_SIZE_LEN + header_len
        && data[HEADER_SIZE_LEN] == b'{'
}

#[derive(Debug, Deserialize)]
struct SafeTensorEntry {
    #[allow(dead_code)]
    dtype: String,
    #[allow(dead_code)]
    shape: Vec<usize>,
    data_offsets: [usize; 2],
}

/// Extract tensors from a SafeTensors file.
pub fn extract_tensors(data: &[u8]) -> Result<Vec<TensorInfo>> {
    if data.len() < HEADER_SIZE_LEN {
        return Err(DeltaError::SafeTensors("file too small".to_string()));
    }

    let header_len =
        u64::from_le_bytes(data[..8].try_into().map_err(|_| {
            DeltaError::SafeTensors("invalid header length".to_string())
        })?) as usize;

    let header_end = HEADER_SIZE_LEN + header_len;
    if data.len() < header_end {
        return Err(DeltaError::SafeTensors("truncated header".to_string()));
    }

    let header_json = &data[HEADER_SIZE_LEN..header_end];
    let header: BTreeMap<String, serde_json::Value> = serde_json::from_slice(header_json)
        .map_err(|e| DeltaError::SafeTensors(format!("invalid JSON header: {e}")))?;

    let data_start = header_end;
    let mut tensors = Vec::new();

    for (name, value) in &header {
        // Skip __metadata__ key
        if name == "__metadata__" {
            continue;
        }

        let entry: SafeTensorEntry = serde_json::from_value(value.clone())
            .map_err(|e| DeltaError::SafeTensors(format!("invalid tensor entry '{name}': {e}")))?;

        let [start, end] = entry.data_offsets;
        let abs_start = data_start + start;
        let abs_end = data_start + end;

        if abs_end > data.len() {
            return Err(DeltaError::SafeTensors(format!(
                "tensor '{name}' data offsets [{start}, {end}] exceed file size"
            )));
        }

        tensors.push(TensorInfo {
            name: name.clone(),
            data: data[abs_start..abs_end].to_vec(),
        });
    }

    Ok(tensors)
}

/// Serialize the SafeTensors "skeleton" — the header JSON + zero-length placeholders.
/// For SafeTensors, the skeleton IS the original file bytes (header + data layout is fixed).
/// We store the full file as skeleton because SafeTensors' layout is positional.
pub fn serialize_skeleton(data: &[u8]) -> Result<Vec<u8>> {
    // For SafeTensors, reconstruction works differently than ONNX.
    // The file layout is: [header_len][json_header][tensor_data_concatenated]
    // The JSON header has exact byte offsets, so we store the full original bytes
    // as the skeleton. On reconstruction, we patch tensor regions in place.
    Ok(data.to_vec())
}
