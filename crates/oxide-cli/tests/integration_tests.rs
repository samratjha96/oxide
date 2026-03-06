//! Integration tests for the Oxide edge AI runtime.
//!
//! These tests exercise the full stack from model loading through inference,
//! encryption, OTA updates, and fleet management.

use std::path::Path;

/// Path to test models directory.
fn test_models_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../models/test"))
}

mod model_loading {
    use super::*;
    use oxide_models::OnnxModel;

    #[test]
    fn test_load_add_model() {
        let path = test_models_dir().join("add_model.onnx");
        let model = OnnxModel::load(&path).expect("Failed to load add model");
        let info = model.info();
        assert_eq!(info.format, oxide_core::model::ModelFormat::Onnx);
        assert_eq!(info.inputs.len(), 1);
        assert_eq!(info.outputs.len(), 1);
        assert_eq!(info.inputs[0].shape, vec![1, 4]);
    }

    #[test]
    fn test_load_linear_model() {
        let path = test_models_dir().join("linear_model.onnx");
        let model = OnnxModel::load(&path).expect("Failed to load linear model");
        let info = model.info();
        assert_eq!(info.inputs[0].shape, vec![1, 8]);
        assert_eq!(info.outputs[0].shape, vec![1, 4]);
    }

    #[test]
    fn test_load_classifier_model() {
        let path = test_models_dir().join("classifier_model.onnx");
        let model = OnnxModel::load(&path).expect("Failed to load classifier model");
        let info = model.info();
        assert_eq!(info.inputs[0].shape, vec![1, 16]);
        assert_eq!(info.outputs[0].shape, vec![1, 4]);
    }

    #[test]
    fn test_load_sigmoid_model() {
        let path = test_models_dir().join("sigmoid_model.onnx");
        let model = OnnxModel::load(&path).expect("Failed to load sigmoid model");
        let info = model.info();
        assert_eq!(info.inputs[0].shape, vec![1, 4]);
        assert_eq!(info.outputs[0].shape, vec![1, 4]);
    }

    #[test]
    fn test_load_from_bytes() {
        let path = test_models_dir().join("add_model.onnx");
        let bytes = std::fs::read(&path).unwrap();
        let model = OnnxModel::load_from_bytes(&bytes, "add-from-bytes")
            .expect("Failed to load from bytes");
        assert_eq!(model.info().id.0, "add-from-bytes");
    }
}

mod inference {
    use super::*;
    use oxide_models::OnnxModel;

    #[test]
    fn test_add_model_inference() {
        let path = test_models_dir().join("add_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let output = model.run_f32(&[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(output, vec![4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn test_add_model_negative_input() {
        let path = test_models_dir().join("add_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let output = model.run_f32(&[-1.0, -2.0, 0.0, 100.0], &[1, 4]).unwrap();
        assert_eq!(output, vec![2.0, 1.0, 3.0, 103.0]);
    }

    #[test]
    fn test_sigmoid_model_inference() {
        let path = test_models_dir().join("sigmoid_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let output = model.run_f32(&[0.0, 1.0, -1.0, 10.0], &[1, 4]).unwrap();
        // sigmoid(0) = 0.5, sigmoid(1) ≈ 0.731, sigmoid(-1) ≈ 0.269, sigmoid(10) ≈ 1.0
        assert!((output[0] - 0.5).abs() < 0.01);
        assert!((output[1] - 0.731).abs() < 0.01);
        assert!((output[2] - 0.269).abs() < 0.01);
        assert!((output[3] - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_classifier_softmax_sums_to_one() {
        let path = test_models_dir().join("classifier_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let input = vec![0.0f32; 16];
        let output = model.run_f32(&input, &[1, 16]).unwrap();
        let sum: f32 = output.iter().sum();
        assert!(
            (sum - 1.0).abs() < 0.01,
            "Softmax output should sum to 1.0, got {}",
            sum
        );
    }

    #[test]
    fn test_linear_model_inference() {
        let path = test_models_dir().join("linear_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let input = vec![1.0f32; 8];
        let output = model.run_f32(&input, &[1, 8]).unwrap();
        assert_eq!(output.len(), 4);
        // Output should be non-zero (weights are random but deterministic)
    }

    #[test]
    fn test_inference_wrong_shape_fails() {
        let path = test_models_dir().join("add_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        // Wrong shape (expects [1,4], giving [1,3])
        let result = model.run_f32(&[1.0, 2.0, 3.0], &[1, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_repeated_inference_consistency() {
        let path = test_models_dir().join("add_model.onnx");
        let model = OnnxModel::load(&path).unwrap();
        let input = [5.0f32, 10.0, 15.0, 20.0];

        for _ in 0..100 {
            let output = model.run_f32(&input, &[1, 4]).unwrap();
            assert_eq!(output, vec![8.0, 13.0, 18.0, 23.0]);
        }
    }
}

mod engine_integration {
    use super::*;
    
    use oxide_runtime::InferenceEngine;

    #[test]
    fn test_engine_load_and_infer() {
        let engine = InferenceEngine::new(1);
        let path = test_models_dir().join("add_model.onnx");
        let info = engine.load_model(&path).unwrap();

        let result = engine.infer(&info.id, &[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(result.outputs, vec![4.0, 5.0, 6.0, 7.0]);
        assert!(result.latency_us > 0.0);
    }

    #[test]
    fn test_engine_metrics_tracking() {
        let engine = InferenceEngine::new(1);
        let path = test_models_dir().join("sigmoid_model.onnx");
        let info = engine.load_model(&path).unwrap();

        // Run multiple inferences
        for _ in 0..50 {
            engine.infer(&info.id, &[0.0, 1.0, 2.0, 3.0], &[1, 4]).unwrap();
        }

        let metrics = engine.get_metrics(&info.id).unwrap();
        assert_eq!(metrics.total_inferences, 50);
        assert_eq!(metrics.failed_inferences, 0);
        assert!(metrics.avg_latency_us > 0.0);
        assert!(metrics.p50_latency_us > 0.0);
        assert!(metrics.p99_latency_us >= metrics.p50_latency_us);
        assert!(metrics.throughput_per_sec > 0.0);
    }

    #[test]
    fn test_engine_multiple_models() {
        let engine = InferenceEngine::new(1);

        let add_path = test_models_dir().join("add_model.onnx");
        let sig_path = test_models_dir().join("sigmoid_model.onnx");

        let add_info = engine.load_model(&add_path).unwrap();
        let sig_info = engine.load_model(&sig_path).unwrap();

        let models = engine.list_models().unwrap();
        assert_eq!(models.len(), 2);

        // Run inference on both
        let add_result = engine.infer(&add_info.id, &[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(add_result.outputs, vec![4.0, 5.0, 6.0, 7.0]);

        let sig_result = engine.infer(&sig_info.id, &[0.0, 0.0, 0.0, 0.0], &[1, 4]).unwrap();
        assert!((sig_result.outputs[0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_engine_unload_model() {
        let engine = InferenceEngine::new(1);
        let path = test_models_dir().join("add_model.onnx");
        let info = engine.load_model(&path).unwrap();

        assert!(engine.is_loaded(&info.id));
        engine.unload_model(&info.id).unwrap();
        assert!(!engine.is_loaded(&info.id));

        // Inference should fail after unload
        let result = engine.infer(&info.id, &[1.0, 2.0, 3.0, 4.0], &[1, 4]);
        assert!(result.is_err());
    }

    #[test]
    fn test_engine_hot_swap() {
        let engine = InferenceEngine::new(1);
        let path = test_models_dir().join("add_model.onnx");

        // Load and verify
        let info = engine.load_model(&path).unwrap();
        let r1 = engine.infer(&info.id, &[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(r1.outputs, vec![4.0, 5.0, 6.0, 7.0]);

        // Unload and reload (simulates hot swap)
        engine.unload_model(&info.id).unwrap();
        let info2 = engine.load_model(&path).unwrap();
        let r2 = engine.infer(&info2.id, &[10.0, 20.0, 30.0, 40.0], &[1, 4]).unwrap();
        assert_eq!(r2.outputs, vec![13.0, 23.0, 33.0, 43.0]);
    }
}

mod security_integration {
    use super::*;
    use oxide_security::*;

    #[test]
    fn test_encrypt_decrypt_model_file() {
        let key = EncryptionKey::generate();
        let model_path = test_models_dir().join("add_model.onnx");
        let original_data = std::fs::read(&model_path).unwrap();

        // Encrypt in memory
        let encrypted = oxide_security::encryption::encrypt_data(&key, &original_data).unwrap();
        assert_ne!(&encrypted, &original_data);

        // Decrypt in memory
        let decrypted = oxide_security::encryption::decrypt_data(&key, &encrypted).unwrap();
        assert_eq!(decrypted, original_data);

        // Verify decrypted model is loadable
        let model = oxide_models::OnnxModel::load_from_bytes(&decrypted, "decrypted-model").unwrap();
        let output = model.run_f32(&[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(output, vec![4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn test_wrong_key_cannot_load_model() {
        let key1 = EncryptionKey::generate();
        let key2 = EncryptionKey::generate();
        let model_path = test_models_dir().join("add_model.onnx");
        let original_data = std::fs::read(&model_path).unwrap();

        let encrypted = oxide_security::encryption::encrypt_data(&key1, &original_data).unwrap();
        let result = oxide_security::encryption::decrypt_data(&key2, &encrypted);
        assert!(result.is_err(), "Decryption with wrong key should fail");
    }

    #[test]
    fn test_integrity_verification() {
        let model_path = test_models_dir().join("add_model.onnx");
        let hash = oxide_security::integrity::sha256_file(&model_path).unwrap();
        assert!(verify_sha256(&model_path, &hash).unwrap());
        assert!(!verify_sha256(&model_path, "badhash").unwrap());
    }
}

mod ota_integration {
    use super::*;
    use oxide_core::model::{ModelId, ModelVersion};
    use oxide_network::OtaUpdater;
    use sha2::{Digest, Sha256};

    fn sha256_hex(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn test_full_ota_update_cycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let updater = OtaUpdater::new(dir.path()).unwrap();

        // Simulate deploying a model
        let model_data = std::fs::read(test_models_dir().join("add_model.onnx")).unwrap();
        let hash = sha256_hex(&model_data);

        let package = oxide_network::ota::UpdatePackage {
            model_id: ModelId::from("face-detection"),
            new_version: ModelVersion::from("v2.0.0"),
            previous_version: None,
            sha256: hash,
            size_bytes: model_data.len() as u64,
            encrypted: false,
        };

        // Stage
        let mut state = updater.stage_update(&package, &model_data).unwrap();
        assert_eq!(
            state.status,
            oxide_network::ota::UpdateStatus::Verifying
        );

        // Apply
        let active_path = updater.apply_update(&mut state).unwrap();
        assert!(active_path.exists());
        assert_eq!(
            state.status,
            oxide_network::ota::UpdateStatus::Complete
        );

        // Verify the deployed model works
        let model = oxide_models::OnnxModel::load(&active_path).unwrap();
        let output = model.run_f32(&[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(output, vec![4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn test_ota_update_with_rollback() {
        let dir = tempfile::TempDir::new().unwrap();
        let updater = OtaUpdater::new(dir.path()).unwrap();

        // Deploy v1
        let v1_data = std::fs::read(test_models_dir().join("add_model.onnx")).unwrap();
        let v1_hash = sha256_hex(&v1_data);
        let v1_pkg = oxide_network::ota::UpdatePackage {
            model_id: ModelId::from("model"),
            new_version: ModelVersion::from("v1.0.0"),
            previous_version: None,
            sha256: v1_hash,
            size_bytes: v1_data.len() as u64,
            encrypted: false,
        };
        let mut v1_state = updater.stage_update(&v1_pkg, &v1_data).unwrap();
        updater.apply_update(&mut v1_state).unwrap();

        // Deploy v2
        let v2_data = std::fs::read(test_models_dir().join("sigmoid_model.onnx")).unwrap();
        let v2_hash = sha256_hex(&v2_data);
        let v2_pkg = oxide_network::ota::UpdatePackage {
            model_id: ModelId::from("model"),
            new_version: ModelVersion::from("v2.0.0"),
            previous_version: Some(ModelVersion::from("v1.0.0")),
            sha256: v2_hash,
            size_bytes: v2_data.len() as u64,
            encrypted: false,
        };
        let mut v2_state = updater.stage_update(&v2_pkg, &v2_data).unwrap();
        updater.apply_update(&mut v2_state).unwrap();

        // Rollback to v1
        let rollback_path = updater
            .rollback(&ModelId::from("model"), &ModelVersion::from("v1.0.0"))
            .unwrap();

        // Verify rollback restored v1 (the add model)
        let model = oxide_models::OnnxModel::load(&rollback_path).unwrap();
        let output = model.run_f32(&[1.0, 2.0, 3.0, 4.0], &[1, 4]).unwrap();
        assert_eq!(output, vec![4.0, 5.0, 6.0, 7.0]);
    }
}

mod fleet_integration {
    use oxide_control::fleet_manager::{DeploymentRequest, FleetManager};
    use oxide_control::registry::DeviceRegistry;
    use oxide_core::device::{Device, DeviceId, DeviceStatus};
    use oxide_core::fleet::{Fleet, FleetId, RolloutStrategy};
    use oxide_core::model::{ModelId, ModelVersion};
    use std::sync::Arc;

    #[test]
    fn test_fleet_deployment_scenario() {
        // Set up registry with devices
        let registry = Arc::new(DeviceRegistry::new());
        for i in 0..20 {
            let mut device = Device::new(
                DeviceId::from(format!("cam-{:03}", i).as_str()),
                format!("Camera {}", i),
            );
            device.status = DeviceStatus::Online;
            registry.register(device).unwrap();
        }

        // Create fleet
        let manager = FleetManager::new(registry);
        let mut fleet = Fleet::new(FleetId::from("warehouse"), "Warehouse Cameras");
        for i in 0..20 {
            fleet.add_device(DeviceId::from(format!("cam-{:03}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        // Deploy with all-at-once
        let request = DeploymentRequest {
            model_id: ModelId::from("defect-detection"),
            model_version: ModelVersion::from("v3.0.0"),
            fleet_id: FleetId::from("warehouse"),
            strategy: RolloutStrategy::AllAtOnce,
        };
        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 20);
        assert_eq!(result.successful, 20);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_canary_deployment_with_offline_devices() {
        let registry = Arc::new(DeviceRegistry::new());

        // 8 online, 2 offline
        for i in 0..10 {
            let mut device = Device::new(
                DeviceId::from(format!("node-{}", i).as_str()),
                format!("Node {}", i),
            );
            device.status = if i < 8 {
                DeviceStatus::Online
            } else {
                DeviceStatus::Offline
            };
            registry.register(device).unwrap();
        }

        let manager = FleetManager::new(registry);
        let mut fleet = Fleet::new(FleetId::from("prod"), "Production");
        for i in 0..10 {
            fleet.add_device(DeviceId::from(format!("node-{}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        // Canary deploy: first 10% = 1 device
        let request = DeploymentRequest {
            model_id: ModelId::from("model"),
            model_version: ModelVersion::from("v2.0.0"),
            fleet_id: FleetId::from("prod"),
            strategy: RolloutStrategy::Canary {
                stages: vec![10, 50, 100],
                wait_seconds: 60,
                health_check: None,
            },
        };
        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 1); // 10% of 10
    }

    #[test]
    fn test_rolling_deployment() {
        let registry = Arc::new(DeviceRegistry::new());
        for i in 0..15 {
            let mut device = Device::new(
                DeviceId::from(format!("dev-{}", i).as_str()),
                format!("Device {}", i),
            );
            device.status = DeviceStatus::Online;
            registry.register(device).unwrap();
        }

        let manager = FleetManager::new(registry);
        let mut fleet = Fleet::new(FleetId::from("fleet"), "Fleet");
        for i in 0..15 {
            fleet.add_device(DeviceId::from(format!("dev-{}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        let request = DeploymentRequest {
            model_id: ModelId::from("model"),
            model_version: ModelVersion::from("v1.0.0"),
            fleet_id: FleetId::from("fleet"),
            strategy: RolloutStrategy::Rolling {
                batch_size: 5,
                wait_seconds: 30,
            },
        };
        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 5); // First batch of 5
        assert_eq!(result.successful, 5);
    }
}

mod control_plane_integration {
    use axum::body::Body;
    use http_body_util::BodyExt;
    use oxide_control::fleet_manager::FleetManager;
    use oxide_control::campaign::CampaignStore;
    use oxide_control::model_store::ControlPlaneModelStore;
    use oxide_control::registry::DeviceRegistry;
    use oxide_control::server::{ControlPlaneServer, ControlPlaneState};
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    fn make_app() -> axum::Router {
        let dir = tempfile::TempDir::new().unwrap();
        let registry = Arc::new(DeviceRegistry::new());
        let fleet_manager = Arc::new(FleetManager::new(registry.clone()));
        let model_store = Arc::new(RwLock::new(
            ControlPlaneModelStore::open(&dir.path().join("models")).unwrap(),
        ));
        let state = Arc::new(ControlPlaneState {
            registry,
            fleet_manager,
            model_store,
            campaigns: Arc::new(RwLock::new(CampaignStore::new())),
        });
        ControlPlaneServer::router(state)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = make_app();
        let request = axum::http::Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_register_and_list_devices() {
        let app = make_app();

        // Register device
        let register_req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/devices")
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"id": "pi-01", "name": "Test Pi"}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(register_req).await.unwrap();
        assert_eq!(resp.status(), 201);

        // List devices
        let list_req = axum::http::Request::builder()
            .uri("/api/v1/devices")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(list_req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let devices: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0]["id"], "pi-01");
    }

    #[tokio::test]
    async fn test_fleet_workflow() {
        let app = make_app();

        // Register devices
        for i in 0..3 {
            let req = axum::http::Request::builder()
                .method("POST")
                .uri("/api/v1/devices")
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"id": "dev-{}", "name": "Device {}"}}"#,
                    i, i
                )))
                .unwrap();
            app.clone().oneshot(req).await.unwrap();
        }

        // Create fleet
        let fleet_req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/v1/fleets")
            .header("Content-Type", "application/json")
            .body(Body::from(
                r#"{"id": "test-fleet", "name": "Test Fleet"}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(fleet_req).await.unwrap();
        assert_eq!(resp.status(), 201);

        // Add devices to fleet
        for i in 0..3 {
            let req = axum::http::Request::builder()
                .method("POST")
                .uri(format!("/api/v1/fleets/test-fleet/devices/dev-{}", i))
                .body(Body::empty())
                .unwrap();
            let resp = app.clone().oneshot(req).await.unwrap();
            assert_eq!(resp.status(), 200);
        }

        // Check fleet status
        let status_req = axum::http::Request::builder()
            .uri("/api/v1/fleets/test-fleet/status")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(status_req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = resp.into_body().collect().await.unwrap().to_bytes();
        let status: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(status["total_devices"], 3);
    }
}
