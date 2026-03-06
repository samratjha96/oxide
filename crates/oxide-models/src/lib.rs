//! # Oxide Models
//!
//! Model format parsers and loaders for the Oxide edge AI runtime.
//! Supports ONNX (via tract) with extensibility for additional formats.

pub mod onnx;

pub use onnx::OnnxModel;
