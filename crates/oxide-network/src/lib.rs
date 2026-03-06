//! # Oxide Network
//!
//! Networking, OTA updates, and device API for the Oxide edge AI runtime.
//! Provides the HTTP API that runs on each device and the OTA update mechanism.

#![deny(unsafe_code)]

pub mod api;
pub mod ota;

pub use api::DeviceApi;
pub use ota::OtaUpdater;
