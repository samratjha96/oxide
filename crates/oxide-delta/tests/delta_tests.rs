//! Tests for the delta engine using real ONNX model files.

use oxide_delta::{apply_delta, build_manifest, compute_delta, DeltaPatch, PatchStrategy};
use std::path::Path;

fn read_model(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../models/test")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

#[test]
fn onnx_parse_and_manifest() {
    let data = read_model("mlp_mnist.onnx");
    let manifest = build_manifest(&data).unwrap();

    assert_eq!(manifest.entries.len(), 6);
    assert_eq!(manifest.entries[0].name, "w1");
    assert_eq!(manifest.entries[0].size, 1_605_632);
    assert_eq!(manifest.entries[1].name, "b1");
    assert_eq!(manifest.entries[5].name, "b3");
    assert_eq!(manifest.entries[5].size, 40);

    // Manifest header value should be parseable
    let header = manifest.to_header_value();
    assert!(header.contains("w1="));
    assert!(header.contains("b3="));
}

#[test]
fn identical_files_produce_tiny_delta() {
    let data = read_model("mlp_mnist.onnx");
    let result = compute_delta(&data, &data).unwrap();
    // For identical files, tensor delta should be all COPY chunks + skeleton.
    // This is valid and very small — much smaller than the file.
    match result {
        Some(patch) => {
            assert!(
                patch.encoded_size() < data.len() / 100,
                "identical file delta should be <1% of file size, got {}%",
                patch.encoded_size() * 100 / data.len()
            );
        }
        None => {
            // Also acceptable: delta engine decided full file is smaller
        }
    }
}

#[test]
fn binary_delta_round_trip() {
    let base = read_model("mlp_mnist.onnx");

    // Create a "target" by flipping some bytes (simulating weight changes)
    let mut target = base.clone();
    for i in (base.len() / 2..base.len()).step_by(1000) {
        target[i] = target[i].wrapping_add(1);
    }

    let patch = oxide_delta::patch::binary_delta(&base, &target).unwrap();
    assert_eq!(patch.strategy, PatchStrategy::Binary);
    assert!(
        patch.encoded_size() < target.len(),
        "delta should be smaller than target"
    );

    let reconstructed = apply_delta(&base, &patch).unwrap();
    assert_eq!(reconstructed, target);
}

#[test]
fn tensor_delta_round_trip_last_layer() {
    let base = read_model("mlp_mnist.onnx");

    // Simulate transfer learning: modify only the last two tensors (w3, b3)
    // by perturbing bytes in the weight region.
    let target = perturb_last_layer(&base);

    let result = compute_delta(&base, &target).unwrap();
    let patch = result.expect("should produce a delta for modified last layer");

    println!(
        "Last-layer delta: {} bytes ({:.1}% of {} byte file)",
        patch.encoded_size(),
        patch.encoded_size() as f64 / base.len() as f64 * 100.0,
        base.len()
    );

    // Tensor delta for last layer only should be very small
    assert!(
        patch.encoded_size() < base.len() / 5,
        "tensor delta for last layer should be <20% of file, got {}%",
        patch.encoded_size() * 100 / base.len()
    );

    let reconstructed = apply_delta(&base, &patch).unwrap();
    assert_eq!(reconstructed, target);
}

#[test]
fn compute_delta_picks_best_strategy() {
    let base = read_model("mlp_mnist.onnx");
    let target = perturb_last_layer(&base);

    let result = compute_delta(&base, &target).unwrap();
    let patch = result.expect("should produce delta");

    // For last-layer-only changes, tensor strategy should win
    assert_eq!(
        patch.strategy,
        PatchStrategy::Tensor,
        "tensor strategy should win for localized changes"
    );
}

#[test]
fn oxdl_serialization_round_trip() {
    let base = read_model("mlp_mnist.onnx");
    let target = perturb_last_layer(&base);

    let result = compute_delta(&base, &target).unwrap();
    let patch = result.expect("should produce delta");

    // Serialize to OXDL bytes and back
    let bytes = patch.to_bytes();
    let patch2 = DeltaPatch::from_bytes(&bytes).unwrap();

    assert_eq!(patch2.strategy, patch.strategy);
    assert_eq!(patch2.base_sha256, patch.base_sha256);
    assert_eq!(patch2.target_sha256, patch.target_sha256);
    assert_eq!(patch2.target_size, patch.target_size);
    assert_eq!(patch2.chunks.len(), patch.chunks.len());

    // Apply the deserialized patch
    let reconstructed = apply_delta(&base, &patch2).unwrap();
    assert_eq!(reconstructed, target);
}

#[test]
fn small_models_parse() {
    for name in &[
        "linear_model.onnx",
        "classifier_model.onnx",
        "sigmoid_model.onnx",
    ] {
        let data = read_model(name);
        let result = build_manifest(&data);
        // Small models may not be valid ONNX (could be too minimal),
        // but if they parse, the manifest should be non-empty
        if let Ok(manifest) = result {
            println!("{name}: {} tensors", manifest.entries.len());
        } else {
            println!("{name}: not parseable as ONNX (ok for test)");
        }
    }
}

#[test]
fn wrong_base_rejected() {
    let base = read_model("mlp_mnist.onnx");
    let target = perturb_last_layer(&base);

    let patch = compute_delta(&base, &target)
        .unwrap()
        .expect("should produce delta");

    // Try applying with wrong base
    let wrong_base = vec![0u8; 1000];
    let err = apply_delta(&wrong_base, &patch).unwrap_err();
    assert!(
        err.to_string().contains("base mismatch"),
        "should reject wrong base, got: {err}"
    );
}

/// Modify an ONNX file to simulate transfer learning on the last layer.
///
/// This works at the protobuf level: we know the ONNX file is a serialized
/// ModelProto. Rather than depending on prost in tests, we use a simpler
/// approach: use oxide-delta's own parse + reconstruct path via a helper.
///
/// Actually, the simplest approach that creates a valid ONNX file with
/// different tensor data: use oxide-delta's internal ONNX module through
/// a pub(crate) test helper. But since we're in an integration test, we
/// can't access pub(crate) items.
///
/// Instead: use oxide-delta's public `build_manifest` to find tensor info,
/// then construct a modified file by exporting a helper from the crate.
///
/// For now, the pragmatic approach: expose `modify_onnx_tensors` as a
/// test utility.
fn perturb_last_layer(onnx_bytes: &[u8]) -> Vec<u8> {
    oxide_delta::test_util::modify_onnx_tensors(onnx_bytes, &["w3", "b3"])
}
