//! Control plane HTTP server for fleet management.

use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use oxide_core::device::{Device, DeviceId};
use oxide_core::fleet::{Fleet, FleetId, RolloutStrategy};
use oxide_core::model::{ModelId, ModelVersion};
use serde::Deserialize;
use std::sync::Arc;

use crate::fleet_manager::{DeploymentRequest, FleetManager};
use crate::registry::DeviceRegistry;

/// Shared control plane state.
pub struct ControlPlaneState {
    pub registry: Arc<DeviceRegistry>,
    pub fleet_manager: Arc<FleetManager>,
}

/// Request to register a device.
#[derive(Debug, Deserialize)]
pub struct RegisterDeviceRequest {
    pub id: String,
    pub name: String,
    pub tags: Option<std::collections::HashMap<String, String>>,
}

/// Request to create a fleet.
#[derive(Debug, Deserialize)]
pub struct CreateFleetRequest {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// Request to deploy a model to a fleet.
#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    pub model_id: String,
    pub model_version: String,
    pub strategy: Option<String>, // "all_at_once", "canary", "rolling"
}

/// Control plane server.
pub struct ControlPlaneServer;

impl ControlPlaneServer {
    /// Build the control plane API router.
    pub fn router(state: Arc<ControlPlaneState>) -> Router {
        Router::new()
            // Device endpoints
            .route("/api/v1/devices", get(Self::list_devices).post(Self::register_device))
            .route("/api/v1/devices/{id}", get(Self::get_device).delete(Self::unregister_device))
            .route("/api/v1/devices/{id}/heartbeat", post(Self::device_heartbeat))
            // Fleet endpoints
            .route("/api/v1/fleets", get(Self::list_fleets).post(Self::create_fleet))
            .route("/api/v1/fleets/{id}", get(Self::get_fleet))
            .route("/api/v1/fleets/{id}/devices/{device_id}", post(Self::add_device_to_fleet))
            .route("/api/v1/fleets/{id}/deploy", post(Self::deploy_to_fleet))
            .route("/api/v1/fleets/{id}/status", get(Self::fleet_status))
            // Health
            .route("/health", get(Self::health))
            .with_state(state)
    }

    async fn health() -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "healthy",
                "service": "oxide-control-plane"
            })),
        )
    }

    async fn list_devices(
        State(state): State<Arc<ControlPlaneState>>,
    ) -> Result<(StatusCode, Json<Vec<Device>>), (StatusCode, String)> {
        state
            .registry
            .list()
            .map(|devices| (StatusCode::OK, Json(devices)))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    async fn register_device(
        State(state): State<Arc<ControlPlaneState>>,
        Json(req): Json<RegisterDeviceRequest>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mut device = Device::new(DeviceId::from(req.id.as_str()), &req.name);
        if let Some(tags) = req.tags {
            device.tags = tags;
        }

        state
            .registry
            .register(device)
            .map(|_| {
                (
                    StatusCode::CREATED,
                    Json(serde_json::json!({"status": "registered", "id": req.id})),
                )
            })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    async fn get_device(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<Device>), (StatusCode, String)> {
        state
            .registry
            .get(&DeviceId::from(id.as_str()))
            .map(|device| (StatusCode::OK, Json(device)))
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
    }

    async fn unregister_device(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        state
            .registry
            .unregister(&DeviceId::from(id.as_str()))
            .map(|_| {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "unregistered"})),
                )
            })
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
    }

    async fn device_heartbeat(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        state
            .registry
            .heartbeat(&DeviceId::from(id.as_str()))
            .map(|_| {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "ok"})),
                )
            })
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
    }

    async fn list_fleets(
        State(state): State<Arc<ControlPlaneState>>,
    ) -> Result<(StatusCode, Json<Vec<Fleet>>), (StatusCode, String)> {
        state
            .fleet_manager
            .list_fleets()
            .map(|fleets| (StatusCode::OK, Json(fleets)))
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    async fn create_fleet(
        State(state): State<Arc<ControlPlaneState>>,
        Json(req): Json<CreateFleetRequest>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mut fleet = Fleet::new(FleetId::from(req.id.as_str()), &req.name);
        fleet.description = req.description;

        state
            .fleet_manager
            .create_fleet(fleet)
            .map(|_| {
                (
                    StatusCode::CREATED,
                    Json(serde_json::json!({"status": "created", "id": req.id})),
                )
            })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    async fn get_fleet(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<Fleet>), (StatusCode, String)> {
        state
            .fleet_manager
            .get_fleet(&FleetId::from(id.as_str()))
            .map(|fleet| (StatusCode::OK, Json(fleet)))
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
    }

    async fn add_device_to_fleet(
        State(state): State<Arc<ControlPlaneState>>,
        Path((fleet_id, device_id)): Path<(String, String)>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        state
            .fleet_manager
            .add_device_to_fleet(
                &FleetId::from(fleet_id.as_str()),
                DeviceId::from(device_id.as_str()),
            )
            .map(|_| {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "added"})),
                )
            })
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
    }

    async fn deploy_to_fleet(
        State(state): State<Arc<ControlPlaneState>>,
        Path(fleet_id): Path<String>,
        Json(req): Json<DeployRequest>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let strategy = match req.strategy.as_deref() {
            Some("canary") => RolloutStrategy::Canary {
                stages: vec![5, 25, 50, 100],
                wait_seconds: 300,
                health_check: None,
            },
            Some("rolling") => RolloutStrategy::Rolling {
                batch_size: 5,
                wait_seconds: 60,
            },
            _ => RolloutStrategy::AllAtOnce,
        };

        let deploy_req = DeploymentRequest {
            model_id: ModelId::from(req.model_id.as_str()),
            model_version: ModelVersion::from(req.model_version.as_str()),
            fleet_id: FleetId::from(fleet_id.as_str()),
            strategy,
        };

        state
            .fleet_manager
            .deploy(&deploy_req)
            .map(|result| {
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "deployed",
                        "total_devices": result.total_devices,
                        "successful": result.successful,
                        "failed": result.failed,
                    })),
                )
            })
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
    }

    async fn fleet_status(
        State(state): State<Arc<ControlPlaneState>>,
        Path(fleet_id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        state
            .fleet_manager
            .fleet_status(&FleetId::from(fleet_id.as_str()))
            .map(|status| (StatusCode::OK, Json(serde_json::to_value(status).unwrap())))
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_request_deserialization() {
        let json = r#"{"id": "pi-01", "name": "Test Pi"}"#;
        let req: RegisterDeviceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.id, "pi-01");
        assert_eq!(req.name, "Test Pi");
    }

    #[test]
    fn test_deploy_request_deserialization() {
        let json =
            r#"{"model_id": "face-detect", "model_version": "v2.0.0", "strategy": "canary"}"#;
        let req: DeployRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model_id, "face-detect");
        assert_eq!(req.strategy, Some("canary".to_string()));
    }
}
