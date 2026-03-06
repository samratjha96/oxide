//! `oxide device` — Device management commands.

use oxide_control::registry::DeviceRegistry;
use oxide_core::device::{Device, DeviceId};

pub fn list() -> anyhow::Result<()> {
    println!("⚡ Oxide — Registered Devices");
    println!("────────────────────────────");

    let registry = load_registry()?;
    let devices = registry.list()?;

    if devices.is_empty() {
        println!("  No devices registered.");
        println!("  Use 'oxide device register <id> --name <name>' to register a device.");
        return Ok(());
    }

    println!(
        "  {:<20} {:<25} {:<10} {:<15}",
        "ID", "Name", "Status", "Model"
    );
    println!("  {}", "─".repeat(70));

    for device in &devices {
        let model = device
            .current_model
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".to_string());
        println!(
            "  {:<20} {:<25} {:<10} {:<15}",
            device.id, device.name, device.status, model
        );
    }

    println!("\n  Total: {} device(s)", devices.len());
    Ok(())
}

pub fn register(id: &str, name: &str) -> anyhow::Result<()> {
    let registry = load_registry()?;
    let device = Device::new(DeviceId::from(id), name);
    registry.register(device)?;
    println!("✅ Device registered: {} ({})", name, id);
    Ok(())
}

pub fn status(id: &str) -> anyhow::Result<()> {
    let registry = load_registry()?;
    let device = registry.get(&DeviceId::from(id))?;

    println!("⚡ Oxide — Device Status");
    println!("───────────────────────");
    println!("  ID:        {}", device.id);
    println!("  Name:      {}", device.name);
    println!("  Status:    {}", device.status);
    println!("  Platform:  {} / {}", device.platform.os, device.platform.arch);
    println!("  CPUs:      {}", device.platform.cpu_count);
    println!(
        "  Model:     {}",
        device
            .current_model
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "  Fleet:     {}",
        device
            .fleet_id
            .as_ref()
            .map(|f| f.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    if let Some(hb) = &device.last_heartbeat {
        println!("  Heartbeat: {}", hb);
    }

    Ok(())
}

fn load_registry() -> anyhow::Result<DeviceRegistry> {
    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;
    let registry_path = data_dir.join("devices.json");
    Ok(DeviceRegistry::with_persistence(&registry_path)?)
}
