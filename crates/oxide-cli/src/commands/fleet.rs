//! `oxide fleet` — Fleet management commands.

use oxide_control::fleet_manager::FleetManager;
use oxide_control::registry::DeviceRegistry;
use oxide_core::fleet::{Fleet, FleetId};
use std::sync::Arc;

pub fn list() -> anyhow::Result<()> {
    println!("⚡ Oxide — Fleets");
    println!("─────────────────");

    let (_registry, manager) = load_fleet_manager()?;
    let fleets = manager.list_fleets()?;

    if fleets.is_empty() {
        println!("  No fleets created.");
        println!("  Use 'oxide fleet create <id> --name <name>' to create a fleet.");
        return Ok(());
    }

    println!("{:<20} {:<25} {:<10}", "ID", "Name", "Devices");
    println!("{}", "─".repeat(55));

    for fleet in &fleets {
        println!(
            "{:<20} {:<25} {:<10}",
            fleet.id,
            fleet.name,
            fleet.device_count()
        );
    }

    Ok(())
}

pub fn create(id: &str, name: &str) -> anyhow::Result<()> {
    let (_registry, manager) = load_fleet_manager()?;
    let fleet = Fleet::new(FleetId::from(id), name);
    manager.create_fleet(fleet)?;
    println!("✅ Fleet created: {} ({})", name, id);
    Ok(())
}

pub fn status(id: &str) -> anyhow::Result<()> {
    let (_registry, manager) = load_fleet_manager()?;
    let fleet_id = FleetId::from(id);
    let status = manager.fleet_status(&fleet_id)?;

    println!("⚡ Oxide — Fleet Status");
    println!("──────────────────────");
    println!("  Fleet:   {} ({})", status.fleet_name, status.fleet_id);
    println!("  Devices: {}", status.total_devices);
    println!("  Online:  {}", status.online);
    println!("  Offline: {}", status.offline);
    println!("  Error:   {}", status.error);
    println!("  Unknown: {}", status.unknown);

    Ok(())
}

fn load_fleet_manager() -> anyhow::Result<(Arc<DeviceRegistry>, FleetManager)> {
    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;
    let registry_path = data_dir.join("devices.json");
    let registry = Arc::new(DeviceRegistry::with_persistence(&registry_path)?);
    let manager = FleetManager::new(registry.clone());
    Ok((registry, manager))
}
