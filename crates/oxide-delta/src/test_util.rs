//! Test utilities for creating modified ONNX files.
//!
//! Exposed as public so integration tests can create test fixtures.

use crate::format::onnx::proto;
use prost::Message;

/// Deserialize an ONNX model, perturb the specified tensors, and reserialize.
///
/// Perturbation: flips ~5% of bytes in each named tensor's raw_data.
pub fn modify_onnx_tensors(onnx_bytes: &[u8], tensor_names: &[&str]) -> Vec<u8> {
    let mut model = proto::ModelProto::decode(onnx_bytes)
        .expect("test_util: failed to decode ONNX model");

    let graph = model
        .graph
        .as_mut()
        .expect("test_util: model has no graph");

    for init in &mut graph.initializer {
        let name = init.name.as_deref().unwrap_or_default();
        if tensor_names.contains(&name) {
            if let Some(ref mut raw) = init.raw_data {
                for i in (0..raw.len()).step_by(20) {
                    raw[i] = raw[i].wrapping_add(1);
                }
            }
        }
    }

    model.encode_to_vec()
}
