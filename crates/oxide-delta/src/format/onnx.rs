//! ONNX protobuf parser.

use crate::format::TensorInfo;
use crate::{DeltaError, Result};
use prost::Message;

// Generated from onnx_ml.proto by prost
pub mod proto {
    #![allow(clippy::all, clippy::nursery)]
    include!(concat!(env!("OUT_DIR"), "/onnx.rs"));
}

/// Heuristic: ONNX files start with protobuf field 1 (ir_version) which
/// encodes as varint tag 0x08. Not bulletproof but good enough in practice.
pub fn is_onnx(data: &[u8]) -> bool {
    // ONNX protobuf typically starts with field 1 (ir_version, varint)
    // tag = (1 << 3) | 0 = 0x08
    data.len() > 4 && data[0] == 0x08
}

/// Extract all initializer tensors from an ONNX model.
pub fn extract_tensors(data: &[u8]) -> Result<Vec<TensorInfo>> {
    let model = proto::ModelProto::decode(data)?;
    let graph = model.graph.ok_or_else(|| {
        DeltaError::InvalidPatch("ONNX model has no graph".to_string())
    })?;

    let mut tensors = Vec::with_capacity(graph.initializer.len());
    for init in &graph.initializer {
        let name = init.name.clone().unwrap_or_default();
        let raw = tensor_raw_bytes(init);
        tensors.push(TensorInfo {
            name,
            data: raw,
        });
    }

    Ok(tensors)
}

/// Serialize the ONNX model structure with all tensor data removed.
/// This "skeleton" captures the graph topology, node definitions, metadata —
/// everything except the weight bytes.
pub fn serialize_skeleton(data: &[u8]) -> Result<Vec<u8>> {
    let mut model = proto::ModelProto::decode(data)?;
    if let Some(ref mut graph) = model.graph {
        for init in &mut graph.initializer {
            init.raw_data = None;
            init.float_data.clear();
            init.double_data.clear();
            init.int32_data.clear();
            init.int64_data.clear();
            init.uint64_data.clear();
        }
    }
    Ok(model.encode_to_vec())
}

/// Reconstruct a full ONNX file from a skeleton and tensor data.
pub fn reconstruct(skeleton: &[u8], tensors: &[TensorInfo]) -> Result<Vec<u8>> {
    let mut model = proto::ModelProto::decode(skeleton)?;
    let graph = model.graph.as_mut().ok_or_else(|| {
        DeltaError::InvalidPatch("skeleton has no graph".to_string())
    })?;

    let tensor_map: std::collections::HashMap<&str, &[u8]> = tensors
        .iter()
        .map(|t| (t.name.as_str(), t.data.as_slice()))
        .collect();

    for init in &mut graph.initializer {
        let name = init.name.as_deref().unwrap_or_default();
        if let Some(data) = tensor_map.get(name) {
            init.raw_data = Some(data.to_vec());
        }
    }

    Ok(model.encode_to_vec())
}

/// Extract raw bytes from a TensorProto, regardless of storage format.
fn tensor_raw_bytes(tensor: &proto::TensorProto) -> Vec<u8> {
    // Prefer raw_data (most common in exported models)
    if let Some(ref raw) = tensor.raw_data {
        if !raw.is_empty() {
            return raw.clone();
        }
    }

    // Fall back to typed fields
    if !tensor.float_data.is_empty() {
        return tensor
            .float_data
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
    }
    if !tensor.double_data.is_empty() {
        return tensor
            .double_data
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();
    }
    if !tensor.int32_data.is_empty() {
        return tensor
            .int32_data
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
    }
    if !tensor.int64_data.is_empty() {
        return tensor
            .int64_data
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
    }
    if !tensor.uint64_data.is_empty() {
        return tensor
            .uint64_data
            .iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();
    }

    Vec::new()
}
