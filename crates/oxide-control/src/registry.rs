//! Device registry: tracks all registered devices and their status.

use oxide_core::device::{Device, DeviceId, DeviceStatus};
use oxide_core::error::{OxideError, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tracing::info;

/// Persistent device registry.
pub struct DeviceRegistry {
    devices: Arc<RwLock<HashMap<DeviceId, Device>>>,
    persist_path: Option<PathBuf>,
}

impl DeviceRegistry {
    /// Create a new in-memory device registry.
    pub fn new() -> Self {
        DeviceRegistry {
            devices: Arc::new(RwLock::new(HashMap::new())),
            persist_path: None,
        }
    }

    /// Create a device registry with file persistence.
    pub fn with_persistence(path: &Path) -> Result<Self> {
        let devices = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let map: HashMap<DeviceId, Device> = serde_json::from_str(&content)
                .map_err(|e| OxideError::Serialization(e.to_string()))?;
            info!("Loaded {} devices from registry", map.len());
            map
        } else {
            HashMap::new()
        };

        Ok(DeviceRegistry {
            devices: Arc::new(RwLock::new(devices)),
            persist_path: Some(path.to_path_buf()),
        })
    }

    /// Register a new device.
    pub fn register(&self, device: Device) -> Result<()> {
        let id = device.id.clone();
        info!("Registering device: {} ({})", device.name, id);

        let mut devices = self.devices.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        devices.insert(id, device);
        drop(devices);

        self.persist()?;
        Ok(())
    }

    /// Unregister a device.
    pub fn unregister(&self, id: &DeviceId) -> Result<()> {
        let mut devices = self.devices.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        if devices.remove(id).is_some() {
            info!("Unregistered device: {}", id);
            drop(devices);
            self.persist()?;
            Ok(())
        } else {
            Err(OxideError::DeviceNotFound(id.to_string()))
        }
    }

    /// Get a device by ID.
    pub fn get(&self, id: &DeviceId) -> Result<Device> {
        let devices = self.devices.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        devices
            .get(id)
            .cloned()
            .ok_or_else(|| OxideError::DeviceNotFound(id.to_string()))
    }

    /// Update a device's status.
    pub fn update_status(&self, id: &DeviceId, status: DeviceStatus) -> Result<()> {
        let mut devices = self.devices.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let device = devices
            .get_mut(id)
            .ok_or_else(|| OxideError::DeviceNotFound(id.to_string()))?;

        device.status = status;
        if status == DeviceStatus::Online {
            device.last_heartbeat = Some(chrono::Utc::now());
        }

        drop(devices);
        self.persist()?;
        Ok(())
    }

    /// Record a heartbeat from a device.
    pub fn heartbeat(&self, id: &DeviceId) -> Result<()> {
        let mut devices = self.devices.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let device = devices
            .get_mut(id)
            .ok_or_else(|| OxideError::DeviceNotFound(id.to_string()))?;

        device.last_heartbeat = Some(chrono::Utc::now());
        device.status = DeviceStatus::Online;

        Ok(())
    }

    /// List all registered devices.
    pub fn list(&self) -> Result<Vec<Device>> {
        let devices = self.devices.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        Ok(devices.values().cloned().collect())
    }

    /// List devices by status.
    pub fn list_by_status(&self, status: DeviceStatus) -> Result<Vec<Device>> {
        let devices = self.devices.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        Ok(devices
            .values()
            .filter(|d| d.status == status)
            .cloned()
            .collect())
    }

    /// Get the total count of devices.
    pub fn count(&self) -> usize {
        self.devices.read().map(|d| d.len()).unwrap_or(0)
    }

    fn persist(&self) -> Result<()> {
        if let Some(path) = &self.persist_path {
            let devices = self.devices.read().map_err(|e| {
                OxideError::Internal(format!("Lock poisoned: {}", e))
            })?;
            let content = serde_json::to_string_pretty(&*devices)?;
            std::fs::write(path, content)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_core::device::DeviceId;

    #[test]
    fn test_register_and_get() {
        let registry = DeviceRegistry::new();
        let device = Device::new(DeviceId::from("pi-01"), "Test Pi");
        registry.register(device).unwrap();

        let got = registry.get(&DeviceId::from("pi-01")).unwrap();
        assert_eq!(got.name, "Test Pi");
    }

    #[test]
    fn test_unregister() {
        let registry = DeviceRegistry::new();
        let device = Device::new(DeviceId::from("pi-01"), "Test Pi");
        registry.register(device).unwrap();
        registry.unregister(&DeviceId::from("pi-01")).unwrap();
        assert!(registry.get(&DeviceId::from("pi-01")).is_err());
    }

    #[test]
    fn test_update_status() {
        let registry = DeviceRegistry::new();
        let device = Device::new(DeviceId::from("pi-01"), "Test Pi");
        registry.register(device).unwrap();

        registry
            .update_status(&DeviceId::from("pi-01"), DeviceStatus::Online)
            .unwrap();

        let got = registry.get(&DeviceId::from("pi-01")).unwrap();
        assert_eq!(got.status, DeviceStatus::Online);
        assert!(got.last_heartbeat.is_some());
    }

    #[test]
    fn test_heartbeat() {
        let registry = DeviceRegistry::new();
        let device = Device::new(DeviceId::from("pi-01"), "Test Pi");
        registry.register(device).unwrap();

        registry.heartbeat(&DeviceId::from("pi-01")).unwrap();

        let got = registry.get(&DeviceId::from("pi-01")).unwrap();
        assert_eq!(got.status, DeviceStatus::Online);
    }

    #[test]
    fn test_list_devices() {
        let registry = DeviceRegistry::new();
        registry
            .register(Device::new(DeviceId::from("pi-01"), "Pi 1"))
            .unwrap();
        registry
            .register(Device::new(DeviceId::from("pi-02"), "Pi 2"))
            .unwrap();

        assert_eq!(registry.list().unwrap().len(), 2);
        assert_eq!(registry.count(), 2);
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("registry.json");

        // Create and save
        {
            let registry = DeviceRegistry::with_persistence(&path).unwrap();
            registry
                .register(Device::new(DeviceId::from("pi-01"), "Pi 1"))
                .unwrap();
        }

        // Reload and verify
        {
            let registry = DeviceRegistry::with_persistence(&path).unwrap();
            assert_eq!(registry.count(), 1);
            let got = registry.get(&DeviceId::from("pi-01")).unwrap();
            assert_eq!(got.name, "Pi 1");
        }
    }
}
