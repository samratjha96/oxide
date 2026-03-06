//! # Oxide Models
//!
//! Model format parsers and loaders for the Oxide edge AI runtime.
//! Supports ONNX (via tract) with extensibility for additional formats.

#![deny(unsafe_code)]

pub mod onnx;

pub use onnx::OnnxModel;
