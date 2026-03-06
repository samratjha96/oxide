//! Inference engine: loads models, runs inference, tracks metrics.

use oxide_core::error::{OxideError, Result};
use oxide_core::metrics::{InferenceMetrics, LatencyTracker};
use oxide_core::model::{ModelId, ModelInfo};
use oxide_models::OnnxModel;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{debug, error, info};

/// The core inference engine that manages loaded models and runs inference.
pub struct InferenceEngine {
    /// Currently loaded models.
    models: Arc<RwLock<HashMap<ModelId, LoadedModel>>>,
    /// Configuration.
    num_threads: usize,
}

/// A model loaded and ready for inference, with associated metrics.
struct LoadedModel {
    onnx: OnnxModel,
    latency_tracker: LatencyTracker,
    total_inferences: u64,
    failed_inferences: u64,
    loaded_at: Instant,
}

/// Result of a single inference run.
#[derive(Debug)]
pub struct InferenceResult {
    /// Output tensor values.
    pub outputs: Vec<f32>,
    /// Inference latency in microseconds.
    pub latency_us: f64,
    /// Model that produced this result.
    pub model_id: ModelId,
}

impl InferenceEngine {
    /// Create a new inference engine.
    pub fn new(num_threads: usize) -> Self {
        let threads = if num_threads == 0 {
            std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(1)
        } else {
            num_threads
        };
        info!("Creating inference engine with {} threads", threads);
        InferenceEngine {
            models: Arc::new(RwLock::new(HashMap::new())),
            num_threads: threads,
        }
    }

    /// Load a model from a file path.
    pub fn load_model(&self, path: &Path) -> Result<ModelInfo> {
        let model = OnnxModel::load(path)?;
        let info = model.info().clone();
        let model_id = info.id.clone();

        info!("Loaded model '{}' ({} bytes)", model_id, info.size_bytes);

        let loaded = LoadedModel {
            onnx: model,
            latency_tracker: LatencyTracker::new(10_000),
            total_inferences: 0,
            failed_inferences: 0,
            loaded_at: Instant::now(),
        };

        let mut models = self.models.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        models.insert(model_id.clone(), loaded);

        Ok(info)
    }

    /// Load a model from raw bytes.
    pub fn load_model_from_bytes(&self, bytes: &[u8], name: &str) -> Result<ModelInfo> {
        let model = OnnxModel::load_from_bytes(bytes, name)?;
        let info = model.info().clone();
        let model_id = info.id.clone();

        info!("Loaded model '{}' from bytes ({} bytes)", model_id, info.size_bytes);

        let loaded = LoadedModel {
            onnx: model,
            latency_tracker: LatencyTracker::new(10_000),
            total_inferences: 0,
            failed_inferences: 0,
            loaded_at: Instant::now(),
        };

        let mut models = self.models.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        models.insert(model_id.clone(), loaded);

        Ok(info)
    }

    /// Unload a model by ID.
    pub fn unload_model(&self, model_id: &ModelId) -> Result<()> {
        let mut models = self.models.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        if models.remove(model_id).is_some() {
            info!("Unloaded model '{}'", model_id);
            Ok(())
        } else {
            Err(OxideError::ModelNotFound(model_id.to_string()))
        }
    }

    /// Run inference on a loaded model with f32 input data.
    pub fn infer(
        &self,
        model_id: &ModelId,
        input: &[f32],
        shape: &[usize],
    ) -> Result<InferenceResult> {
        let mut models = self.models.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let loaded = models.get_mut(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        let start = Instant::now();
        let result = loaded.onnx.run_f32(input, shape);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;

        loaded.latency_tracker.record(elapsed);
        loaded.total_inferences += 1;

        match result {
            Ok(outputs) => {
                debug!(
                    "Inference on '{}': {:.2}us, {} outputs",
                    model_id,
                    latency_us,
                    outputs.len()
                );
                Ok(InferenceResult {
                    outputs,
                    latency_us,
                    model_id: model_id.clone(),
                })
            }
            Err(e) => {
                loaded.failed_inferences += 1;
                error!("Inference failed on '{}': {}", model_id, e);
                Err(e)
            }
        }
    }

    /// Get metrics for a loaded model.
    pub fn get_metrics(&self, model_id: &ModelId) -> Result<InferenceMetrics> {
        let models = self.models.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let loaded = models.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        let mut metrics = loaded.latency_tracker.to_metrics(
            loaded.total_inferences,
            loaded.failed_inferences,
            0, // Memory tracking would need a separate mechanism
        );
        metrics.uptime_seconds = loaded.loaded_at.elapsed().as_secs();

        Ok(metrics)
    }

    /// Get information about a loaded model.
    pub fn get_model_info(&self, model_id: &ModelId) -> Result<ModelInfo> {
        let models = self.models.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let loaded = models.get(model_id).ok_or_else(|| {
            OxideError::ModelNotFound(model_id.to_string())
        })?;

        Ok(loaded.onnx.info().clone())
    }

    /// List all loaded model IDs.
    pub fn list_models(&self) -> Result<Vec<ModelId>> {
        let models = self.models.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        Ok(models.keys().cloned().collect())
    }

    /// Check if a model is loaded.
    pub fn is_loaded(&self, model_id: &ModelId) -> bool {
        self.models
            .read()
            .map(|m| m.contains_key(model_id))
            .unwrap_or(false)
    }

    /// Get the number of configured threads.
    pub fn num_threads(&self) -> usize {
        self.num_threads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = InferenceEngine::new(4);
        assert_eq!(engine.num_threads(), 4);
    }

    #[test]
    fn test_engine_auto_threads() {
        let engine = InferenceEngine::new(0);
        assert!(engine.num_threads() > 0);
    }

    #[test]
    fn test_load_nonexistent_model() {
        let engine = InferenceEngine::new(1);
        let result = engine.load_model(Path::new("/nonexistent/model.onnx"));
        assert!(result.is_err());
    }

    #[test]
    fn test_infer_unloaded_model() {
        let engine = InferenceEngine::new(1);
        let model_id = ModelId::from("not-loaded");
        let result = engine.infer(&model_id, &[1.0], &[1]);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_models_empty() {
        let engine = InferenceEngine::new(1);
        let models = engine.list_models().unwrap();
        assert!(models.is_empty());
    }
}
