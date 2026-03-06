//! # Oxide Core
//!
//! Core traits, types, and configuration for the Oxide edge AI runtime.
//! This crate defines the foundational abstractions used across all Oxide components.

pub mod config;
pub mod device;
pub mod error;
pub mod fleet;
pub mod metrics;
pub mod model;
pub mod telemetry;

pub use config::OxideConfig;
pub use device::{Device, DeviceId, DeviceStatus};
pub use error::{OxideError, Result};
pub use fleet::{Fleet, FleetId, RolloutStrategy};
pub use metrics::InferenceMetrics;
pub use model::{ModelFormat, ModelId, ModelInfo, ModelVersion, QuantizationType};
