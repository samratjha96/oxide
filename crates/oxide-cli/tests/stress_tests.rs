//! Stress tests and edge case tests for the Oxide runtime.

use std::path::Path;
use std::sync::Arc;
use std::thread;

fn test_models_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/test"))
}

mod stress_tests {
    use super::*;
    use oxide_runtime::InferenceEngine;

    #[test]
    fn test_high_throughput_inference() {
        let engine = InferenceEngine::new(0);
        let path = test_models_dir().join("sigmoid_model.onnx");
        let info = engine.load_model(&path).unwrap();
        let input = [0.5f32, 1.0, -0.5, 2.0];
        let shape = [1usize, 4];

        // Run 10,000 inferences
        for _ in 0..10_000 {
            let result = engine.infer(&info.id, &input, &shape).unwrap();
            assert_eq!(result.outputs.len(), 4);
        }

        let metrics = engine.get_metrics(&info.id).unwrap();
        assert_eq!(metrics.total_inferences, 10_000);
        assert_eq!(metrics.failed_inferences, 0);
        assert!(metrics.throughput_per_sec > 0.0);
    }

    #[test]
    fn test_model_load_unload_cycle() {
        let engine = InferenceEngine::new(1);
        let path = test_models_dir().join("add_model.onnx");

        // Load and unload 50 times
        for _i in 0..50 {
            let info = engine.load_model(&path).unwrap();
            let result = engine.infer(&info.id, &[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
            assert_eq!(result.outputs, vec![4.0, 5.0, 6.0, 7.0]);
            engine.unload_model(&info.id).unwrap();
        }
    }

    #[test]
    fn test_concurrent_inference_readonly() {
        // This test verifies that multiple threads can read the engine concurrently
        // Note: InferenceEngine uses RwLock, so we test that concurrent writes
        // (which happen during infer to update metrics) don't cause issues
        let engine = Arc::new(InferenceEngine::new(0));
        let path = test_models_dir().join("sigmoid_model.onnx");
        let info = engine.load_model(&path).unwrap();
        let model_id = info.id;

        let mut handles = Vec::new();
        for t in 0..4 {
            let engine = engine.clone();
            let model_id = model_id.clone();
            let handle = thread::spawn(move || {
                let input = [t as f32 * 0.1, 0.5, 1.0, -1.0];
                for _ in 0..1000 {
                    let result = engine.infer(&model_id, &input, &[1, 4]).unwrap();
                    assert_eq!(result.outputs.len(), 4);
                }
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let metrics = engine.get_metrics(&model_id).unwrap();
        assert_eq!(metrics.total_inferences, 4000);
        assert_eq!(metrics.failed_inferences, 0);
    }

    #[test]
    fn test_large_model_inference() {
        let engine = InferenceEngine::new(0);
        let path = test_models_dir().join("mlp_mnist.onnx");

        if !path.exists() {
            // Skip if benchmark model not generated
            return;
        }

        let info = engine.load_model(&path).unwrap();
        assert_eq!(info.inputs[0].shape, vec![1, 784]);
        assert_eq!(info.outputs[0].shape, vec![1, 10]);

        // Run inference with random-ish input
        let input: Vec<f32> = (0..784).map(|i| i as f32 / 784.0).collect();
        let result = engine.infer(&info.id, &input, &[1, 784]).unwrap();

        // Output should be softmax (sum ≈ 1.0)
        let sum: f32 = result.outputs.iter().sum();
        assert!(
            (sum - 1.0).abs() < 0.01,
            "Softmax should sum to ~1.0, got {}",
            sum
        );

        // Run 100 iterations to check stability
        for _ in 0..100 {
            let result = engine.infer(&info.id, &input, &[1, 784]).unwrap();
            assert_eq!(result.outputs.len(), 10);
        }
    }
}

mod encryption_stress {
    use oxide_security::encryption::{encrypt_data, decrypt_data};
    use oxide_security::EncryptionKey;

    #[test]
    fn test_encrypt_decrypt_many_keys() {
        let plaintext = b"This is a test model file for encryption stress testing";

        for _ in 0..100 {
            let key = EncryptionKey::generate();
            let encrypted = encrypt_data(&key, plaintext).unwrap();
            let decrypted = decrypt_data(&key, &encrypted).unwrap();
            assert_eq!(decrypted, plaintext);
        }
    }

    #[test]
    fn test_encrypt_varying_sizes() {
        let key = EncryptionKey::generate();

        for size in [0, 1, 15, 16, 17, 255, 256, 1023, 1024, 4096, 65536] {
            let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
            let encrypted = encrypt_data(&key, &data).unwrap();
            let decrypted = decrypt_data(&key, &encrypted).unwrap();
            assert_eq!(decrypted, data, "Failed for size {}", size);
        }
    }

    #[test]
    fn test_empty_plaintext() {
        let key = EncryptionKey::generate();
        let encrypted = encrypt_data(&key, &[]).unwrap();
        let decrypted = decrypt_data(&key, &encrypted).unwrap();
        assert!(decrypted.is_empty());
    }
}

mod ota_stress {
    use oxide_core::model::{ModelId, ModelVersion};
    use oxide_network::ota::{OtaUpdater, UpdatePackage, UpdateStatus};
    use sha2::{Digest, Sha256};

    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn test_sequential_updates() {
        let dir = tempfile::TempDir::new().unwrap();
        let updater = OtaUpdater::new(dir.path()).unwrap();

        for version in 1..=20 {
            let data = format!("model data version {}", version).into_bytes();
            let hash = sha256_hex(&data);

            let package = UpdatePackage {
                model_id: ModelId::from("sequential-model"),
                new_version: ModelVersion::from(format!("v{}.0.0", version).as_str()),
                previous_version: if version > 1 {
                    Some(ModelVersion::from(
                        format!("v{}.0.0", version - 1).as_str(),
                    ))
                } else {
                    None
                },
                sha256: hash,
                size_bytes: data.len() as u64,
                encrypted: false,
            };

            let mut state = updater.stage_update(&package, &data).unwrap();
            let path = updater.apply_update(&mut state).unwrap();
            assert!(path.exists());
            assert_eq!(state.status, UpdateStatus::Complete);

            let stored = std::fs::read(&path).unwrap();
            assert_eq!(stored, data);
        }
    }

    #[test]
    fn test_rollback_chain() {
        let dir = tempfile::TempDir::new().unwrap();
        let updater = OtaUpdater::new(dir.path()).unwrap();

        // Deploy v1
        let v1 = b"v1 model".to_vec();
        let pkg1 = UpdatePackage {
            model_id: ModelId::from("model"),
            new_version: ModelVersion::from("v1"),
            previous_version: None,
            sha256: sha256_hex(&v1),
            size_bytes: v1.len() as u64,
            encrypted: false,
        };
        let mut s = updater.stage_update(&pkg1, &v1).unwrap();
        updater.apply_update(&mut s).unwrap();

        // Deploy v2
        let v2 = b"v2 model".to_vec();
        let pkg2 = UpdatePackage {
            model_id: ModelId::from("model"),
            new_version: ModelVersion::from("v2"),
            previous_version: Some(ModelVersion::from("v1")),
            sha256: sha256_hex(&v2),
            size_bytes: v2.len() as u64,
            encrypted: false,
        };
        let mut s = updater.stage_update(&pkg2, &v2).unwrap();
        updater.apply_update(&mut s).unwrap();

        // Deploy v3
        let v3 = b"v3 model".to_vec();
        let pkg3 = UpdatePackage {
            model_id: ModelId::from("model"),
            new_version: ModelVersion::from("v3"),
            previous_version: Some(ModelVersion::from("v2")),
            sha256: sha256_hex(&v3),
            size_bytes: v3.len() as u64,
            encrypted: false,
        };
        let mut s = updater.stage_update(&pkg3, &v3).unwrap();
        updater.apply_update(&mut s).unwrap();

        // Rollback to v2
        let path = updater
            .rollback(&ModelId::from("model"), &ModelVersion::from("v2"))
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), v2);
    }
}

mod fleet_stress {
    use oxide_control::fleet_manager::{DeploymentRequest, FleetManager};
    use oxide_control::registry::DeviceRegistry;
    use oxide_core::device::{Device, DeviceId, DeviceStatus};
    use oxide_core::fleet::{Fleet, FleetId, RolloutStrategy};
    use oxide_core::model::{ModelId, ModelVersion};
    use std::sync::Arc;

    #[test]
    fn test_large_fleet_deployment() {
        let registry = Arc::new(DeviceRegistry::new());

        // Register 100 devices
        for i in 0..100 {
            let mut device = Device::new(
                DeviceId::from(format!("device-{:04}", i).as_str()),
                format!("Device {}", i),
            );
            device.status = DeviceStatus::Online;
            registry.register(device).unwrap();
        }

        let manager = FleetManager::new(registry);
        let mut fleet = Fleet::new(FleetId::from("mega-fleet"), "Mega Fleet");
        for i in 0..100 {
            fleet.add_device(DeviceId::from(format!("device-{:04}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        // Deploy to all 100
        let request = DeploymentRequest {
            model_id: ModelId::from("model"),
            model_version: ModelVersion::from("v1.0.0"),
            fleet_id: FleetId::from("mega-fleet"),
            strategy: RolloutStrategy::AllAtOnce,
        };
        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 100);
        assert_eq!(result.successful, 100);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_mixed_status_fleet() {
        let registry = Arc::new(DeviceRegistry::new());

        let statuses = [
            DeviceStatus::Online,
            DeviceStatus::Online,
            DeviceStatus::Offline,
            DeviceStatus::Error,
            DeviceStatus::Online,
            DeviceStatus::Unknown,
            DeviceStatus::Online,
            DeviceStatus::Offline,
            DeviceStatus::Online,
            DeviceStatus::Updating,
        ];

        for (i, status) in statuses.iter().enumerate() {
            let mut device = Device::new(
                DeviceId::from(format!("dev-{}", i).as_str()),
                format!("Device {}", i),
            );
            device.status = *status;
            registry.register(device).unwrap();
        }

        let manager = FleetManager::new(registry);
        let mut fleet = Fleet::new(FleetId::from("mixed"), "Mixed Fleet");
        for i in 0..10 {
            fleet.add_device(DeviceId::from(format!("dev-{}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        let request = DeploymentRequest {
            model_id: ModelId::from("model"),
            model_version: ModelVersion::from("v1.0.0"),
            fleet_id: FleetId::from("mixed"),
            strategy: RolloutStrategy::AllAtOnce,
        };
        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 10);
        // Online (5) + Unknown (1) = 6 successful
        // Offline (2) + Error (1) + Updating (1) = 4 failed
        assert_eq!(result.successful, 6);
        assert_eq!(result.failed, 4);
    }
}
