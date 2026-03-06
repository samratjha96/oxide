//! # Oxide Runtime
//!
//! Inference engine and model store for Oxide.
//! Manages model lifecycle, inference execution, metrics collection,
//! and model hot-swapping.

#![deny(unsafe_code)]

pub mod engine;
pub mod store;

pub use engine::InferenceEngine;
pub use store::ModelStore;
