//! Fleet manager: coordinates deployments across fleets of devices.

use oxide_core::device::{DeviceId, DeviceStatus};
use oxide_core::error::{OxideError, Result};
use oxide_core::fleet::{Fleet, FleetId, RolloutStrategy};
use oxide_core::model::{ModelId, ModelVersion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::info;

use crate::registry::DeviceRegistry;

/// Manages fleets and orchestrates deployments.
pub struct FleetManager {
    fleets: Arc<RwLock<HashMap<FleetId, Fleet>>>,
    registry: Arc<DeviceRegistry>,
    persist_path: Option<PathBuf>,
}

/// Deployment request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentRequest {
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub fleet_id: FleetId,
    pub strategy: RolloutStrategy,
}

/// Deployment result for a single device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceDeployResult {
    pub device_id: DeviceId,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a fleet deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentResult {
    pub fleet_id: FleetId,
    pub model_id: ModelId,
    pub model_version: ModelVersion,
    pub total_devices: usize,
    pub successful: usize,
    pub failed: usize,
    pub device_results: Vec<DeviceDeployResult>,
}

impl FleetManager {
    /// Create a new in-memory fleet manager.
    pub fn new(registry: Arc<DeviceRegistry>) -> Self {
        FleetManager {
            fleets: Arc::new(RwLock::new(HashMap::new())),
            registry,
            persist_path: None,
        }
    }

    /// Create a fleet manager with file persistence.
    pub fn with_persistence(
        registry: Arc<DeviceRegistry>,
        path: &std::path::Path,
    ) -> Result<Self> {
        let fleets = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let map: HashMap<FleetId, Fleet> = serde_json::from_str(&content)
                .map_err(|e| OxideError::Serialization(e.to_string()))?;
            info!("Loaded {} fleets from store", map.len());
            map
        } else {
            HashMap::new()
        };

        Ok(FleetManager {
            fleets: Arc::new(RwLock::new(fleets)),
            registry,
            persist_path: Some(path.to_path_buf()),
        })
    }

    /// Create a new fleet.
    pub fn create_fleet(&self, fleet: Fleet) -> Result<()> {
        let id = fleet.id.clone();
        info!("Creating fleet: {} ({})", fleet.name, id);

        let mut fleets = self.fleets.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        fleets.insert(id, fleet);
        drop(fleets);

        self.persist()?;
        Ok(())
    }

    /// Get a fleet by ID.
    pub fn get_fleet(&self, id: &FleetId) -> Result<Fleet> {
        let fleets = self.fleets.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        fleets
            .get(id)
            .cloned()
            .ok_or_else(|| OxideError::Fleet(format!("Fleet not found: {}", id)))
    }

    /// List all fleets.
    pub fn list_fleets(&self) -> Result<Vec<Fleet>> {
        let fleets = self.fleets.read().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;
        Ok(fleets.values().cloned().collect())
    }

    /// Add a device to a fleet.
    pub fn add_device_to_fleet(&self, fleet_id: &FleetId, device_id: DeviceId) -> Result<()> {
        let mut fleets = self.fleets.write().map_err(|e| {
            OxideError::Internal(format!("Lock poisoned: {}", e))
        })?;

        let fleet = fleets
            .get_mut(fleet_id)
            .ok_or_else(|| OxideError::Fleet(format!("Fleet not found: {}", fleet_id)))?;

        // Verify device exists in registry
        self.registry.get(&device_id)?;

        fleet.add_device(device_id.clone());
        info!("Added device '{}' to fleet '{}'", device_id, fleet_id);
        drop(fleets);

        self.persist()?;
        Ok(())
    }

    /// Execute a deployment across a fleet.
    ///
    /// This simulates the deployment process - in production this would
    /// communicate with each device's OTA updater.
    pub fn deploy(&self, request: &DeploymentRequest) -> Result<DeploymentResult> {
        let fleet = self.get_fleet(&request.fleet_id)?;

        info!(
            "Deploying model '{}' version '{}' to fleet '{}' ({} devices)",
            request.model_id,
            request.model_version,
            request.fleet_id,
            fleet.devices.len()
        );

        let mut device_results = Vec::new();
        let mut successful = 0;
        let mut failed = 0;

        // Get the devices to deploy to based on strategy
        let deploy_devices = match &request.strategy {
            RolloutStrategy::AllAtOnce => fleet.devices,
            RolloutStrategy::Canary { stages, .. } => {
                if let Some(&first_pct) = stages.first() {
                    let count =
                        (fleet.devices.len() as f64 * first_pct as f64 / 100.0).ceil() as usize;
                    fleet.devices.iter().take(count).cloned().collect()
                } else {
                    fleet.devices
                }
            }
            RolloutStrategy::Rolling { batch_size, .. } => {
                fleet.devices.iter().take(*batch_size).cloned().collect()
            }
        };

        for device_id in &deploy_devices {
            match self.registry.get(device_id) {
                Ok(device) => {
                    if device.status == DeviceStatus::Online
                        || device.status == DeviceStatus::Unknown
                    {
                        successful += 1;
                        device_results.push(DeviceDeployResult {
                            device_id: device_id.clone(),
                            success: true,
                            error: None,
                        });
                    } else {
                        failed += 1;
                        device_results.push(DeviceDeployResult {
                            device_id: device_id.clone(),
                            success: false,
                            error: Some(format!("Device status: {}", device.status)),
                        });
                    }
                }
                Err(e) => {
                    failed += 1;
                    device_results.push(DeviceDeployResult {
                        device_id: device_id.clone(),
                        success: false,
                        error: Some(e.to_string()),
                    });
                }
            }
        }

        let result = DeploymentResult {
            fleet_id: request.fleet_id.clone(),
            model_id: request.model_id.clone(),
            model_version: request.model_version.clone(),
            total_devices: deploy_devices.len(),
            successful,
            failed,
            device_results,
        };

        info!(
            "Deployment complete: {}/{} successful",
            successful,
            deploy_devices.len()
        );

        Ok(result)
    }

    /// Get fleet status summary.
    pub fn fleet_status(&self, fleet_id: &FleetId) -> Result<FleetStatusSummary> {
        let fleet = self.get_fleet(fleet_id)?;
        let mut online = 0;
        let mut offline = 0;
        let mut error = 0;
        let mut unknown = 0;

        for device_id in &fleet.devices {
            if let Ok(device) = self.registry.get(device_id) {
                match device.status {
                    DeviceStatus::Online => online += 1,
                    DeviceStatus::Offline => offline += 1,
                    DeviceStatus::Error => error += 1,
                    _ => unknown += 1,
                }
            } else {
                unknown += 1;
            }
        }

        Ok(FleetStatusSummary {
            fleet_id: fleet_id.clone(),
            fleet_name: fleet.name.clone(),
            total_devices: fleet.devices.len(),
            online,
            offline,
            error,
            unknown,
        })
    }

    fn persist(&self) -> Result<()> {
        if let Some(path) = &self.persist_path {
            let fleets = self.fleets.read().map_err(|e| {
                OxideError::Internal(format!("Lock poisoned: {}", e))
            })?;
            let content = serde_json::to_string_pretty(&*fleets)?;
            std::fs::write(path, content)?;
        }
        Ok(())
    }
}

/// Summary of fleet health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetStatusSummary {
    pub fleet_id: FleetId,
    pub fleet_name: String,
    pub total_devices: usize,
    pub online: usize,
    pub offline: usize,
    pub error: usize,
    pub unknown: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxide_core::device::Device;

    fn setup() -> (Arc<DeviceRegistry>, FleetManager) {
        let registry = Arc::new(DeviceRegistry::new());
        let manager = FleetManager::new(registry.clone());
        (registry, manager)
    }

    #[test]
    fn test_create_fleet() {
        let (_, manager) = setup();
        let fleet = Fleet::new(FleetId::from("prod"), "Production");
        manager.create_fleet(fleet).unwrap();

        let got = manager.get_fleet(&FleetId::from("prod")).unwrap();
        assert_eq!(got.name, "Production");
    }

    #[test]
    fn test_add_device_to_fleet() {
        let (registry, manager) = setup();

        let device = Device::new(DeviceId::from("pi-01"), "Pi 1");
        registry.register(device).unwrap();

        let fleet = Fleet::new(FleetId::from("prod"), "Production");
        manager.create_fleet(fleet).unwrap();
        manager
            .add_device_to_fleet(&FleetId::from("prod"), DeviceId::from("pi-01"))
            .unwrap();

        let fleet = manager.get_fleet(&FleetId::from("prod")).unwrap();
        assert_eq!(fleet.device_count(), 1);
    }

    #[test]
    fn test_deploy_all_at_once() {
        let (registry, manager) = setup();

        for i in 0..5 {
            let mut device = Device::new(
                DeviceId::from(format!("pi-{:02}", i).as_str()),
                format!("Pi {}", i),
            );
            device.status = DeviceStatus::Online;
            registry.register(device).unwrap();
        }

        let mut fleet = Fleet::new(FleetId::from("prod"), "Production");
        for i in 0..5 {
            fleet.add_device(DeviceId::from(format!("pi-{:02}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        let request = DeploymentRequest {
            model_id: ModelId::from("face-detection"),
            model_version: ModelVersion::from("v2.0.0"),
            fleet_id: FleetId::from("prod"),
            strategy: RolloutStrategy::AllAtOnce,
        };

        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 5);
        assert_eq!(result.successful, 5);
        assert_eq!(result.failed, 0);
    }

    #[test]
    fn test_deploy_canary() {
        let (registry, manager) = setup();

        for i in 0..10 {
            let mut device = Device::new(
                DeviceId::from(format!("pi-{:02}", i).as_str()),
                format!("Pi {}", i),
            );
            device.status = DeviceStatus::Online;
            registry.register(device).unwrap();
        }

        let mut fleet = Fleet::new(FleetId::from("prod"), "Production");
        for i in 0..10 {
            fleet.add_device(DeviceId::from(format!("pi-{:02}", i).as_str()));
        }
        manager.create_fleet(fleet).unwrap();

        let request = DeploymentRequest {
            model_id: ModelId::from("face-detection"),
            model_version: ModelVersion::from("v2.0.0"),
            fleet_id: FleetId::from("prod"),
            strategy: RolloutStrategy::Canary {
                stages: vec![10, 50, 100],
                wait_seconds: 60,
                health_check: None,
            },
        };

        let result = manager.deploy(&request).unwrap();
        assert_eq!(result.total_devices, 1);
        assert_eq!(result.successful, 1);
    }

    #[test]
    fn test_fleet_status() {
        let (registry, manager) = setup();

        let mut d1 = Device::new(DeviceId::from("pi-01"), "Pi 1");
        d1.status = DeviceStatus::Online;
        registry.register(d1).unwrap();

        let mut d2 = Device::new(DeviceId::from("pi-02"), "Pi 2");
        d2.status = DeviceStatus::Offline;
        registry.register(d2).unwrap();

        let mut fleet = Fleet::new(FleetId::from("prod"), "Production");
        fleet.add_device(DeviceId::from("pi-01"));
        fleet.add_device(DeviceId::from("pi-02"));
        manager.create_fleet(fleet).unwrap();

        let status = manager.fleet_status(&FleetId::from("prod")).unwrap();
        assert_eq!(status.total_devices, 2);
        assert_eq!(status.online, 1);
        assert_eq!(status.offline, 1);
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::TempDir::new().unwrap();
        let fleet_path = dir.path().join("fleets.json");

        let registry = Arc::new(DeviceRegistry::new());

        // Create and save
        {
            let manager =
                FleetManager::with_persistence(registry.clone(), &fleet_path).unwrap();
            let fleet = Fleet::new(FleetId::from("prod"), "Production");
            manager.create_fleet(fleet).unwrap();
        }

        // Reload and verify
        {
            let manager =
                FleetManager::with_persistence(registry, &fleet_path).unwrap();
            let fleets = manager.list_fleets().unwrap();
            assert_eq!(fleets.len(), 1);
            let got = manager.get_fleet(&FleetId::from("prod")).unwrap();
            assert_eq!(got.name, "Production");
        }
    }
}
