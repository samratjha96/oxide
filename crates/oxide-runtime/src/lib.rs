//! # Oxide Runtime
//!
//! The inference engine and model runtime for the Oxide edge AI runtime.
//! Manages model lifecycle, inference execution, metrics collection,
//! model hot-swapping, and health checks.

#![deny(unsafe_code)]

pub mod engine;
pub mod health;
pub mod store;

pub use engine::InferenceEngine;
pub use health::HealthChecker;
pub use store::ModelStore;
