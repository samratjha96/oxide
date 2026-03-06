//! `oxide serve` — Start the control plane server.

use oxide_control::campaign::CampaignStore;
use oxide_control::fleet_manager::FleetManager;
use oxide_control::model_store::ControlPlaneModelStore;
use oxide_control::registry::DeviceRegistry;
use oxide_control::server::{ControlPlaneServer, ControlPlaneState};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub async fn execute(host: &str, port: u16) -> anyhow::Result<()> {
    println!("⚡ oxide control plane");
    println!("───────────────────────");

    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;

    let registry = Arc::new(DeviceRegistry::with_persistence(
        &data_dir.join("devices.json"),
    )?);
    let fleet_manager = Arc::new(FleetManager::new(registry.clone()));
    let model_store = Arc::new(RwLock::new(ControlPlaneModelStore::open(
        &data_dir.join("models"),
    )?));

    let state = Arc::new(ControlPlaneState {
        registry,
        fleet_manager,
        model_store,
        campaigns: Arc::new(RwLock::new(CampaignStore::new())),
    });

    let app = ControlPlaneServer::router(state);
    let addr = format!("{host}:{port}");

    println!("  listening: http://{addr}");
    println!("  data dir:  {}", data_dir.display());
    println!();
    println!("  endpoints:");
    println!("    GET  /health                                   — health check");
    println!("    POST /api/v1/devices                           — register device");
    println!("    GET  /api/v1/devices                           — list devices");
    println!("    GET  /api/v1/devices/:id                       — get device");
    println!("    DEL  /api/v1/devices/:id                       — unregister");
    println!("    POST /api/v1/devices/:id/heartbeat             — heartbeat");
    println!("    POST /api/v1/fleets                            — create fleet");
    println!("    GET  /api/v1/fleets/:id                        — get fleet");
    println!("    POST /api/v1/fleets/:id/devices/:did           — add device");
    println!("    POST /api/v1/fleets/:id/deploy                 — deploy to fleet");
    println!("    GET  /api/v1/fleets/:id/status                 — fleet status");
    println!("    POST /api/v1/models/:id/versions/:ver          — upload model");
    println!("    GET  /api/v1/models/:id/versions/:ver/download — download model");
    println!("    GET  /api/v1/models/:id/versions/:ver/meta     — model metadata");
    println!("    POST /api/v1/campaigns                          — create campaign");
    println!("    GET  /api/v1/campaigns                          — list campaigns");
    println!("    GET  /api/v1/campaigns/:id                      — campaign status");
    println!("    POST /api/v1/campaigns/:id/pause                — pause campaign");
    println!("    POST /api/v1/campaigns/:id/resume               — resume campaign");
    println!("    POST /api/v1/campaigns/:id/abort                — abort campaign");
    println!();
    println!("  press ctrl-c to stop");
    println!();

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Control plane started on {addr}");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Control plane shut down cleanly");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => { info!("received ctrl-c, shutting down..."); },
        () = terminate => { info!("received SIGTERM, shutting down..."); },
    }
}
