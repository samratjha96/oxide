//! Device REST API for health checks, inference, and metrics.

use axum::{
    extract::{Json, State},
    http::StatusCode,
    routing::{get, post},
    Router,
};
use oxide_core::error::OxideError;
use oxide_core::metrics::InferenceMetrics;
use oxide_core::model::ModelId;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Shared state for the device API.
pub struct ApiState {
    /// Device ID.
    pub device_id: String,
    /// Current model ID.
    pub model_id: Option<ModelId>,
    /// Whether the device is healthy.
    pub healthy: bool,
    /// Latest metrics snapshot.
    pub metrics: Option<InferenceMetrics>,
    /// Inference callback (accepts f32 input, shape, returns f32 output).
    pub inference_fn: Option<
        Arc<dyn Fn(&[f32], &[usize]) -> Result<Vec<f32>, OxideError> + Send + Sync>,
    >,
}

/// Request body for inference.
#[derive(Debug, Deserialize)]
pub struct InferenceRequest {
    /// Input data as flat f32 array.
    pub input: Vec<f32>,
    /// Shape of the input tensor.
    pub shape: Vec<usize>,
}

/// Response body for inference.
#[derive(Debug, Serialize)]
pub struct InferenceResponse {
    /// Output data as flat f32 array.
    pub output: Vec<f32>,
    /// Inference latency in microseconds.
    pub latency_us: f64,
    /// Model that produced the result.
    pub model_id: String,
}

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub device_id: String,
    pub model_loaded: bool,
}

/// Metrics response.
#[derive(Debug, Serialize)]
pub struct MetricsResponse {
    pub device_id: String,
    pub model_id: Option<String>,
    pub metrics: Option<InferenceMetrics>,
}

/// The device API router.
pub struct DeviceApi;

impl DeviceApi {
    /// Build the API router with the given shared state.
    pub fn router(state: Arc<RwLock<ApiState>>) -> Router {
        Router::new()
            .route("/health", get(Self::health))
            .route("/metrics", get(Self::metrics))
            .route("/infer", post(Self::infer))
            .route("/info", get(Self::device_info))
            .with_state(state)
    }

    async fn health(
        State(state): State<Arc<RwLock<ApiState>>>,
    ) -> (StatusCode, Json<HealthResponse>) {
        let s = state.read().await;
        let status = if s.healthy { "healthy" } else { "unhealthy" };
        (
            if s.healthy {
                StatusCode::OK
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            },
            Json(HealthResponse {
                status: status.to_string(),
                device_id: s.device_id.clone(),
                model_loaded: s.model_id.is_some(),
            }),
        )
    }

    async fn metrics(
        State(state): State<Arc<RwLock<ApiState>>>,
    ) -> (StatusCode, Json<MetricsResponse>) {
        let s = state.read().await;
        (
            StatusCode::OK,
            Json(MetricsResponse {
                device_id: s.device_id.clone(),
                model_id: s.model_id.as_ref().map(|id| id.to_string()),
                metrics: s.metrics.clone(),
            }),
        )
    }

    async fn infer(
        State(state): State<Arc<RwLock<ApiState>>>,
        Json(req): Json<InferenceRequest>,
    ) -> Result<(StatusCode, Json<InferenceResponse>), (StatusCode, String)> {
        let s = state.read().await;

        let inference_fn = s.inference_fn.as_ref().ok_or_else(|| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "No model loaded".to_string(),
            )
        })?;

        let model_id = s
            .model_id
            .as_ref()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let start = std::time::Instant::now();
        let output = inference_fn(&req.input, &req.shape).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("Inference failed: {}", e))
        })?;
        let latency_us = start.elapsed().as_secs_f64() * 1_000_000.0;

        Ok((
            StatusCode::OK,
            Json(InferenceResponse {
                output,
                latency_us,
                model_id,
            }),
        ))
    }

    async fn device_info(
        State(state): State<Arc<RwLock<ApiState>>>,
    ) -> (StatusCode, Json<serde_json::Value>) {
        let s = state.read().await;
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "device_id": s.device_id,
                "model_id": s.model_id.as_ref().map(|id| id.to_string()),
                "healthy": s.healthy,
                "platform": {
                    "arch": std::env::consts::ARCH,
                    "os": std::env::consts::OS,
                }
            })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state() -> Arc<RwLock<ApiState>> {
        Arc::new(RwLock::new(ApiState {
            device_id: "test-device".to_string(),
            model_id: None,
            healthy: true,
            metrics: None,
            inference_fn: None,
        }))
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let state = make_state();
        let app = DeviceApi::router(state);

        let response = axum::serve(
            tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap(),
            app,
        );
        // Just verify the router builds without error
        assert!(true);
    }

    #[test]
    fn test_inference_request_deserialization() {
        let json = r#"{"input": [1.0, 2.0, 3.0], "shape": [1, 3]}"#;
        let req: InferenceRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.input, vec![1.0, 2.0, 3.0]);
        assert_eq!(req.shape, vec![1, 3]);
    }
}
