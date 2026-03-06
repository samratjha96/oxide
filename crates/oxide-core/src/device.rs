use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a device.
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeviceId(pub String);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for DeviceId {
    fn from(s: &str) -> Self {
        DeviceId(s.to_string())
    }
}

impl DeviceId {
    /// Generate a new random device ID.
    pub fn generate() -> Self {
        DeviceId(format!("device-{}", Uuid::new_v4().as_simple()))
    }
}

/// Device health and operational status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceStatus {
    /// Device is online and operational.
    Online,
    /// Device is offline / unreachable.
    Offline,
    /// Device is performing an update.
    Updating,
    /// Device encountered an error.
    Error,
    /// Device is in an unknown state.
    Unknown,
}

impl fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceStatus::Online => write!(f, "online"),
            DeviceStatus::Offline => write!(f, "offline"),
            DeviceStatus::Updating => write!(f, "updating"),
            DeviceStatus::Error => write!(f, "error"),
            DeviceStatus::Unknown => write!(f, "unknown"),
        }
    }
}

/// Platform / architecture information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePlatform {
    /// Architecture (e.g., "aarch64", "x86_64", "armv7l").
    pub arch: String,
    /// Operating system (e.g., "linux", "macos").
    pub os: String,
    /// CPU count.
    pub cpu_count: u32,
    /// Total memory in bytes.
    pub total_memory_bytes: u64,
}

impl DevicePlatform {
    /// Detect platform information for the current machine.
    pub fn detect() -> Self {
        DevicePlatform {
            arch: std::env::consts::ARCH.to_string(),
            os: std::env::consts::OS.to_string(),
            cpu_count: std::thread::available_parallelism()
                .map(|p| p.get() as u32)
                .unwrap_or(1),
            total_memory_bytes: 0, // Would use sysinfo crate in production
        }
    }
}

/// Result of the last OTA update attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateResult {
    Success,
    Failed { error: String },
}

/// Represents an edge device that runs the Oxide runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Unique device identifier.
    pub id: DeviceId,
    /// Human-readable name.
    pub name: String,
    /// Current operational status.
    pub status: DeviceStatus,
    /// Platform information.
    pub platform: DevicePlatform,
    /// Currently deployed model (if any).
    pub current_model: Option<crate::model::ModelId>,
    /// Currently deployed model version.
    pub current_model_version: Option<crate::model::ModelVersion>,
    /// Model the control plane wants this device to run.
    #[serde(default)]
    pub assigned_model: Option<crate::model::ModelId>,
    /// Version the control plane wants this device to run.
    #[serde(default)]
    pub assigned_model_version: Option<crate::model::ModelVersion>,
    /// Fleet this device belongs to (if any).
    pub fleet_id: Option<crate::fleet::FleetId>,
    /// Last time device checked in.
    pub last_heartbeat: Option<chrono::DateTime<chrono::Utc>>,
    /// Result of the last update attempt.
    #[serde(default)]
    pub last_update_result: Option<UpdateResult>,
    /// Device tags for grouping/filtering.
    pub tags: std::collections::HashMap<String, String>,
}

impl Device {
    /// Create a new device with the given ID and name.
    pub fn new(id: DeviceId, name: impl Into<String>) -> Self {
        Device {
            id,
            name: name.into(),
            status: DeviceStatus::Unknown,
            platform: DevicePlatform::detect(),
            current_model: None,
            current_model_version: None,
            assigned_model: None,
            assigned_model_version: None,
            fleet_id: None,
            last_heartbeat: None,
            last_update_result: None,
            tags: std::collections::HashMap::new(),
        }
    }

    /// Check if the device is healthy (online and recent heartbeat).
    pub fn is_healthy(&self) -> bool {
        if self.status != DeviceStatus::Online {
            return false;
        }
        if let Some(heartbeat) = &self.last_heartbeat {
            let elapsed = chrono::Utc::now() - *heartbeat;
            // Consider device unhealthy if no heartbeat in 5 minutes
            elapsed.num_seconds() < 300
        } else {
            false
        }
    }
}

/// Lightweight metrics sent with heartbeats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicMetrics {
    #[serde(default)]
    pub inference_count: u64,
    #[serde(default)]
    pub avg_latency_us: f64,
    #[serde(default)]
    pub uptime_secs: u64,
    #[serde(default)]
    pub free_memory_bytes: Option<u64>,
}

/// Heartbeat request body sent by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    #[serde(default)]
    pub current_model: Option<crate::model::ModelId>,
    #[serde(default)]
    pub current_model_version: Option<crate::model::ModelVersion>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub last_update_result: Option<UpdateResult>,
    #[serde(default)]
    pub metrics: Option<BasicMetrics>,
}

/// Heartbeat response returned to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub status: String,
    #[serde(default)]
    pub assigned_model: Option<crate::model::ModelId>,
    #[serde(default)]
    pub assigned_model_version: Option<crate::model::ModelVersion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_creation() {
        let device = Device::new(DeviceId::from("pi-01"), "Raspberry Pi Camera");
        assert_eq!(device.id.0, "pi-01");
        assert_eq!(device.name, "Raspberry Pi Camera");
        assert_eq!(device.status, DeviceStatus::Unknown);
    }

    #[test]
    fn test_device_id_generation() {
        let id1 = DeviceId::generate();
        let id2 = DeviceId::generate();
        assert_ne!(id1, id2);
        assert!(id1.0.starts_with("device-"));
    }

    #[test]
    fn test_device_health_check() {
        let mut device = Device::new(DeviceId::from("pi-01"), "Test");

        // Unknown status = not healthy
        assert!(!device.is_healthy());

        // Online but no heartbeat = not healthy
        device.status = DeviceStatus::Online;
        assert!(!device.is_healthy());

        // Online with recent heartbeat = healthy
        device.last_heartbeat = Some(chrono::Utc::now());
        assert!(device.is_healthy());
    }

    #[test]
    fn test_platform_detect() {
        let platform = DevicePlatform::detect();
        assert!(!platform.arch.is_empty());
        assert!(!platform.os.is_empty());
        assert!(platform.cpu_count > 0);
    }

    #[test]
    fn test_device_serialization() {
        let device = Device::new(DeviceId::from("pi-01"), "Test Device");
        let json = serde_json::to_string(&device).unwrap();
        let deserialized: Device = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id.0, "pi-01");
    }
}
