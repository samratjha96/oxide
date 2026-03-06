//! `oxide fleet` — Fleet management commands.

use oxide_control::fleet_manager::FleetManager;
use oxide_control::registry::DeviceRegistry;
use oxide_core::fleet::{Fleet, FleetId};
use std::sync::Arc;

pub fn list() -> anyhow::Result<()> {
    let (_registry, manager) = load_fleet_manager()?;
    let fleets = manager.list_fleets()?;

    if fleets.is_empty() {
        println!("no fleets created");
        println!("use 'oxide fleet create <id> --name <name>' to create one");
        return Ok(());
    }

    println!(
        "{:<20} {:<25} {:<10}",
        "ID", "NAME", "DEVICES"
    );
    println!("{}", "─".repeat(55));

    for fleet in &fleets {
        println!(
            "{:<20} {:<25} {:<10}",
            fleet.id,
            fleet.name,
            fleet.device_count()
        );
    }

    println!("\n{} fleet(s)", fleets.len());
    Ok(())
}

pub fn create(id: &str, name: &str) -> anyhow::Result<()> {
    let (_registry, manager) = load_fleet_manager()?;
    let fleet = Fleet::new(FleetId::from(id), name);
    manager.create_fleet(fleet)?;
    println!("created fleet '{}' ({})", name, id);
    Ok(())
}

pub fn status(id: &str) -> anyhow::Result<()> {
    let (_registry, manager) = load_fleet_manager()?;
    let fleet_id = FleetId::from(id);
    let status = manager.fleet_status(&fleet_id)?;

    println!("oxide fleet status {}", id);
    println!("  name:    {}", status.fleet_name);
    println!("  devices: {}", status.total_devices);
    println!("  online:  {}", status.online);
    println!("  offline: {}", status.offline);
    println!("  error:   {}", status.error);
    println!("  unknown: {}", status.unknown);

    Ok(())
}

fn load_fleet_manager() -> anyhow::Result<(Arc<DeviceRegistry>, FleetManager)> {
    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;
    let registry_path = data_dir.join("devices.json");
    let fleet_path = data_dir.join("fleets.json");
    let registry = Arc::new(DeviceRegistry::with_persistence(&registry_path)?);
    let manager = FleetManager::with_persistence(registry.clone(), &fleet_path)?;
    Ok((registry, manager))
}
