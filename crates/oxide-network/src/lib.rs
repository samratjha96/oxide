//! # Oxide Network
//!
//! OTA update engine for atomic model deployment with rollback.

#![deny(unsafe_code)]

pub mod ota;

pub use ota::OtaUpdater;
