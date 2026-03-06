//! # Oxide Control
//!
//! Control plane for fleet management, device registry,
//! and centralized deployment orchestration.

pub mod fleet_manager;
pub mod registry;
pub mod server;

pub use fleet_manager::FleetManager;
pub use registry::DeviceRegistry;
pub use server::ControlPlaneServer;
