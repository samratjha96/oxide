//! Control plane HTTP server for fleet management and model distribution.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Json, Path, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use oxide_core::device::{Device, DeviceId, HeartbeatRequest, HeartbeatResponse};
use oxide_core::fleet::{Fleet, FleetId, RolloutStrategy};
use oxide_core::model::{ModelId, ModelVersion};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

use crate::campaign::{Campaign, CampaignId, CampaignStore};
use crate::fleet_manager::{DeploymentRequest, FleetManager};
use crate::model_store::ControlPlaneModelStore;
use crate::registry::DeviceRegistry;

/// Shared control plane state.
pub struct ControlPlaneState {
    pub registry: Arc<DeviceRegistry>,
    pub fleet_manager: Arc<FleetManager>,
    pub model_store: Arc<RwLock<ControlPlaneModelStore>>,
    pub campaigns: Arc<RwLock<CampaignStore>>,
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
    pub fleet_id: Option<String>,
    pub strategy: Option<String>, // "all_at_once", "canary", "rolling"
}

/// Maximum upload size for model files (512 MB).
const MAX_MODEL_UPLOAD_BYTES: usize = 512 * 1024 * 1024;

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
            // Model endpoints (with explicit body limit for upload)
            .route(
                "/api/v1/models/{model_id}/versions/{version}",
                post(Self::upload_model).layer(DefaultBodyLimit::max(MAX_MODEL_UPLOAD_BYTES)),
            )
            .route("/api/v1/models/{model_id}/versions/{version}/download", get(Self::download_model))
            .route("/api/v1/models/{model_id}/versions/{version}/meta", get(Self::model_meta))
            .route("/api/v1/models/{model_id}", get(Self::list_model_versions))
            // Campaign endpoints
            .route("/api/v1/campaigns", post(Self::create_campaign).get(Self::list_campaigns))
            .route("/api/v1/campaigns/{id}", get(Self::get_campaign))
            .route("/api/v1/campaigns/{id}/pause", post(Self::pause_campaign))
            .route("/api/v1/campaigns/{id}/resume", post(Self::resume_campaign))
            .route("/api/v1/campaigns/{id}/abort", post(Self::abort_campaign))
            // Health
            .route("/health", get(Self::health))
            .with_state(state)
    }

    // ─── Health ───

    async fn health() -> (StatusCode, Json<serde_json::Value>) {
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "healthy",
                "service": "oxide-control-plane"
            })),
        )
    }

    // ─── Devices ───

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

    /// Extended heartbeat: accepts device state, returns model assignment.
    async fn device_heartbeat(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
        body: Option<Json<HeartbeatRequest>>,
    ) -> Result<(StatusCode, Json<HeartbeatResponse>), (StatusCode, String)> {
        let device_id = DeviceId::from(id.as_str());

        // Update heartbeat timestamp + status
        state
            .registry
            .heartbeat(&device_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        // If the agent sent a body, update device record with current model info
        if let Some(Json(req)) = body {
            state
                .registry
                .update_current_model(
                    &device_id,
                    req.current_model,
                    req.current_model_version,
                    req.last_update_result,
                )
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        }

        // Read the device to get its assignment
        let device = state
            .registry
            .get(&device_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        Ok((
            StatusCode::OK,
            Json(HeartbeatResponse {
                status: "ok".to_string(),
                assigned_model: device.assigned_model,
                assigned_model_version: device.assigned_model_version,
            }),
        ))
    }

    // ─── Fleets ───

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

    /// Deploy to fleet: sets assigned_model on all fleet devices.
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

        let model_id = ModelId::from(req.model_id.as_str());
        let model_version = ModelVersion::from(req.model_version.as_str());
        let fleet_id_val = FleetId::from(fleet_id.as_str());

        // Get the fleet to find devices
        let fleet = state
            .fleet_manager
            .get_fleet(&fleet_id_val)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        // Set assigned_model on each device in the fleet
        for device_id in &fleet.devices {
            if let Err(e) = state.registry.set_assignment(
                device_id,
                Some(model_id.clone()),
                Some(model_version.clone()),
            ) {
                info!("Failed to set assignment for {}: {}", device_id, e);
            }
        }

        let deploy_req = DeploymentRequest {
            model_id,
            model_version,
            fleet_id: fleet_id_val,
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

    // ─── Campaigns ───

    /// Create a new campaign (replaces fire-and-forget deploy).
    async fn create_campaign(
        State(state): State<Arc<ControlPlaneState>>,
        Json(req): Json<DeployRequest>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let model_id = ModelId::from(req.model_id.as_str());
        let model_version = ModelVersion::from(req.model_version.as_str());
        let fleet_id = FleetId::from(
            req.fleet_id
                .as_deref()
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "fleet_id required".to_string()))?,
        );

        // Get fleet devices
        let fleet = state
            .fleet_manager
            .get_fleet(&fleet_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        // Set assignments on all fleet devices
        for device_id in &fleet.devices {
            let _ = state.registry.set_assignment(
                device_id,
                Some(model_id.clone()),
                Some(model_version.clone()),
            );
        }

        // Create campaign
        let campaign_id = CampaignId(format!(
            "{}-{}-{}",
            model_id.0,
            model_version.0,
            chrono::Utc::now().timestamp()
        ));
        let campaign = Campaign::new(
            campaign_id.clone(),
            model_id,
            model_version,
            fleet_id,
            fleet.devices.clone(),
        );

        let summary = campaign.summary();
        let mut campaigns = state.campaigns.write().await;
        campaigns.create(campaign);

        Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({
                "campaign_id": campaign_id.0,
                "state": "rolling_out",
                "total_devices": summary.total,
            })),
        ))
    }

    async fn list_campaigns(
        State(state): State<Arc<ControlPlaneState>>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let campaigns = state.campaigns.read().await;
        let list: Vec<serde_json::Value> = campaigns
            .list()
            .iter()
            .map(|c| {
                let s = c.summary();
                serde_json::json!({
                    "id": c.id.0,
                    "model_id": c.model_id.0,
                    "target_version": c.target_version.0,
                    "fleet_id": c.fleet_id.0,
                    "state": format!("{:?}", c.state),
                    "total": s.total,
                    "complete": s.complete,
                    "failed": s.failed,
                })
            })
            .collect();
        Ok((StatusCode::OK, Json(serde_json::json!({ "campaigns": list }))))
    }

    async fn get_campaign(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let campaigns = state.campaigns.read().await;
        let campaign = campaigns
            .get(&CampaignId(id.clone()))
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("campaign {id} not found")))?;

        let summary = campaign.summary();
        let devices: Vec<serde_json::Value> = campaign
            .devices
            .iter()
            .map(|(did, state)| {
                serde_json::json!({
                    "device_id": did.0,
                    "state": format!("{state:?}"),
                })
            })
            .collect();

        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "id": campaign.id.0,
                "model_id": campaign.model_id.0,
                "target_version": campaign.target_version.0,
                "state": format!("{:?}", campaign.state),
                "created_at": campaign.created_at,
                "summary": summary,
                "devices": devices,
                "bandwidth": {
                    "bytes_served": campaign.total_bytes_served,
                    "bytes_saved_by_delta": campaign.total_bytes_saved_by_delta,
                },
            })),
        ))
    }

    async fn pause_campaign(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mut campaigns = state.campaigns.write().await;
        let campaign = campaigns
            .get_mut(&CampaignId(id.clone()))
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("campaign {id} not found")))?;
        campaign.pause();
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "state": format!("{:?}", campaign.state) })),
        ))
    }

    async fn resume_campaign(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mut campaigns = state.campaigns.write().await;
        let campaign = campaigns
            .get_mut(&CampaignId(id.clone()))
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("campaign {id} not found")))?;
        campaign.resume();
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "state": format!("{:?}", campaign.state) })),
        ))
    }

    async fn abort_campaign(
        State(state): State<Arc<ControlPlaneState>>,
        Path(id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mut campaigns = state.campaigns.write().await;
        let campaign = campaigns
            .get_mut(&CampaignId(id.clone()))
            .ok_or_else(|| (StatusCode::NOT_FOUND, format!("campaign {id} not found")))?;
        campaign.abort();
        Ok((
            StatusCode::OK,
            Json(serde_json::json!({ "state": format!("{:?}", campaign.state) })),
        ))
    }

    // ─── Models ───

    /// Upload model bytes.
    async fn upload_model(
        State(state): State<Arc<ControlPlaneState>>,
        Path((model_id, version)): Path<(String, String)>,
        body: Bytes,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mid = ModelId::from(model_id.as_str());
        let ver = ModelVersion::from(version.as_str());

        let mut store = state.model_store.write().await;
        let entry = store
            .store(&mid, &ver, &body)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

        Ok((
            StatusCode::CREATED,
            Json(serde_json::json!({
                "model_id": model_id,
                "version": version,
                "sha256": entry.sha256,
                "size_bytes": entry.size_bytes,
            })),
        ))
    }

    /// Download model bytes.
    ///
    /// If the agent sends `X-Oxide-Base-Version` and a cached delta exists,
    /// serves the delta patch instead of the full file.
    async fn download_model(
        State(state): State<Arc<ControlPlaneState>>,
        Path((model_id, version)): Path<(String, String)>,
        request_headers: axum::http::HeaderMap,
    ) -> Result<(StatusCode, axum::http::HeaderMap, Bytes), (StatusCode, String)> {
        let mid = ModelId::from(model_id.as_str());
        let ver = ModelVersion::from(version.as_str());

        let store = state.model_store.read().await;

        // Check if agent sent base version for delta download
        let base_version = request_headers
            .get("x-oxide-base-version")
            .and_then(|v| v.to_str().ok())
            .map(ModelVersion::from);

        // Try to serve a delta if we have one
        if let Some(base_ver) = &base_version {
            if let Ok(Some((delta_bytes, cached))) = store.get_delta(&mid, base_ver, &ver) {
                info!(
                    "Serving delta for {} {} → {} ({} bytes, {:.1}% savings)",
                    model_id, base_ver, ver, delta_bytes.len(), cached.savings_pct
                );

                let mut headers = axum::http::HeaderMap::new();
                headers.insert("content-type", "application/x-oxide-delta".parse().unwrap());
                headers.insert(
                    "x-oxide-delta-strategy",
                    cached.strategy.to_lowercase().parse().unwrap(),
                );
                headers.insert(
                    "x-oxide-delta-base",
                    base_ver.0.parse().unwrap(),
                );

                // Also include target SHA for verification
                if let Ok(meta) = store.get_meta(&mid, &ver) {
                    headers.insert("x-oxide-target-sha256", meta.sha256.parse().unwrap());
                    headers.insert(
                        "x-oxide-target-size",
                        meta.size_bytes.to_string().parse().unwrap(),
                    );
                }

                return Ok((StatusCode::OK, headers, Bytes::from(delta_bytes)));
            }
        }

        // No delta available — serve full file
        let meta = store
            .get_meta(&mid, &ver)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
        let sha256 = meta.sha256.clone();

        let data = store
            .get_bytes(&mid, &ver)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        let mut headers = axum::http::HeaderMap::new();
        headers.insert("content-type", "application/octet-stream".parse().unwrap());
        headers.insert("x-oxide-sha256", sha256.parse().unwrap());

        Ok((StatusCode::OK, headers, Bytes::from(data)))
    }

    /// Get model metadata.
    async fn model_meta(
        State(state): State<Arc<ControlPlaneState>>,
        Path((model_id, version)): Path<(String, String)>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mid = ModelId::from(model_id.as_str());
        let ver = ModelVersion::from(version.as_str());

        let store = state.model_store.read().await;
        let meta = store
            .get_meta(&mid, &ver)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        Ok((
            StatusCode::OK,
            Json(serde_json::json!({
                "model_id": meta.model_id.0,
                "version": meta.version.0,
                "sha256": meta.sha256,
                "size_bytes": meta.size_bytes,
                "uploaded_at": meta.uploaded_at.to_rfc3339(),
            })),
        ))
    }

    /// List versions of a model.
    async fn list_model_versions(
        State(state): State<Arc<ControlPlaneState>>,
        Path(model_id): Path<String>,
    ) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, String)> {
        let mid = ModelId::from(model_id.as_str());

        let store = state.model_store.read().await;
        let versions = store
            .list_versions(&mid)
            .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;

        let entries: Vec<serde_json::Value> = versions
            .iter()
            .map(|v| {
                serde_json::json!({
                    "version": v.version.0,
                    "sha256": v.sha256,
                    "size_bytes": v.size_bytes,
                    "uploaded_at": v.uploaded_at.to_rfc3339(),
                })
            })
            .collect();

        Ok((StatusCode::OK, Json(serde_json::json!({ "model_id": model_id, "versions": entries }))))
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

    #[test]
    fn test_heartbeat_request_deserialization() {
        let json = r#"{"current_model": "test", "current_model_version": "v1", "status": "online"}"#;
        let req: HeartbeatRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.current_model.unwrap().0, "test");
    }

    #[test]
    fn test_heartbeat_request_empty() {
        let json = r#"{}"#;
        let req: HeartbeatRequest = serde_json::from_str(json).unwrap();
        assert!(req.current_model.is_none());
    }
}
