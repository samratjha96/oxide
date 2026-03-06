use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Top-level Oxide configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OxideConfig {
    /// Runtime configuration.
    pub runtime: RuntimeConfig,
    /// Security configuration.
    pub security: SecurityConfig,
    /// Network configuration.
    pub network: NetworkConfig,
    /// Telemetry configuration.
    pub telemetry: TelemetryConfig,
}

impl OxideConfig {
    /// Load configuration from a TOML file.
    pub fn from_file(path: &Path) -> crate::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    /// Save configuration to a TOML file.
    pub fn to_file(&self, path: &Path) -> crate::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Directory where models are stored.
    pub model_dir: PathBuf,
    /// Maximum memory budget for models in bytes.
    pub max_memory_bytes: u64,
    /// Number of inference threads.
    pub num_threads: usize,
    /// Enable SIMD optimizations.
    pub enable_simd: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            model_dir: PathBuf::from("./models"),
            max_memory_bytes: 50 * 1024 * 1024, // 50MB
            num_threads: 0,                       // 0 = auto-detect
            enable_simd: true,
        }
    }
}

/// Security configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Enable model encryption at rest.
    pub encrypt_models: bool,
    /// Path to the encryption key file.
    pub key_file: Option<PathBuf>,
    /// Enable mTLS for device-to-control-plane communication.
    pub enable_mtls: bool,
    /// Path to TLS certificate.
    pub cert_file: Option<PathBuf>,
    /// Path to TLS private key.
    pub key_pem_file: Option<PathBuf>,
    /// Path to CA certificate for mTLS verification.
    pub ca_file: Option<PathBuf>,
}

/// Network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Control plane URL.
    pub control_plane_url: Option<String>,
    /// Device listen address for the local API.
    pub listen_addr: String,
    /// Port for the local API.
    pub listen_port: u16,
    /// Heartbeat interval in seconds.
    pub heartbeat_interval_secs: u64,
    /// Connection timeout in seconds.
    pub connect_timeout_secs: u64,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            control_plane_url: None,
            listen_addr: "0.0.0.0".to_string(),
            listen_port: 8090,
            heartbeat_interval_secs: 30,
            connect_timeout_secs: 10,
        }
    }
}

/// Telemetry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Enable telemetry collection.
    pub enabled: bool,
    /// Reporting interval in seconds.
    pub report_interval_secs: u64,
    /// Maximum telemetry queue size (for offline buffering).
    pub max_queue_size: usize,
    /// Enable Prometheus metrics endpoint.
    pub enable_prometheus: bool,
    /// Prometheus metrics port.
    pub prometheus_port: u16,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        TelemetryConfig {
            enabled: true,
            report_interval_secs: 60,
            max_queue_size: 1000,
            enable_prometheus: true,
            prometheus_port: 9090,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = OxideConfig::default();
        assert_eq!(config.runtime.max_memory_bytes, 50 * 1024 * 1024);
        assert!(config.runtime.enable_simd);
        assert!(!config.security.encrypt_models);
        assert_eq!(config.network.listen_port, 8090);
        assert!(config.telemetry.enabled);
    }

    #[test]
    fn test_config_serialization_roundtrip() {
        let config = OxideConfig::default();
        let toml_str =
            toml::to_string_pretty(&config).expect("Failed to serialize config to TOML");
        let deserialized: OxideConfig =
            toml::from_str(&toml_str).expect("Failed to deserialize config from TOML");
        assert_eq!(deserialized.runtime.max_memory_bytes, config.runtime.max_memory_bytes);
        assert_eq!(deserialized.network.listen_port, config.network.listen_port);
    }

    #[test]
    fn test_config_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oxide.toml");
        let config = OxideConfig::default();
        config.to_file(&path).unwrap();
        let loaded = OxideConfig::from_file(&path).unwrap();
        assert_eq!(loaded.runtime.max_memory_bytes, config.runtime.max_memory_bytes);
    }
}
