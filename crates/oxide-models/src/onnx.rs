//! ONNX model loading and inference using the `tract` crate.
//!
//! tract is a pure-Rust inference engine — no C dependencies, small binary,
//! excellent ARM support, and first-class ONNX compatibility.

use oxide_core::error::{OxideError, Result};
use oxide_core::model::{ModelFormat, ModelId, ModelInfo, ModelVersion, QuantizationType, TensorInfo};
use std::path::Path;
use tract_onnx::prelude::*;
use tracing::{debug, info};

/// Type alias for the optimized tract inference plan.
type TractPlan = SimplePlan<TypedFact, Box<dyn TypedOp>, Graph<TypedFact, Box<dyn TypedOp>>>;

/// An ONNX model loaded via tract, ready for inference.
pub struct OnnxModel {
    /// The optimized tract model, ready to run.
    model: TractPlan,
    /// Model metadata.
    info: ModelInfo,
}

impl OnnxModel {
    /// Load an ONNX model from a file path.
    pub fn load(path: &Path) -> Result<Self> {
        let path_str = path.display().to_string();
        info!("Loading ONNX model from: {}", path_str);

        // Verify file exists and get size
        let metadata = std::fs::metadata(path).map_err(|e| {
            OxideError::ModelNotFound(format!("{}: {}", path_str, e))
        })?;
        let size_bytes = metadata.len();

        // Load and optimize the model with tract
        let start = std::time::Instant::now();

        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|e| OxideError::Model(format!("Failed to parse ONNX model: {}", e)))?
            .into_optimized()
            .map_err(|e| OxideError::Model(format!("Failed to optimize model: {}", e)))?
            .into_runnable()
            .map_err(|e| OxideError::Model(format!("Failed to create runnable plan: {}", e)))?;

        let load_time = start.elapsed();
        info!("Model loaded in {:.2?}", load_time);

        // Extract input/output information from the model
        let inputs = Self::extract_inputs(&model);
        let outputs = Self::extract_outputs(&model);

        let model_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let info = ModelInfo {
            id: ModelId(model_name),
            version: ModelVersion("v1.0.0".to_string()),
            format: ModelFormat::Onnx,
            size_bytes,
            inputs,
            outputs,
            quantization: QuantizationType::None,
            loaded_at: Some(chrono::Utc::now()),
            description: None,
        };

        debug!("Model info: {:?}", info);

        Ok(OnnxModel { model, info })
    }

    /// Load an ONNX model from raw bytes.
    pub fn load_from_bytes(bytes: &[u8], name: &str) -> Result<Self> {
        info!("Loading ONNX model from bytes ({} bytes)", bytes.len());
        let size_bytes = bytes.len() as u64;

        let cursor = std::io::Cursor::new(bytes);
        let start = std::time::Instant::now();

        let model = tract_onnx::onnx()
            .model_for_read(&mut cursor.clone())
            .map_err(|e| OxideError::Model(format!("Failed to parse ONNX model: {}", e)))?
            .into_optimized()
            .map_err(|e| OxideError::Model(format!("Failed to optimize model: {}", e)))?
            .into_runnable()
            .map_err(|e| OxideError::Model(format!("Failed to create runnable plan: {}", e)))?;

        let load_time = start.elapsed();
        info!("Model loaded from bytes in {:.2?}", load_time);

        let inputs = Self::extract_inputs(&model);
        let outputs = Self::extract_outputs(&model);

        let info = ModelInfo {
            id: ModelId(name.to_string()),
            version: ModelVersion("v1.0.0".to_string()),
            format: ModelFormat::Onnx,
            size_bytes,
            inputs,
            outputs,
            quantization: QuantizationType::None,
            loaded_at: Some(chrono::Utc::now()),
            description: None,
        };

        Ok(OnnxModel { model, info })
    }

    /// Run inference on the model with the given input tensors.
    pub fn run(&self, inputs: TVec<TValue>) -> Result<TVec<TValue>> {
        let results = self.model.run(inputs).map_err(|e| {
            OxideError::Inference(format!("Inference failed: {}", e))
        })?;
        Ok(results)
    }

    /// Run inference with a single f32 input tensor.
    ///
    /// This is a convenience method for models with a single input.
    pub fn run_f32(&self, input: &[f32], shape: &[usize]) -> Result<Vec<f32>> {
        let tensor = tract_ndarray::Array::from_shape_vec(
            tract_ndarray::IxDyn(shape),
            input.to_vec(),
        )
        .map_err(|e| OxideError::Inference(format!("Invalid input shape: {}", e)))?;

        let input_tv: TValue = tensor.into_tvalue();
        let results = self.run(tvec![input_tv])?;

        // Extract the first output as f32 vec
        let output = results
            .first()
            .ok_or_else(|| OxideError::Inference("No output produced".to_string()))?;

        let output_slice = output.as_slice::<f32>().map_err(|e| {
            OxideError::Inference(format!("Failed to read output as f32: {}", e))
        })?;

        Ok(output_slice.to_vec())
    }

    /// Get model information / metadata.
    pub const fn info(&self) -> &ModelInfo {
        &self.info
    }

    /// Get the expected input shapes.
    pub fn input_shapes(&self) -> Vec<Vec<usize>> {
        if let Ok(outlets) = self.model.model().input_outlets() {
            outlets
                .iter()
                .filter_map(|outlet| {
                    let fact = self.model.model().outlet_fact(*outlet).ok()?;
                    let shape = fact.shape.as_concrete()?.to_vec();
                    Some(shape)
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    fn extract_inputs(
        model: &TractPlan,
    ) -> Vec<TensorInfo> {
        let outlets = match model.model().input_outlets() {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        outlets
            .iter()
            .enumerate()
            .filter_map(|(i, outlet)| {
                let fact = model.model().outlet_fact(*outlet).ok()?;
                let name = model.model().node(outlet.node).name.clone();
                let shape: Vec<i64> = fact
                    .shape
                    .iter()
                    .map(|d| d.to_i64().unwrap_or(-1))
                    .collect();
                let dtype = format!("{:?}", fact.datum_type);
                Some(TensorInfo {
                    name: if name.is_empty() {
                        format!("input_{}", i)
                    } else {
                        name
                    },
                    shape,
                    dtype,
                })
            })
            .collect()
    }

    fn extract_outputs(
        model: &TractPlan,
    ) -> Vec<TensorInfo> {
        let outlets = match model.model().output_outlets() {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };
        outlets
            .iter()
            .enumerate()
            .filter_map(|(i, outlet)| {
                let fact = model.model().outlet_fact(*outlet).ok()?;
                let name = model.model().node(outlet.node).name.clone();
                let shape: Vec<i64> = fact
                    .shape
                    .iter()
                    .map(|d| d.to_i64().unwrap_or(-1))
                    .collect();
                let dtype = format!("{:?}", fact.datum_type);
                Some(TensorInfo {
                    name: if name.is_empty() {
                        format!("output_{}", i)
                    } else {
                        name
                    },
                    shape,
                    dtype,
                })
            })
            .collect()
    }
}

impl std::fmt::Debug for OnnxModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnnxModel")
            .field("info", &self.info)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_nonexistent_model() {
        let result = OnnxModel::load(Path::new("/nonexistent/model.onnx"));
        assert!(result.is_err());
    }
}
