//! `oxide device` — Device management commands.

use oxide_control::registry::DeviceRegistry;
use oxide_core::device::{Device, DeviceId};

pub fn list() -> anyhow::Result<()> {
    let registry = load_registry()?;
    let devices = registry.list()?;

    if devices.is_empty() {
        println!("No devices registered.");
        println!("Use 'oxide device register <id> --name <name>' to register a device.");
        return Ok(());
    }

    println!(
        "{:<20} {:<25} {:<10} {:<15}",
        "ID", "NAME", "STATUS", "MODEL"
    );
    println!("{}", "─".repeat(70));

    for device in &devices {
        let model = device
            .current_model
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "{:<20} {:<25} {:<10} {:<15}",
            device.id, device.name, device.status, model
        );
    }

    println!("\n{} device(s)", devices.len());
    Ok(())
}

pub fn register(id: &str, name: &str) -> anyhow::Result<()> {
    let registry = load_registry()?;
    let device = Device::new(DeviceId::from(id), name);
    registry.register(device)?;
    println!("registered device '{}' ({})", name, id);
    Ok(())
}

pub fn status(id: &str) -> anyhow::Result<()> {
    let registry = load_registry()?;
    let device = registry.get(&DeviceId::from(id))?;

    println!("oxide device status {}", id);
    println!("  id:        {}", device.id);
    println!("  name:      {}", device.name);
    println!("  status:    {}", device.status);
    println!("  platform:  {} / {}", device.platform.os, device.platform.arch);
    println!("  cpus:      {}", device.platform.cpu_count);
    println!(
        "  model:     {}",
        device
            .current_model
            .as_ref()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "  fleet:     {}",
        device
            .fleet_id
            .as_ref()
            .map(|f| f.to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    if let Some(hb) = &device.last_heartbeat {
        println!("  heartbeat: {}", hb);
    }

    Ok(())
}

fn load_registry() -> anyhow::Result<DeviceRegistry> {
    let data_dir = std::env::current_dir()?.join(".oxide");
    std::fs::create_dir_all(&data_dir)?;
    let registry_path = data_dir.join("devices.json");
    Ok(DeviceRegistry::with_persistence(&registry_path)?)
}
