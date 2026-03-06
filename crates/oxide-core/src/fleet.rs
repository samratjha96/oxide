use serde::{Deserialize, Serialize};
use std::fmt;

use crate::device::DeviceId;
use crate::model::ModelVersion;

/// Unique identifier for a fleet.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct FleetId(pub String);

impl fmt::Display for FleetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for FleetId {
    fn from(s: &str) -> Self {
        FleetId(s.to_string())
    }
}

/// Strategy for rolling out updates to a fleet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutStrategy {
    /// Deploy to all devices simultaneously.
    AllAtOnce,
    /// Canary rollout: deploy to a percentage, wait, then expand.
    Canary {
        /// Percentage steps (e.g., [5, 25, 50, 100]).
        stages: Vec<u8>,
        /// Seconds to wait between stages.
        wait_seconds: u64,
        /// Health check to pass before proceeding.
        health_check: Option<String>,
    },
    /// Rolling update: deploy to N devices at a time.
    Rolling {
        /// Max devices to update concurrently.
        batch_size: usize,
        /// Seconds to wait between batches.
        wait_seconds: u64,
    },
}

impl Default for RolloutStrategy {
    fn default() -> Self {
        RolloutStrategy::AllAtOnce
    }
}

/// Status of an ongoing rollout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RolloutStatus {
    /// Target model version being deployed.
    pub target_version: ModelVersion,
    /// Rollout strategy being used.
    pub strategy: RolloutStrategy,
    /// Total number of devices in the fleet.
    pub total_devices: usize,
    /// Number of devices successfully updated.
    pub updated_devices: usize,
    /// Number of devices that failed to update.
    pub failed_devices: usize,
    /// Number of devices pending update.
    pub pending_devices: usize,
    /// Current stage (for canary rollouts).
    pub current_stage: Option<usize>,
    /// Whether rollout is complete.
    pub complete: bool,
    /// Whether rollout was rolled back.
    pub rolled_back: bool,
}

/// A fleet of edge devices.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fleet {
    /// Unique fleet identifier.
    pub id: FleetId,
    /// Human-readable fleet name.
    pub name: String,
    /// Description of what this fleet does.
    pub description: Option<String>,
    /// Device IDs in this fleet.
    pub devices: Vec<DeviceId>,
    /// Default rollout strategy for this fleet.
    pub default_strategy: RolloutStrategy,
    /// Tags for filtering.
    pub tags: std::collections::HashMap<String, String>,
}

impl Fleet {
    /// Create a new fleet.
    pub fn new(id: FleetId, name: impl Into<String>) -> Self {
        Fleet {
            id,
            name: name.into(),
            description: None,
            devices: Vec::new(),
            default_strategy: RolloutStrategy::default(),
            tags: std::collections::HashMap::new(),
        }
    }

    /// Add a device to the fleet.
    pub fn add_device(&mut self, device_id: DeviceId) {
        if !self.devices.contains(&device_id) {
            self.devices.push(device_id);
        }
    }

    /// Remove a device from the fleet.
    pub fn remove_device(&mut self, device_id: &DeviceId) -> bool {
        let len_before = self.devices.len();
        self.devices.retain(|id| id != device_id);
        self.devices.len() < len_before
    }

    /// Number of devices in the fleet.
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fleet_creation() {
        let fleet = Fleet::new(FleetId::from("prod-warehouse"), "Production Warehouse");
        assert_eq!(fleet.id.0, "prod-warehouse");
        assert_eq!(fleet.name, "Production Warehouse");
        assert_eq!(fleet.device_count(), 0);
    }

    #[test]
    fn test_fleet_device_management() {
        let mut fleet = Fleet::new(FleetId::from("test"), "Test Fleet");
        let d1 = DeviceId::from("pi-01");
        let d2 = DeviceId::from("pi-02");

        fleet.add_device(d1.clone());
        fleet.add_device(d2.clone());
        assert_eq!(fleet.device_count(), 2);

        // No duplicates
        fleet.add_device(d1.clone());
        assert_eq!(fleet.device_count(), 2);

        // Remove
        assert!(fleet.remove_device(&d1));
        assert_eq!(fleet.device_count(), 1);

        // Remove non-existent
        assert!(!fleet.remove_device(&DeviceId::from("pi-99")));
    }

    #[test]
    fn test_canary_rollout_serialization() {
        let strategy = RolloutStrategy::Canary {
            stages: vec![5, 25, 50, 100],
            wait_seconds: 300,
            health_check: Some("inference_latency < 50ms".to_string()),
        };
        let json = serde_json::to_string(&strategy).unwrap();
        assert!(json.contains("canary"));
        let deserialized: RolloutStrategy = serde_json::from_str(&json).unwrap();
        if let RolloutStrategy::Canary { stages, .. } = deserialized {
            assert_eq!(stages, vec![5, 25, 50, 100]);
        } else {
            panic!("Wrong variant");
        }
    }
}
