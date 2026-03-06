//! `oxide serve` — Start the control plane server.

use oxide_control::fleet_manager::FleetManager;
use oxide_control::registry::DeviceRegistry;
use oxide_control::server::{ControlPlaneServer, ControlPlaneState};
use std::sync::Arc;
use tracing::info;

pub async fn execute(host: &str, port: u16) -> anyhow::Result<()> {
    println!("oxide control plane");
    println!("───────────────────");

    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;

    let registry = Arc::new(DeviceRegistry::with_persistence(
        &data_dir.join("devices.json"),
    )?);
    let fleet_manager = Arc::new(FleetManager::new(registry.clone()));

    let state = Arc::new(ControlPlaneState {
        registry,
        fleet_manager,
    });

    let app = ControlPlaneServer::router(state);
    let addr = format!("{}:{}", host, port);

    println!("  Listening on: http://{}", addr);
    println!("  Data dir:     {}", data_dir.display());
    println!();
    println!("  Endpoints:");
    println!("    GET  /health                          — Health check");
    println!("    GET  /api/v1/devices                  — List devices");
    println!("    POST /api/v1/devices                  — Register device");
    println!("    GET  /api/v1/devices/:id              — Get device");
    println!("    DEL  /api/v1/devices/:id              — Unregister device");
    println!("    POST /api/v1/devices/:id/heartbeat    — Device heartbeat");
    println!("    GET  /api/v1/fleets                   — List fleets");
    println!("    POST /api/v1/fleets                   — Create fleet");
    println!("    GET  /api/v1/fleets/:id               — Get fleet");
    println!("    POST /api/v1/fleets/:id/devices/:did  — Add device to fleet");
    println!("    POST /api/v1/fleets/:id/deploy        — Deploy to fleet");
    println!("    GET  /api/v1/fleets/:id/status        — Fleet status");
    println!();

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Control plane started on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
