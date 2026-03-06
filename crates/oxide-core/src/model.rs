use serde::{Deserialize, Serialize};
use std::fmt;

/// Unique identifier for a model.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ModelId {
    fn from(s: &str) -> Self {
        ModelId(s.to_string())
    }
}

/// Model version string (e.g., "v2.3.1").
#[derive(Debug, Clone, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ModelVersion(pub String);

impl fmt::Display for ModelVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for ModelVersion {
    fn from(s: &str) -> Self {
        ModelVersion(s.to_string())
    }
}

/// Supported model formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelFormat {
    /// ONNX format (primary, broadest compatibility).
    Onnx,
    /// TensorFlow Lite format.
    TfLite,
    /// Custom/unknown format.
    Unknown,
}

impl ModelFormat {
    /// Detect model format from file extension.
    pub fn from_extension(path: &str) -> Self {
        let lower = path.to_lowercase();
        if lower.ends_with(".onnx") {
            ModelFormat::Onnx
        } else if lower.ends_with(".tflite") {
            ModelFormat::TfLite
        } else {
            ModelFormat::Unknown
        }
    }
}

impl fmt::Display for ModelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelFormat::Onnx => write!(f, "ONNX"),
            ModelFormat::TfLite => write!(f, "TFLite"),
            ModelFormat::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Quantization type for models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuantizationType {
    /// No quantization (fp32).
    None,
    /// Float16 quantization.
    Fp16,
    /// Int8 quantization.
    Int8,
}

impl fmt::Display for QuantizationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantizationType::None => write!(f, "none (fp32)"),
            QuantizationType::Fp16 => write!(f, "fp16"),
            QuantizationType::Int8 => write!(f, "int8"),
        }
    }
}

/// Metadata about a loaded model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Unique identifier.
    pub id: ModelId,
    /// Version of this model.
    pub version: ModelVersion,
    /// File format.
    pub format: ModelFormat,
    /// Size on disk in bytes.
    pub size_bytes: u64,
    /// Input tensor names and shapes.
    pub inputs: Vec<TensorInfo>,
    /// Output tensor names and shapes.
    pub outputs: Vec<TensorInfo>,
    /// Quantization applied.
    pub quantization: QuantizationType,
    /// When the model was loaded.
    pub loaded_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Optional description.
    pub description: Option<String>,
}

/// Information about a tensor (input or output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensorInfo {
    /// Name of the tensor.
    pub name: String,
    /// Shape dimensions (e.g., [1, 3, 224, 224]).
    pub shape: Vec<i64>,
    /// Element data type.
    pub dtype: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_format_detection() {
        assert_eq!(ModelFormat::from_extension("model.onnx"), ModelFormat::Onnx);
        assert_eq!(
            ModelFormat::from_extension("model.ONNX"),
            ModelFormat::Onnx
        );
        assert_eq!(
            ModelFormat::from_extension("model.tflite"),
            ModelFormat::TfLite
        );
        assert_eq!(
            ModelFormat::from_extension("model.bin"),
            ModelFormat::Unknown
        );
    }

    #[test]
    fn test_model_id_display() {
        let id = ModelId::from("face-detection");
        assert_eq!(id.to_string(), "face-detection");
    }

    #[test]
    fn test_model_version_ordering() {
        let v1 = ModelVersion::from("v1.0.0");
        let v2 = ModelVersion::from("v2.0.0");
        assert!(v1 < v2);
    }

    #[test]
    fn test_quantization_display() {
        assert_eq!(QuantizationType::Int8.to_string(), "int8");
        assert_eq!(QuantizationType::Fp16.to_string(), "fp16");
        assert_eq!(QuantizationType::None.to_string(), "none (fp32)");
    }

    #[test]
    fn test_model_info_serialization() {
        let info = ModelInfo {
            id: ModelId::from("test-model"),
            version: ModelVersion::from("v1.0.0"),
            format: ModelFormat::Onnx,
            size_bytes: 1024,
            inputs: vec![TensorInfo {
                name: "input".into(),
                shape: vec![1, 3, 224, 224],
                dtype: "float32".into(),
            }],
            outputs: vec![TensorInfo {
                name: "output".into(),
                shape: vec![1, 1000],
                dtype: "float32".into(),
            }],
            quantization: QuantizationType::None,
            loaded_at: None,
            description: Some("Test model".into()),
        };
        let json = serde_json::to_string(&info).unwrap();
        let deserialized: ModelInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id.0, "test-model");
        assert_eq!(deserialized.format, ModelFormat::Onnx);
    }
}
