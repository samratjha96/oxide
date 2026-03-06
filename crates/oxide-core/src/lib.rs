//! # Oxide Core
//!
//! Core traits, types, and configuration for the Oxide edge AI runtime.
//! This crate defines the foundational abstractions used across all Oxide components.

#![deny(unsafe_code)]

pub mod device;
pub mod error;
pub mod fleet;
pub mod metrics;
pub mod model;

pub use device::{BasicMetrics, Device, DeviceId, DeviceStatus, HeartbeatRequest, HeartbeatResponse, UpdateResult};
pub use error::{OxideError, Result};
pub use fleet::{Fleet, FleetId, RolloutStrategy};
pub use metrics::InferenceMetrics;
pub use model::{ModelFormat, ModelId, ModelInfo, ModelVersion, QuantizationType};
