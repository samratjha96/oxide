//! # Oxide Control
//!
//! Control plane for fleet management, device registry,
//! and centralized deployment orchestration.

#![deny(unsafe_code)]

pub mod fleet_manager;
pub mod model_store;
pub mod registry;
pub mod server;

pub use fleet_manager::FleetManager;
pub use model_store::ControlPlaneModelStore;
pub use registry::DeviceRegistry;
pub use server::ControlPlaneServer;
